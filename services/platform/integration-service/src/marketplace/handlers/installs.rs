//! Install routes plus the `/api/v1/integrations/...` alias.
//!
//! The alias exists purely so the Integrations UI can address connectors by
//! `connector_key`. `connect` resolves the key to a published listing and then
//! calls the same [`install::create_install`]; `disconnect` calls the same
//! [`install::revoke_install`]. No branch on `listing_kind` occurs here.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::IdKind;

use crate::marketplace::auth::AuthCtx;
use crate::marketplace::principal::authorize;
use crate::marketplace::types::{
    ConnectRequest, CreateInstallRequest, InstallCreatedResponse, InstallDto, InstallListResponse,
    IntegrationListResponse, ReconsentRequest,
};
use crate::marketplace::{install, internal, listings, parse_public_id, set_org, validation};
use crate::AppState;

pub async fn list_installs(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<InstallListResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_read()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let rows = install::list(&mut tx, auth.ctx.org_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(InstallListResponse {
        items: rows.iter().map(install::InstallRow::to_dto).collect(),
    }))
}

pub async fn get_install(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(public_id): Path<String>,
) -> Result<Json<InstallDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_read()).await?;
    let install_id = parse_public_id(IdKind::MarketplaceInstall, &public_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row = install::fetch(&mut tx, auth.ctx.org_id, install_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(row.to_dto()))
}

/// Install a published listing with an explicit consented scope set.
pub async fn create_install(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<CreateInstallRequest>,
) -> Result<(StatusCode, Json<InstallCreatedResponse>), AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal = authorize(&state, &auth, perms::admin_marketplace_install()).await?;
    let listing_id = parse_public_id(IdKind::MarketplaceApp, &req.listing_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let listing = listings::fetch_published(&mut tx, listing_id, request_id).await?;
    let consented =
        install::validate_consent(&listing, &req.consented_scopes, &principal, request_id)?;
    let (row, tokens) = install::create_install(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.clone(),
        &listing,
        &consented,
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok((
        StatusCode::CREATED,
        Json(InstallCreatedResponse {
            install: row.to_dto(),
            tokens,
        }),
    ))
}

pub async fn uninstall(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(public_id): Path<String>,
) -> Result<Json<InstallDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_uninstall()).await?;
    let install_id = parse_public_id(IdKind::MarketplaceInstall, &public_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row = install::revoke_install(
        &mut tx,
        auth.ctx.org_id,
        install_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.clone(),
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(row.to_dto()))
}

/// Widen or narrow consent. Old tokens are revoked and a new pair is issued.
pub async fn reconsent(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(public_id): Path<String>,
    Json(req): Json<ReconsentRequest>,
) -> Result<Json<InstallCreatedResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal = authorize(&state, &auth, perms::admin_marketplace_install()).await?;
    let install_id = parse_public_id(IdKind::MarketplaceInstall, &public_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let existing = install::fetch(&mut tx, auth.ctx.org_id, install_id, request_id).await?;
    let listing = listings::fetch_by_uuid(&mut tx, existing.listing_id, request_id).await?;
    let consented =
        install::validate_consent(&listing, &req.consented_scopes, &principal, request_id)?;
    let (row, tokens) = install::reconsent(
        &mut tx,
        auth.ctx.org_id,
        install_id,
        &consented,
        auth.ctx.actor.clone(),
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(InstallCreatedResponse {
        install: row.to_dto(),
        tokens,
    }))
}

// ---------------------------------------------------------------------------
// Integrations UI alias — same tables, same functions, filtered by connector.
// ---------------------------------------------------------------------------

pub async fn list_integrations(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<IntegrationListResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_read()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let rows = install::list(&mut tx, auth.ctx.org_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(IntegrationListResponse {
        items: rows
            .iter()
            .filter_map(install::InstallRow::to_integration_dto)
            .collect(),
    }))
}

pub async fn connect(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(connector_key): Path<String>,
    Json(req): Json<ConnectRequest>,
) -> Result<(StatusCode, Json<InstallCreatedResponse>), AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal = authorize(&state, &auth, perms::admin_marketplace_install()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let listing =
        listings::fetch_published_by_connector(&mut tx, &connector_key, request_id).await?;

    let requested = match req.consented_scopes {
        Some(scopes) => scopes,
        None => install::default_consent(&listing, &principal),
    };
    if requested.is_empty() {
        return Err(validation(
            request_id,
            format!("no scopes of {connector_key} can be granted by this member"),
        ));
    }
    let consented = install::validate_consent(&listing, &requested, &principal, request_id)?;

    let (row, tokens) = install::create_install(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.clone(),
        &listing,
        &consented,
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok((
        StatusCode::CREATED,
        Json(InstallCreatedResponse {
            install: row.to_dto(),
            tokens,
        }),
    ))
}

pub async fn disconnect(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(connector_key): Path<String>,
) -> Result<Json<InstallDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::admin_marketplace_uninstall()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let existing =
        install::fetch_active_by_connector(&mut tx, auth.ctx.org_id, &connector_key, request_id)
            .await?;
    let row = install::revoke_install(
        &mut tx,
        auth.ctx.org_id,
        existing.id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.clone(),
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(row.to_dto()))
}
