use sqlx::Row;

use crate::{
    dto::catalog_admin::{CatalogEntryResponse, CatalogPageQuery, CatalogPageResponse},
    error::ApiResult,
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    /// Page catalog identities by UUID; e.g. `repo.list_catalog_entries("products", query, 20).await`.
    pub async fn list_catalog_entries(
        &self,
        kind: &str,
        query: CatalogPageQuery,
        limit: i64,
    ) -> ApiResult<CatalogPageResponse> {
        let rows = sqlx::query(CATALOG_ENTRIES)
            .bind(kind)
            .bind(query.cursor)
            .bind(query.parent_id)
            .bind(limit + 1)
            .fetch_all(&self.pool())
            .await?;
        let has_more = rows.len() as i64 > limit;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(|row| CatalogEntryResponse {
                id: row.get("id"),
                kind: row.get("kind"),
                parent_id: row.get("parent_id"),
                name: row.get("name"),
                status: row.get("status"),
                created_at: row.get("created_at"),
                published_at: row.get("published_at"),
            })
            .collect::<Vec<_>>();
        let next_cursor = has_more.then(|| items.last().map(|item| item.id)).flatten();
        Ok(CatalogPageResponse { items, next_cursor })
    }
}

const CATALOG_ENTRIES: &str = "WITH entries AS (
  SELECT product_id id, 'products'::text kind, NULL::uuid parent_id, name, status, created_at, published_at FROM products
  UNION ALL SELECT item_id, 'items', product_id, name, status, created_at, NULL::timestamptz FROM items
  UNION ALL SELECT price_version_id, 'prices', item_id, pricing_model, state, created_at, published_at FROM price_versions
  UNION ALL SELECT subscription_id, 'subscriptions', NULL::uuid, name, subscription_model, created_at, created_at FROM subscriptions
  UNION ALL SELECT plan_version_id, 'plans', subscription_id, name, CASE WHEN revoked_at IS NULL THEN 'PUBLISHED' ELSE 'REVOKED' END, created_at, published_at FROM subscription_plan_versions
  UNION ALL SELECT on_demand_plan_id, 'on-demand', subscription_id, name, CASE WHEN revoked_at IS NULL THEN 'PUBLISHED' ELSE 'REVOKED' END, created_at, published_at FROM on_demand_plans
  UNION ALL SELECT policy_version_id, 'policies', policy_id, policy_id::text, 'PUBLISHED', created_at, created_at FROM subscription_admission_policies
) SELECT * FROM entries WHERE kind=$1 AND ($2::uuid IS NULL OR id>$2) AND ($3::uuid IS NULL OR parent_id=$3) ORDER BY id LIMIT $4";
