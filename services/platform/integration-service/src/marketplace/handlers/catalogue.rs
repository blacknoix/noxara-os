//! Read-only catalogue: published listings visible to every organization.

use axum::extract::{Path, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::IdKind;

use crate::marketplace::auth::AuthCtx;
use crate::marketplace::listings;
use crate::marketplace::principal::authorize;
use crate::marketplace::types::{ListingDto, ListingListResponse};
use crate::marketplace::{internal, parse_public_id, set_org};
use crate::AppState;

pub async fn list_catalogue(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ListingListResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_read()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let rows = listings::list_published(&mut tx, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(ListingListResponse {
        items: rows.iter().map(listings::ListingRow::to_dto).collect(),
    }))
}

/// Listing detail. Publishers see their own drafts; everyone sees published.
pub async fn get_listing(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(public_id): Path<String>,
) -> Result<Json<ListingDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_read()).await?;
    let listing_id = parse_public_id(IdKind::MarketplaceApp, &public_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row = listings::fetch_by_uuid(&mut tx, listing_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(row.to_dto()))
}
