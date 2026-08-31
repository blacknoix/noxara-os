//! HTTP surface for the marketplace.
//!
//! Every route is session-authenticated (`admin.marketplace.*` permissions),
//! except `POST /api/v1/marketplace/oauth/token` and
//! `POST /api/v1/marketplace/oauth/authorize-permission`, which authenticate
//! with client credentials and bearer app tokens respectively. Marketplace
//! routes are deliberately absent from the public API-key allowlist.

pub mod catalogue;
pub mod installs;
pub mod oauth;
pub mod publisher;
pub mod review;

use axum::routing::{get, post};
use axum::Router;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/marketplace/catalogue",
            get(catalogue::list_catalogue),
        )
        .route(
            "/api/v1/marketplace/listings",
            post(publisher::create_listing),
        )
        .route(
            "/api/v1/marketplace/listings/mine",
            get(publisher::list_mine),
        )
        .route(
            "/api/v1/marketplace/listings/{public_id}",
            get(catalogue::get_listing),
        )
        .route(
            "/api/v1/marketplace/listings/{public_id}/submit",
            post(publisher::submit_listing),
        )
        .route("/api/v1/marketplace/reviews/queue", get(review::queue))
        .route(
            "/api/v1/marketplace/reviews/{listing_id}/checklist",
            post(review::update_checklist),
        )
        .route(
            "/api/v1/marketplace/reviews/{listing_id}/publish",
            post(review::publish),
        )
        .route(
            "/api/v1/marketplace/reviews/{listing_id}/reject",
            post(review::reject),
        )
        .route(
            "/api/v1/marketplace/installs",
            get(installs::list_installs).post(installs::create_install),
        )
        .route(
            "/api/v1/marketplace/installs/{public_id}",
            get(installs::get_install),
        )
        .route(
            "/api/v1/marketplace/installs/{public_id}/uninstall",
            post(installs::uninstall),
        )
        .route(
            "/api/v1/marketplace/installs/{public_id}/reconsent",
            post(installs::reconsent),
        )
        .route("/api/v1/integrations", get(installs::list_integrations))
        .route(
            "/api/v1/integrations/{connector_key}/connect",
            post(installs::connect),
        )
        .route(
            "/api/v1/integrations/{connector_key}/disconnect",
            post(installs::disconnect),
        )
        .route(
            "/api/v1/marketplace/oauth/authorize",
            post(oauth::authorize),
        )
        .route("/api/v1/marketplace/oauth/token", post(oauth::token))
        .route(
            "/api/v1/marketplace/oauth/authorize-permission",
            post(oauth::authorize_permission),
        )
}
