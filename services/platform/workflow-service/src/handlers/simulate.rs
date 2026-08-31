//! Simulation / dry-run — zero side effects.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;

use crate::auth::AuthCtx;
use crate::engine::{load_or_default_bounds, DEFAULT_MAX_STEPS};
use crate::principal::{enforce, load_principal};
use crate::simulate::{self, SimulateResult};
use crate::state::AppState;
use crate::types::SimulateRequest;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/workflows/simulate", post(simulate_handler))
}

#[utoipa::path(
    post,
    path = "/api/v1/workflows/simulate",
    tag = "workflows-simulate",
    request_body = SimulateRequest,
    responses((status = 200, body = SimulateResult))
)]
pub async fn simulate_handler(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<SimulateRequest>,
) -> Result<Json<SimulateResult>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;

    body.graph
        .validate()
        .map_err(|e| crate::handlers::validation(rid, e))?;

    // Also ensure creator (caller) could own these actions — same gate as save.
    crate::permissions::enforce_creator_can_own_graph(&principal, &body.graph, rid)?;

    let max_steps = if let Some(m) = body.max_steps {
        m
    } else {
        let mut tx = state
            .pool
            .begin()
            .await
            .map_err(crate::handlers::internal(rid))?;
        crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
        let bounds = load_or_default_bounds(&mut tx, auth.ctx.org_id.as_uuid())
            .await
            .map_err(crate::handlers::internal(rid))?;
        tx.commit().await.map_err(crate::handlers::internal(rid))?;
        bounds.max_steps_per_instance
    };

    let result = simulate::simulate(
        &body.graph,
        &principal,
        &body.payload,
        max_steps.clamp(1, DEFAULT_MAX_STEPS * 2),
    );
    debug_assert!(!result.side_effects);
    Ok(Json(result))
}
