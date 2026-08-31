//! Review queue, checklist completion, and the publish / reject decisions.

use axum::extract::{Path, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::IdKind;
use serde_json::json;

use crate::marketplace::auth::AuthCtx;
use crate::marketplace::principal::authorize;
use crate::marketplace::review as review_svc;
use crate::marketplace::types::{
    ChecklistUpdateRequest, ListingDto, RejectRequest, ReviewDto, ReviewQueueResponse,
    LISTING_PUBLISHED, LISTING_REJECTED,
};
use crate::marketplace::{emit_event, internal, listings, parse_public_id, set_org};
use crate::AppState;

/// Listings awaiting a decision (`submitted` or `in_review`).
pub async fn queue(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ReviewQueueResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_review()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let listings_in_queue: Vec<listings::ListingRow> =
        listings::list_owned(&mut tx, auth.ctx.org_id, request_id)
            .await?
            .into_iter()
            .filter(|l| matches!(l.status.as_str(), "submitted" | "in_review"))
            .collect();

    let mut items = Vec::with_capacity(listings_in_queue.len());
    for listing in &listings_in_queue {
        let review =
            review_svc::ensure_review(&mut tx, auth.ctx.org_id, listing.id, request_id).await?;
        items.push(review.to_dto(listing));
    }
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(ReviewQueueResponse { items }))
}

pub async fn update_checklist(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(listing_public_id): Path<String>,
    Json(req): Json<ChecklistUpdateRequest>,
) -> Result<Json<ReviewDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_review()).await?;
    let listing_id = parse_public_id(IdKind::MarketplaceApp, &listing_public_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let listing = listings::fetch_owned(&mut tx, auth.ctx.org_id, listing_id, request_id).await?;
    let review = review_svc::complete_items(
        &mut tx,
        auth.ctx.org_id,
        listing_id,
        &req.completed_item_ids,
        req.reviewer_notes.as_deref(),
        auth.ctx.actor.on_behalf_of,
        request_id,
    )
    .await?;
    // A submitted listing picked up by a reviewer moves to in_review; already
    // approved or published listings keep their status.
    let listing = if listing.status == "submitted" {
        listings::set_status(
            &mut tx,
            auth.ctx.org_id,
            listing_id,
            "in_review",
            request_id,
        )
        .await?
    } else {
        listing
    };
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(review.to_dto(&listing)))
}

/// Publish. Fails with 403 until every required item **and** the security
/// review are complete.
pub async fn publish(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(listing_public_id): Path<String>,
) -> Result<Json<ListingDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_review()).await?;
    let listing_id = parse_public_id(IdKind::MarketplaceApp, &listing_public_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    listings::fetch_owned(&mut tx, auth.ctx.org_id, listing_id, request_id).await?;
    review_svc::assert_publishable(&mut tx, auth.ctx.org_id, listing_id, request_id).await?;

    let listing = listings::set_status(
        &mut tx,
        auth.ctx.org_id,
        listing_id,
        LISTING_PUBLISHED,
        request_id,
    )
    .await?;
    review_svc::set_status(
        &mut tx,
        auth.ctx.org_id,
        listing_id,
        review_svc::REVIEW_PUBLISHED,
        None,
        request_id,
    )
    .await?;

    emit_event(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.clone(),
        "listing_published",
        json!({
            "listing_id": listing.public_id,
            "slug": listing.slug,
            "connector_key": listing.connector_key,
        }),
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(listing.to_dto()))
}

pub async fn reject(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(listing_public_id): Path<String>,
    Json(body): Json<RejectRequest>,
) -> Result<Json<ListingDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_review()).await?;
    let listing_id = parse_public_id(IdKind::MarketplaceApp, &listing_public_id, request_id)?;
    let reason = body.reason;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    listings::fetch_owned(&mut tx, auth.ctx.org_id, listing_id, request_id).await?;
    let listing = listings::set_status(
        &mut tx,
        auth.ctx.org_id,
        listing_id,
        LISTING_REJECTED,
        request_id,
    )
    .await?;
    review_svc::set_status(
        &mut tx,
        auth.ctx.org_id,
        listing_id,
        review_svc::REVIEW_REJECTED,
        reason.as_deref(),
        request_id,
    )
    .await?;

    emit_event(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.clone(),
        "listing_rejected",
        json!({ "listing_id": listing.public_id, "reason": reason }),
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(listing.to_dto()))
}
