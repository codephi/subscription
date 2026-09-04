use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::catalog::{
        CatalogScopeResponse, CreateItemRequest, CreatePriceVersionRequest, CreateProductRequest,
        ItemResponse, PriceVersionResponse, ProductResponse, UpdateItemRequest,
        UpdateProductRequest,
    },
    error::{ApiResult, ErrorResponse},
    services::catalog,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_product))
        .routes(routes!(get_product, update_product))
        .routes(routes!(create_item))
        .routes(routes!(get_item, update_item))
        .routes(routes!(create_price_version))
        .routes(routes!(get_price_version))
        .routes(routes!(publish_price_version))
        .routes(routes!(get_current_catalog_scope))
}

#[utoipa::path(post, path = "/v1/products", tag = "Catalog", request_body = CreateProductRequest,
    responses((status = 201, body = ProductResponse)))]
async fn create_product(
    State(state): State<AppState>,
    Json(request): Json<CreateProductRequest>,
) -> ApiResult<(StatusCode, Json<ProductResponse>)> {
    let response = catalog::create_product(&state.database(), request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(get, path = "/v1/products/{product_id}", tag = "Catalog",
    params(("product_id" = Uuid, Path)), responses((status = 200, body = ProductResponse)))]
async fn get_product(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
) -> ApiResult<Json<ProductResponse>> {
    Ok(Json(
        catalog::get_product(&state.database(), product_id).await?,
    ))
}

#[utoipa::path(patch, path = "/v1/products/{product_id}", tag = "Catalog",
    params(("product_id" = Uuid, Path)), request_body = UpdateProductRequest,
    responses((status = 200, body = ProductResponse)))]
async fn update_product(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    Json(request): Json<UpdateProductRequest>,
) -> ApiResult<Json<ProductResponse>> {
    Ok(Json(
        catalog::update_product(&state.database(), product_id, request).await?,
    ))
}

#[utoipa::path(post, path = "/v1/products/{product_id}/items", tag = "Catalog",
    params(("product_id" = Uuid, Path)), request_body = CreateItemRequest,
    responses((status = 201, body = ItemResponse)))]
async fn create_item(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    Json(request): Json<CreateItemRequest>,
) -> ApiResult<(StatusCode, Json<ItemResponse>)> {
    let response = catalog::create_item(&state.database(), product_id, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(get, path = "/v1/items/{item_id}", tag = "Catalog", params(("item_id" = Uuid, Path)),
    responses((status = 200, body = ItemResponse)))]
async fn get_item(
    State(state): State<AppState>,
    Path(item_id): Path<Uuid>,
) -> ApiResult<Json<ItemResponse>> {
    Ok(Json(catalog::get_item(&state.database(), item_id).await?))
}

#[utoipa::path(patch, path = "/v1/items/{item_id}", tag = "Catalog", params(("item_id" = Uuid, Path)),
    request_body = UpdateItemRequest, responses((status = 200, body = ItemResponse)))]
async fn update_item(
    State(state): State<AppState>,
    Path(item_id): Path<Uuid>,
    Json(request): Json<UpdateItemRequest>,
) -> ApiResult<Json<ItemResponse>> {
    Ok(Json(
        catalog::update_item(&state.database(), item_id, request).await?,
    ))
}

#[utoipa::path(post, path = "/v1/items/{item_id}/price-versions", tag = "Catalog",
    params(("item_id" = Uuid, Path)), request_body = CreatePriceVersionRequest,
    responses(
        (status = 201, body = PriceVersionResponse),
        (status = 422, body = ErrorResponse, description = "Invalid conversion, tiers, cycle, or overflow")
    ))]
async fn create_price_version(
    State(state): State<AppState>,
    Path(item_id): Path<Uuid>,
    Json(request): Json<CreatePriceVersionRequest>,
) -> ApiResult<(StatusCode, Json<PriceVersionResponse>)> {
    let response = catalog::create_price_version(&state.database(), item_id, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(get, path = "/v1/price-versions/{price_id}", tag = "Catalog",
    params(("price_id" = Uuid, Path)), responses((status = 200, body = PriceVersionResponse)))]
async fn get_price_version(
    State(state): State<AppState>,
    Path(price_id): Path<Uuid>,
) -> ApiResult<Json<PriceVersionResponse>> {
    Ok(Json(
        catalog::get_price_version(&state.database(), price_id).await?,
    ))
}

#[utoipa::path(post, path = "/v1/price-versions/{price_id}/publish", tag = "Catalog",
    params(("price_id" = Uuid, Path)), responses(
        (status = 200, body = PriceVersionResponse),
        (status = 409, body = ErrorResponse, description = "Published or overlapping version")
    ))]
async fn publish_price_version(
    State(state): State<AppState>,
    Path(price_id): Path<Uuid>,
) -> ApiResult<Json<PriceVersionResponse>> {
    Ok(Json(
        catalog::publish_price_version(&state.database(), price_id).await?,
    ))
}

#[utoipa::path(get, path = "/v1/catalog-scope/current", tag = "Catalog",
    responses(
        (status = 200, body = CatalogScopeResponse),
        (status = 404, body = ErrorResponse, description = "No catalog scope has been published")
    ))]
async fn get_current_catalog_scope(
    State(state): State<AppState>,
) -> ApiResult<Json<CatalogScopeResponse>> {
    Ok(Json(
        catalog::get_current_catalog_scope(&state.database()).await?,
    ))
}
