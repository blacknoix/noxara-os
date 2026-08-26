//! `/api/v1/finance/events/sales/apply` — in-process CRM event apply (tests).

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::EventEnvelope;
use companyos_tenancy::set_session_org_id;

use super::{internal, validation};
use crate::auth::AuthCtx;
use crate::journal::ensure_ledger_accounts;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::projection::apply_sales_event;
use crate::state::AppState;
use crate::types::{ApplySalesEventRequest, ApplySalesEventResponse};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/finance/events/sales/apply",
        post(apply_sales_event_handler),
    )
}

/// POST /api/v1/finance/events/sales/apply
#[utoipa::path(post, path = "/api/v1/finance/events/sales/apply", tag = "finance-events",
    request_body = ApplySalesEventRequest,
    responses((status = 200, body = ApplySalesEventResponse)))]
pub async fn apply_sales_event_handler(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ApplySalesEventRequest>,
) -> Result<Json<ApplySalesEventResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    // Projection apply is an internal/test path; require invoice create as a coarse gate.
    enforce_any_scope(
        &membership.principal,
        perms::finance_invoice_create(),
        &request_id,
    )?;

    let envelope: EventEnvelope = serde_json::from_value(body.envelope)
        .map_err(|e| validation(&request_id, format!("invalid event envelope: {e}")))?;

    if envelope.org_id.as_uuid() != auth.ctx.org_id.as_uuid() {
        return Err(validation(
            &request_id,
            "envelope org_id does not match authenticated org",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, auth.ctx.org_id.as_uuid())
        .await
        .map_err(internal(&request_id))?;

    let applied = apply_sales_event(&mut tx, &envelope)
        .await
        .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(ApplySalesEventResponse { applied }))
}
