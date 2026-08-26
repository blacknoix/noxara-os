//! `/api/v1/operations/events/sales/apply` — project projection from Sales events.
//!
//! On `deal.won`: create a project from the deal payload when none exists for
//! that `deal_id` (idempotent by deal UUID). Never reads `sales_*` tables —
//! customer/deal links are opaque UUID + public_id text only.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::Deserialize;

use super::{internal, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{ApplySalesEventRequest, ApplySalesEventResponse};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/operations/events/sales/apply",
        post(apply_sales_event),
    )
}

#[derive(Debug, Deserialize)]
struct DealWonPayload {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    customer_id: Option<String>,
    #[serde(default)]
    customer: Option<String>,
}

/// POST /api/v1/operations/events/sales/apply
#[utoipa::path(post, path = "/api/v1/operations/events/sales/apply", tag = "operations-events",
    request_body = ApplySalesEventRequest,
    responses((status = 200, body = ApplySalesEventResponse)))]
pub async fn apply_sales_event(
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
    // Projection apply gate: require project create.
    enforce_any_scope(
        &membership.principal,
        perms::operations_project_create(),
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

    if envelope.context.as_str() != "sales"
        || envelope.aggregate != "deal"
        || envelope.event_type != "won"
    {
        return Ok(Json(ApplySalesEventResponse {
            applied: false,
            project_id: None,
        }));
    }

    let payload: DealWonPayload = serde_json::from_value(envelope.payload.clone())
        .map_err(|e| validation(&request_id, format!("invalid deal.won payload: {e}")))?;

    let deal_uuid = parse_public_id(IdKind::Deal, &payload.id, &request_id)?;
    let deal_public_id = payload.id.clone();

    let customer_public = payload
        .customer_id
        .as_deref()
        .or(payload.customer.as_deref());
    let (customer_id, customer_public_id) = match customer_public {
        Some(raw) if !raw.trim().is_empty() => {
            let u = parse_public_id(IdKind::Customer, raw, &request_id)?;
            (Some(u), Some(raw.to_string()))
        }
        _ => (None, None),
    };

    let project_name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Project from {deal_public_id}"));

    let org_id = auth.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // Idempotent by deal_id — never create a second project for the same deal.
    let existing: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT public_id FROM operations_project
        WHERE org_id = $1 AND deal_id = $2 AND deleted_at IS NULL
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(deal_uuid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Some((public_id,)) = existing {
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(ApplySalesEventResponse {
            applied: true,
            project_id: Some(public_id),
        }));
    }

    let public_id = PublicId::generate(IdKind::Project);
    let id = public_id.uuid();

    sqlx::query(
        r#"
        INSERT INTO operations_project (
            id, org_id, public_id, name, description, status, owner_user_id,
            customer_id, deal_id, customer_public_id, deal_public_id
        ) VALUES ($1,$2,$3,$4,NULL,'active',$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&project_name)
    .bind(auth.ctx.actor.user_id)
    .bind(customer_id)
    .bind(deal_uuid)
    .bind(&customer_public_id)
    .bind(&deal_public_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let created = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "project",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "name": project_name,
            "deal_id": deal_public_id,
            "customer_id": customer_public_id,
            "source": "sales.deal.won",
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &created)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.project.create",
        "project",
        &public_id.as_str(),
        serde_json::json!({
            "source": "sales.deal.won",
            "deal_id": deal_public_id,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ApplySalesEventResponse {
        applied: true,
        project_id: Some(public_id.as_str()),
    }))
}
