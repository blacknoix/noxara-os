//! Monitoring view: running, waiting, failed, SLA breaches + org bounds.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{IdKind, PublicId};
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::engine::{load_or_default_bounds, DEFAULT_MAX_CONCURRENT, DEFAULT_MAX_STEPS};
use crate::handlers::{internal, user_public, validation};
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{
    MonitorResponse, MonitorSummaryDto, OrgBoundsDto, UpdateOrgBoundsRequest, WorkflowInstanceDto,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/workflows/monitor", get(monitor))
        .route(
            "/api/v1/workflows/bounds",
            get(get_bounds).put(update_bounds),
        )
}

#[allow(clippy::type_complexity)]
type InstRow = (
    Uuid,
    Uuid,
    Uuid,
    i32,
    String,
    Uuid,
    String,
    i32,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    String,
);

#[utoipa::path(
    get,
    path = "/api/v1/workflows/monitor",
    tag = "workflows-monitor",
    responses((status = 200, body = MonitorResponse))
)]
pub async fn monitor(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<MonitorResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;

    sqlx::query(
        r#"
        UPDATE workflow_instance
        SET status = 'sla_breached', updated_at = now()
        WHERE org_id = $1
          AND status IN ('running', 'waiting')
          AND sla_deadline IS NOT NULL
          AND sla_deadline < now()
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let summary: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          COUNT(*) FILTER (WHERE status = 'running'),
          COUNT(*) FILTER (WHERE status = 'waiting'),
          COUNT(*) FILTER (WHERE status = 'failed'),
          COUNT(*) FILTER (WHERE status = 'completed'),
          COUNT(*) FILTER (WHERE status = 'cancelled'),
          COUNT(*) FILTER (WHERE status = 'sla_breached')
        FROM workflow_instance WHERE org_id = $1
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let rows: Vec<InstRow> = sqlx::query_as(
        r#"
        SELECT i.id, i.definition_id, i.version_id, i.version_number, i.status, i.actor_user_id,
               i.temporal_workflow_id, i.step_count, i.current_node_id, i.error_message,
               i.waiting_until, i.sla_deadline, i.started_at, i.updated_at, i.completed_at,
               d.public_id
        FROM workflow_instance i
        JOIN workflow_definition d ON d.id = i.definition_id AND d.org_id = i.org_id
        WHERE i.org_id = $1 AND i.status IN ('running', 'waiting', 'failed', 'sla_breached')
        ORDER BY i.updated_at DESC
        LIMIT 100
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let instances = rows
        .into_iter()
        .map(|r| WorkflowInstanceDto {
            id: PublicId::new(IdKind::WorkflowInstance, r.0).as_str(),
            definition_id: r.15,
            version_id: PublicId::new(IdKind::WorkflowVersion, r.2).as_str(),
            version_number: r.3,
            status: r.4,
            actor_user_id: user_public(r.5),
            temporal_workflow_id: r.6,
            step_count: r.7,
            current_node_id: r.8,
            error_message: r.9,
            waiting_until: r.10,
            sla_deadline: r.11,
            started_at: r.12,
            updated_at: r.13,
            completed_at: r.14,
        })
        .collect();

    Ok(Json(MonitorResponse {
        summary: MonitorSummaryDto {
            running: summary.0,
            waiting: summary.1,
            failed: summary.2,
            completed: summary.3,
            cancelled: summary.4,
            sla_breached: summary.5,
        },
        instances,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/bounds",
    tag = "workflows-monitor",
    responses((status = 200, body = OrgBoundsDto))
)]
pub async fn get_bounds(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<OrgBoundsDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let bounds = load_or_default_bounds(&mut tx, auth.ctx.org_id.as_uuid())
        .await
        .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(bounds))
}

#[utoipa::path(
    put,
    path = "/api/v1/workflows/bounds",
    tag = "workflows-monitor",
    request_body = UpdateOrgBoundsRequest,
    responses((status = 200, body = OrgBoundsDto))
)]
pub async fn update_bounds(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<UpdateOrgBoundsRequest>,
) -> Result<Json<OrgBoundsDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_manage(), rid)?;

    if body.max_concurrent < 1 || body.max_concurrent > 1000 {
        return Err(validation(rid, "max_concurrent must be 1..=1000"));
    }
    if body.max_steps_per_instance < 1 || body.max_steps_per_instance > 10_000 {
        return Err(validation(rid, "max_steps_per_instance must be 1..=10000"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    sqlx::query(
        r#"
        INSERT INTO workflow_org_bounds (org_id, max_concurrent, max_steps_per_instance, updated_by)
        VALUES ($1,$2,$3,$4)
        ON CONFLICT (org_id) DO UPDATE
        SET max_concurrent = EXCLUDED.max_concurrent,
            max_steps_per_instance = EXCLUDED.max_steps_per_instance,
            updated_by = EXCLUDED.updated_by,
            updated_at = now()
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(body.max_concurrent)
    .bind(body.max_steps_per_instance)
    .bind(auth.ctx.actor.on_behalf_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let _ = (DEFAULT_MAX_CONCURRENT, DEFAULT_MAX_STEPS);
    Ok(Json(OrgBoundsDto {
        max_concurrent: body.max_concurrent,
        max_steps_per_instance: body.max_steps_per_instance,
    }))
}
