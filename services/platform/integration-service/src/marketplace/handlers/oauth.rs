//! OAuth routes: consent → code (session auth), code/refresh → tokens (client
//! credentials), and the bearer-token permission check (no session).

use axum::extract::State;
use axum::Json;
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::IdKind;

use crate::marketplace::auth::AuthCtx;
use crate::marketplace::oauth as oauth_svc;
use crate::marketplace::principal::authorize as enforce_permission;
use crate::marketplace::types::{
    AuthorizePermissionRequest, AuthorizePermissionResponse, AuthorizeRequest, AuthorizeResponse,
    OauthTokenRequest, OauthTokenResponse,
};
use crate::marketplace::{install, internal, listings, parse_public_id, set_org, validation};
use crate::AppState;

/// Grant consent and mint a single-use authorization code (PKCE).
pub async fn authorize(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<AuthorizeRequest>,
) -> Result<Json<AuthorizeResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal =
        enforce_permission(&state, &auth, perms::admin_marketplace_install()).await?;
    let listing_id = parse_public_id(IdKind::MarketplaceApp, &req.listing_id, request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let listing = listings::fetch_published(&mut tx, listing_id, request_id).await?;
    let consented =
        install::validate_consent(&listing, &req.consented_scopes, &principal, request_id)?;
    let method = req.code_challenge_method.as_deref().unwrap_or("S256");

    let issued = oauth_svc::create_authorization_code(
        &mut tx,
        auth.ctx.org_id,
        &oauth_svc::CodeGrant {
            listing: &listing,
            consented: &consented,
            redirect_uri: &req.redirect_uri,
            code_challenge: &req.code_challenge,
            code_challenge_method: method,
            created_by: auth.ctx.actor.on_behalf_of,
        },
        request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(Json(AuthorizeResponse {
        code: issued.code,
        redirect_uri: req.redirect_uri,
        expires_at: issued.expires_at,
        state: req.state,
    }))
}

/// Client-credentials token endpoint: `authorization_code` or `refresh_token`.
pub async fn token(
    State(state): State<AppState>,
    Json(req): Json<OauthTokenRequest>,
) -> Result<Json<OauthTokenResponse>, AppError> {
    let request_id = "oauth-token";
    let (install_row, tokens) = match req.grant_type.as_str() {
        oauth_svc::GRANT_AUTHORIZATION_CODE => {
            oauth_svc::exchange_authorization_code(&state.pool, &req, request_id).await?
        }
        oauth_svc::GRANT_REFRESH_TOKEN => {
            oauth_svc::exchange_refresh_token(&state.pool, &req, request_id).await?
        }
        other => {
            return Err(validation(
                request_id,
                format!("unsupported grant_type {other}"),
            ))
        }
    };

    Ok(Json(OauthTokenResponse {
        install_id: install_row.public_id,
        tokens,
    }))
}

/// Resource-server check: does this app token carry `permission`?
pub async fn authorize_permission(
    State(state): State<AppState>,
    Json(req): Json<AuthorizePermissionRequest>,
) -> Result<Json<AuthorizePermissionResponse>, AppError> {
    let request_id = "oauth-authorize-permission";
    let decision =
        oauth_svc::authorize_permission(&state.pool, &req.access_token, &req.permission, request_id)
            .await?;
    Ok(Json(decision))
}
