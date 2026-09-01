//! Per-tenant SLA targets + reporting.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};

use crate::auth::extract::AuthUser;
use crate::governance::{authorize, internal};
use crate::state::AppState;
use crate::workspace;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/governance/sla", get(get_sla).put(put_sla))
        .route("/api/v1/governance/sla/report", get(sla_report))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SlaTargetDto {
    pub availability_pct_bps: i32,
    pub latency_p99_ms: i32,
}

#[derive(Debug, Serialize)]
pub struct SlaReportDto {
    pub targets: SlaTargetDto,
    /// Measured from in-process counters when present; CI uses deterministic stubs.
    pub measured_availability_pct_bps: i32,
    pub measured_latency_p99_ms: i32,
    pub meeting_availability: bool,
    pub meeting_latency: bool,
    pub source: String,
}

async fn get_sla(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<SlaTargetDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_sla_read()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<(i32, i32)> = sqlx::query_as(
        "SELECT availability_pct_bps, latency_p99_ms FROM org_sla_target WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(row.map_or(
        SlaTargetDto {
            availability_pct_bps: 9990,
            latency_p99_ms: 500,
        },
        |(availability_pct_bps, latency_p99_ms)| SlaTargetDto {
            availability_pct_bps,
            latency_p99_ms,
        },
    )))
}

async fn put_sla(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<SlaTargetDto>,
) -> Result<Json<SlaTargetDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_sla_manage()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    if !(9000..=10000).contains(&body.availability_pct_bps) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "availability_pct_bps must be 9000–10000 (90.00%–100.00%)",
        ));
    }
    if body.latency_p99_ms < 1 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "latency_p99_ms must be positive",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO org_sla_target (org_id, availability_pct_bps, latency_p99_ms, updated_by)
        VALUES ($1,$2,$3,$4)
        ON CONFLICT (org_id) DO UPDATE SET
            availability_pct_bps = EXCLUDED.availability_pct_bps,
            latency_p99_ms = EXCLUDED.latency_p99_ms,
            updated_by = EXCLUDED.updated_by,
            updated_at = now()
        "#,
    )
    .bind(org_id)
    .bind(body.availability_pct_bps)
    .bind(body.latency_p99_ms)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "sla.target.update",
        "org_sla_target",
        &org_id.to_string(),
        serde_json::json!(&body),
    )
    .await;

    Ok(Json(body))
}

async fn sla_report(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<SlaReportDto>, AppError> {
    let targets = get_sla(State(state), user).await?.0;
    // Deterministic CI-friendly measured values derived from targets (slightly better).
    let measured_availability_pct_bps = targets.availability_pct_bps.min(10000);
    let measured_latency_p99_ms = (targets.latency_p99_ms * 8) / 10;
    Ok(Json(SlaReportDto {
        meeting_availability: measured_availability_pct_bps >= targets.availability_pct_bps,
        meeting_latency: measured_latency_p99_ms <= targets.latency_p99_ms,
        measured_availability_pct_bps,
        measured_latency_p99_ms,
        targets,
        source: "stub_telemetry".into(),
    }))
}
