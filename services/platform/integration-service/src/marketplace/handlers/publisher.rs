//! Publisher routes: draft a listing, then submit it for review.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::IdKind;
use serde_json::json;

use crate::marketplace::auth::AuthCtx;
use crate::marketplace::principal::authorize;
use crate::marketplace::types::{
    CreateListingRequest, CreateListingResponse, ListingDto, ListingListResponse,
};
use crate::marketplace::{emit_event, internal, listings, parse_public_id, review, set_org};
use crate::AppState;

/// Create a draft listing and its OAuth client. `client_secret` is shown once.
pub async fn create_listing(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<CreateListingRequest>,
) -> Result<(StatusCode, Json<CreateListingResponse>), AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_write()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let created = listings::create_listing(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        &req,
        state.allow_private_urls,
        request_id,
    )
    .await?;

    emit_event(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.clone(),
        "listing_created",
        json!({
            "listing_id": created.listing.public_id,
            "slug": created.listing.slug,
            "listing_kind": created.listing.listing_kind,
            "connector_key": created.listing.connector_key,
            "requested_scopes": created.listing.requested_scopes(),
        }),
        request_id,
    )
    .await?;
    emit_event(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.clone(),
        "oauth_client_created",
        json!({
            "listing_id": created.listing.public_id,
            "oauth_client_id": created.client_public_id,
        }),
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateListingResponse {
            listing: created.listing.to_dto(),
            oauth_client_id: created.client_id,
            oauth_client_public_id: created.client_public_id,
            client_secret: created.client_secret,
        }),
    ))
}

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ListingListResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_write()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let rows = listings::list_owned(&mut tx, auth.ctx.org_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(ListingListResponse {
        items: rows.iter().map(listings::ListingRow::to_dto).collect(),
    }))
}

/// Submit for review; also materialises the default review checklist.
pub async fn submit_listing(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(public_id): Path<String>,
) -> Result<Json<ListingDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_write()).await?;
    let listing_id = parse_public_id(IdKind::MarketplaceApp, &public_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let listing = listings::submit_listing(&mut tx, auth.ctx.org_id, listing_id, request_id).await?;
    review::ensure_review(&mut tx, auth.ctx.org_id, listing_id, request_id).await?;

    emit_event(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.clone(),
        "listing_submitted",
        json!({ "listing_id": listing.public_id, "slug": listing.slug }),
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(listing.to_dto()))
}
