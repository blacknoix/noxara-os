#![allow(clippy::type_complexity)]
//! Phase 4.2 — Enterprise multi-tenancy (CMEK, SCIM, grants, network, SLA, eDiscovery).

pub mod cmk;
pub mod ediscovery;
pub mod grants;
pub mod network;
pub mod scim;
pub mod sla;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(cmk::router())
        .merge(scim::admin_router())
        .merge(scim::scim_router())
        .merge(grants::router())
        .merge(network::router())
        .merge(sla::router())
        .merge(ediscovery::router())
}
