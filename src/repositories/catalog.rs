use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        catalog::{
            CatalogScopeItemResponse, CatalogScopeResponse, CatalogStatus, CreateItemRequest,
            CreatePriceVersionRequest, CreateProductRequest, ItemResponse, PriceTierInput,
            PriceVersionResponse, ProductResponse, UpdateItemRequest, UpdateProductRequest,
        },
        units::{CreditUnits, ItemUnitBoundary, ItemUnits},
    },
    error::{ApiError, ApiResult},
    repositories::{
        catalog_rows::{item_from_row, price_from_parts, product_from_row, tier_from_row},
        database::DatabaseRepository,
    },
};

impl DatabaseRepository {
    pub async fn insert_product(
        &self,
        request: &CreateProductRequest,
    ) -> ApiResult<ProductResponse> {
        let row = sqlx::query(
            "INSERT INTO products (product_id,name,description,usage_model,status) \
             VALUES ($1,$2,$3,$4,'INACTIVE') RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(&request.name)
        .bind(&request.description)
        .bind(request.usage_model.as_str())
        .fetch_one(&self.pool())
        .await?;
        product_from_row(&row)
    }

    pub async fn find_product(&self, product_id: Uuid) -> ApiResult<ProductResponse> {
        let row = sqlx::query("SELECT * FROM products WHERE product_id=$1")
            .bind(product_id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| missing("product", product_id))?;
        product_from_row(&row)
    }

    pub async fn update_product(
        &self,
        product_id: Uuid,
        request: &UpdateProductRequest,
    ) -> ApiResult<ProductResponse> {
        let mut transaction = self.pool().begin().await?;
        let row = update_product_row(&mut transaction, product_id, request).await?;
        refresh_scope(&mut transaction).await?;
        transaction.commit().await?;
        product_from_row(&row)
    }

    pub async fn insert_item(
        &self,
        product_id: Uuid,
        request: &CreateItemRequest,
    ) -> ApiResult<ItemResponse> {
        let row = sqlx::query(
            "INSERT INTO items (item_id,product_id,parent_item_id,name,unit_name,quantity_scale,status) \
             VALUES ($1,$2,$3,$4,$5,$6,'INACTIVE') RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(product_id)
        .bind(request.parent_item_id)
        .bind(&request.name)
        .bind(&request.unit_name)
        .bind(request.quantity_scale.map(ItemUnits::value))
        .fetch_one(&self.pool())
        .await?;
        item_from_row(&row)
    }

    pub async fn find_item(&self, item_id: Uuid) -> ApiResult<ItemResponse> {
        let row = sqlx::query("SELECT * FROM items WHERE item_id=$1")
            .bind(item_id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| missing("item", item_id))?;
        item_from_row(&row)
    }

    pub async fn item_has_published_price(&self, item_id: Uuid) -> ApiResult<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM price_versions WHERE item_id=$1 \
             AND state IN ('SCHEDULED','ACTIVE'))",
        )
        .bind(item_id)
        .fetch_one(&self.pool())
        .await?)
    }

    pub async fn update_item(
        &self,
        item_id: Uuid,
        request: &UpdateItemRequest,
    ) -> ApiResult<ItemResponse> {
        let mut transaction = self.pool().begin().await?;
        let row = update_item_row(&mut transaction, item_id, request).await?;
        refresh_scope(&mut transaction).await?;
        transaction.commit().await?;
        item_from_row(&row)
    }

    pub async fn insert_price_version(
        &self,
        item_id: Uuid,
        request: &CreatePriceVersionRequest,
    ) -> ApiResult<PriceVersionResponse> {
        let mut transaction = self.pool().begin().await?;
        let price_id = Uuid::new_v4();
        let row = insert_price_row(&mut transaction, price_id, item_id, request).await?;
        insert_tiers(&mut transaction, price_id, &request.tiers).await?;
        transaction.commit().await?;
        price_from_parts(&row, request.tiers.clone())
    }

    pub async fn find_price_version(&self, price_id: Uuid) -> ApiResult<PriceVersionResponse> {
        let row = sqlx::query("SELECT * FROM price_versions WHERE price_version_id=$1")
            .bind(price_id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| missing("price_version", price_id))?;
        let tiers = load_tiers(&self.pool(), price_id).await?;
        price_from_parts(&row, tiers)
    }

    pub async fn publish_price_version(&self, price_id: Uuid) -> ApiResult<PriceVersionResponse> {
        let mut transaction = self.pool().begin().await?;
        let draft = lock_draft_price(&mut transaction, price_id).await?;
        reject_overlapping_price(&mut transaction, &draft).await?;
        let row = mark_price_published(&mut transaction, price_id, draft.effective_from).await?;
        let tiers = load_tiers(&mut *transaction, price_id).await?;
        refresh_scope(&mut transaction).await?;
        transaction.commit().await?;
        price_from_parts(&row, tiers)
    }

    pub async fn find_current_catalog_scope(&self) -> ApiResult<CatalogScopeResponse> {
        let row = sqlx::query(
            "SELECT v.scope_version,v.fingerprint,v.created_at FROM catalog_scope_current c \
             JOIN catalog_scope_versions v ON v.scope_version=c.scope_version WHERE c.singleton",
        )
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| {
            ApiError::not_found("catalog_scope_not_found", "catalog scope does not exist")
        })?;
        let scope_version: Uuid = row.get("scope_version");
        let item_rows = sqlx::query(
            "SELECT item_id,price_version_id FROM catalog_scope_items \
             WHERE scope_version=$1 ORDER BY item_id",
        )
        .bind(scope_version)
        .fetch_all(&self.pool())
        .await?;
        Ok(CatalogScopeResponse {
            scope_version,
            fingerprint: row.get("fingerprint"),
            items: item_rows
                .iter()
                .map(|item| CatalogScopeItemResponse {
                    item_id: item.get("item_id"),
                    price_version_id: item.get("price_version_id"),
                })
                .collect(),
            created_at: row.get("created_at"),
        })
    }
}

async fn update_product_row(
    transaction: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
    request: &UpdateProductRequest,
) -> ApiResult<sqlx::postgres::PgRow> {
    sqlx::query(
        "UPDATE products SET name=COALESCE($2,name),description=COALESCE($3,description), \
         status=COALESCE($4,status),published_at=CASE WHEN $4='ACTIVE' AND published_at IS NULL \
         THEN now() ELSE published_at END,version=version+1 WHERE product_id=$1 AND version=$5 \
         RETURNING *",
    )
    .bind(product_id)
    .bind(&request.name)
    .bind(&request.description)
    .bind(request.status.map(CatalogStatus::as_str))
    .bind(request.expected_version)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| version_conflict("product", product_id, request.expected_version))
}

async fn update_item_row(
    transaction: &mut Transaction<'_, Postgres>,
    item_id: Uuid,
    request: &UpdateItemRequest,
) -> ApiResult<sqlx::postgres::PgRow> {
    sqlx::query(
        "UPDATE items SET name=COALESCE($2,name),status=COALESCE($3,status),version=version+1 \
         WHERE item_id=$1 AND version=$4 RETURNING *",
    )
    .bind(item_id)
    .bind(&request.name)
    .bind(request.status.map(CatalogStatus::as_str))
    .bind(request.expected_version)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| version_conflict("item", item_id, request.expected_version))
}

async fn insert_price_row(
    transaction: &mut Transaction<'_, Postgres>,
    price_id: Uuid,
    item_id: Uuid,
    request: &CreatePriceVersionRequest,
) -> Result<sqlx::postgres::PgRow, sqlx::Error> {
    let cycle = request.accumulation_cycle.as_ref();
    sqlx::query(
        "INSERT INTO price_versions (price_version_id,item_id,pricing_model,unit_block_size, \
         credit_units,effective_from,effective_until,accumulation_anchor_at, \
         accumulation_recurrence_rule,state) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'DRAFT') \
         RETURNING *",
    )
    .bind(price_id)
    .bind(item_id)
    .bind(request.pricing_model.as_str())
    .bind(request.unit_block_size.map(ItemUnits::value))
    .bind(request.credit_units.map(CreditUnits::value))
    .bind(request.effective_from)
    .bind(request.effective_until)
    .bind(cycle.map(|value| value.anchor_at))
    .bind(cycle.map(|value| value.recurrence_rule.as_str()))
    .fetch_one(&mut **transaction)
    .await
}

async fn insert_tiers(
    transaction: &mut Transaction<'_, Postgres>,
    price_id: Uuid,
    tiers: &[PriceTierInput],
) -> Result<(), sqlx::Error> {
    for (position, tier) in tiers.iter().enumerate() {
        sqlx::query(
            "INSERT INTO price_tiers (price_version_id,position,from_accumulated_units, \
             to_accumulated_units,unit_block_size,credit_units) VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(price_id)
        .bind(i32::try_from(position).expect("tier position must fit i32"))
        .bind(tier.from_accumulated_units.value())
        .bind(tier.to_accumulated_units.map(ItemUnitBoundary::value))
        .bind(tier.unit_block_size.value())
        .bind(tier.credit_units.value())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

struct DraftPrice {
    item_id: Uuid,
    effective_from: DateTime<Utc>,
    effective_until: Option<DateTime<Utc>>,
}

async fn lock_draft_price(
    transaction: &mut Transaction<'_, Postgres>,
    price_id: Uuid,
) -> ApiResult<DraftPrice> {
    let row = sqlx::query(
        "SELECT item_id,effective_from,effective_until,state FROM price_versions \
         WHERE price_version_id=$1 FOR UPDATE",
    )
    .bind(price_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| missing("price_version", price_id))?;
    let state: String = row.get("state");
    if state != "DRAFT" {
        return Err(ApiError::conflict(
            "price_already_published",
            format!("price_version {price_id} must be DRAFT, found {state}"),
        ));
    }
    Ok(DraftPrice {
        item_id: row.get("item_id"),
        effective_from: row.get("effective_from"),
        effective_until: row.get("effective_until"),
    })
}

async fn reject_overlapping_price(
    transaction: &mut Transaction<'_, Postgres>,
    draft: &DraftPrice,
) -> ApiResult<()> {
    sqlx::query("SELECT item_id FROM items WHERE item_id=$1 FOR UPDATE")
        .bind(draft.item_id)
        .fetch_one(&mut **transaction)
        .await?;
    let overlap: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM price_versions WHERE item_id=$1 \
         AND state IN ('SCHEDULED','ACTIVE') AND (effective_until IS NULL OR effective_until>$2) \
         AND ($3::timestamptz IS NULL OR effective_from<$3))",
    )
    .bind(draft.item_id)
    .bind(draft.effective_from)
    .bind(draft.effective_until)
    .fetch_one(&mut **transaction)
    .await?;
    if overlap {
        return Err(ApiError::conflict(
            "price_period_overlap",
            format!(
                "item {} already has a published price in the requested period",
                draft.item_id
            ),
        ));
    }
    Ok(())
}

async fn mark_price_published(
    transaction: &mut Transaction<'_, Postgres>,
    price_id: Uuid,
    effective_from: DateTime<Utc>,
) -> Result<sqlx::postgres::PgRow, sqlx::Error> {
    let state = if effective_from <= Utc::now() {
        "ACTIVE"
    } else {
        "SCHEDULED"
    };
    sqlx::query(
        "UPDATE price_versions SET state=$2,published_at=now(),version=version+1 \
         WHERE price_version_id=$1 RETURNING *",
    )
    .bind(price_id)
    .bind(state)
    .fetch_one(&mut **transaction)
    .await
}

async fn load_tiers<'a, E>(executor: E, price_id: Uuid) -> ApiResult<Vec<PriceTierInput>>
where
    E: sqlx::Executor<'a, Database = Postgres>,
{
    let rows = sqlx::query(
        "SELECT from_accumulated_units,to_accumulated_units,unit_block_size,credit_units \
         FROM price_tiers WHERE price_version_id=$1 ORDER BY position",
    )
    .bind(price_id)
    .fetch_all(executor)
    .await?;
    rows.iter().map(tier_from_row).collect()
}

async fn refresh_scope(transaction: &mut Transaction<'_, Postgres>) -> ApiResult<Uuid> {
    let rows = sqlx::query(
        "SELECT DISTINCT ON (i.item_id) i.item_id,pv.price_version_id \
         FROM items i JOIN products p ON p.product_id=i.product_id \
         JOIN price_versions pv ON pv.item_id=i.item_id WHERE i.status='ACTIVE' \
         AND p.status='ACTIVE' AND p.usage_model='CREDIT_METERED' \
         AND pv.state IN ('SCHEDULED','ACTIVE') \
         ORDER BY i.item_id,pv.effective_from DESC,pv.price_version_id",
    )
    .fetch_all(&mut **transaction)
    .await?;
    let scope_items: Vec<(Uuid, Uuid)> = rows
        .iter()
        .map(|row| (row.get("item_id"), row.get("price_version_id")))
        .collect();
    let fingerprint = scope_fingerprint(&scope_items);
    let scope_id = insert_or_find_scope(transaction, &fingerprint).await?;
    insert_scope_items(transaction, scope_id, &scope_items).await?;
    select_current_scope(transaction, scope_id).await?;
    Ok(scope_id)
}

fn scope_fingerprint(scope_items: &[(Uuid, Uuid)]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for (item_id, price_version_id) in scope_items {
        digest.update(item_id.as_bytes());
        digest.update(price_version_id.as_bytes());
    }
    format!("{:#x}", digest.finalize())
}

async fn insert_or_find_scope(
    transaction: &mut Transaction<'_, Postgres>,
    fingerprint: &str,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO catalog_scope_versions (scope_version,fingerprint) VALUES ($1,$2) \
         ON CONFLICT (fingerprint) DO UPDATE SET fingerprint=EXCLUDED.fingerprint \
         RETURNING scope_version",
    )
    .bind(Uuid::new_v4())
    .bind(fingerprint)
    .fetch_one(&mut **transaction)
    .await
}

async fn insert_scope_items(
    transaction: &mut Transaction<'_, Postgres>,
    scope_id: Uuid,
    scope_items: &[(Uuid, Uuid)],
) -> Result<(), sqlx::Error> {
    for (item_id, price_version_id) in scope_items {
        sqlx::query(
            "INSERT INTO catalog_scope_items (scope_version,item_id,price_version_id) VALUES ($1,$2,$3) \
             ON CONFLICT DO NOTHING",
        )
        .bind(scope_id)
        .bind(item_id)
        .bind(price_version_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn select_current_scope(
    transaction: &mut Transaction<'_, Postgres>,
    scope_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO catalog_scope_current (singleton,scope_version) VALUES (true,$1) \
         ON CONFLICT (singleton) DO UPDATE SET scope_version=EXCLUDED.scope_version,selected_at=now()",
    )
    .bind(scope_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn missing(kind: &str, id: Uuid) -> ApiError {
    ApiError::not_found(
        "catalog_resource_not_found",
        format!("{kind} {id} does not exist"),
    )
}

fn version_conflict(kind: &str, id: Uuid, expected: i64) -> ApiError {
    ApiError::conflict(
        "catalog_version_conflict",
        format!("{kind} {id} must exist at version {expected}"),
    )
}
