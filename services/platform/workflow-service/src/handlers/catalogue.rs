//! Trigger/action catalogues + fixture workflows.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;

use crate::auth::AuthCtx;
use crate::catalogue::{fixture_graphs, ACTION_CATALOGUE, TRIGGER_CATALOGUE};
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{
    ActionCatalogueResponse, FixtureListResponse, FixtureWorkflowDto, TriggerCatalogueResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/workflows/catalogue/triggers", get(list_triggers))
        .route("/api/v1/workflows/catalogue/actions", get(list_actions))
        .route("/api/v1/workflows/fixtures", get(list_fixtures))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/catalogue/triggers",
    tag = "workflows-catalogue",
    responses((status = 200, body = TriggerCatalogueResponse))
)]
pub async fn list_triggers(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<TriggerCatalogueResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;
    Ok(Json(TriggerCatalogueResponse {
        items: TRIGGER_CATALOGUE.to_vec(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/catalogue/actions",
    tag = "workflows-catalogue",
    responses((status = 200, body = ActionCatalogueResponse))
)]
pub async fn list_actions(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ActionCatalogueResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;
    Ok(Json(ActionCatalogueResponse {
        items: ACTION_CATALOGUE.to_vec(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/fixtures",
    tag = "workflows-catalogue",
    responses((status = 200, body = FixtureListResponse))
)]
pub async fn list_fixtures(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<FixtureListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;
    let items = fixture_graphs()
        .into_iter()
        .map(|(name, description, graph)| FixtureWorkflowDto {
            name: name.into(),
            description: description.into(),
            graph,
        })
        .collect();
    Ok(Json(FixtureListResponse { items }))
}
