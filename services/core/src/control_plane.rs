//! Phase 4.1 control plane — region catalogue, cell health, org routing.
//!
//! These routes are **not** on the public API-key allowlist. They expose the
//! global control-plane slice (identity-adjacent): region map + cell health +
//! org→home-region directory lookups. Data-plane remains cell-local.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::{
    run_failover_drill, CellHealth, CellId, ControlPlane, RegionCode, CI_FAILOVER_DRILL_BUDGET,
    PRODUCTION_REGION_RTO,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::extract::AuthUser;
use crate::state::AppState;

/// Shared in-process cell health + routing directory for CI / compose drills.
/// Production replaces this with a global control-plane service; the contracts
/// (catalogue, health, resolve, failover drill) stay the same.
#[derive(Clone, Default)]
pub struct CellPlaneState {
    pub(crate) inner: Arc<RwLock<ControlPlane>>,
}

impl CellPlaneState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ControlPlane::new())),
        }
    }

    pub async fn register_org(&self, org_public_id: &str, region: RegionCode) {
        self.inner.write().await.register_org(org_public_id, region);
    }

    pub async fn snapshot(&self) -> ControlPlane {
        self.inner.read().await.clone()
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RegionPolicyView {
    pub region: String,
    pub jurisdiction: String,
    pub home_cell_only_data_plane: bool,
    pub allowed_replicas: Vec<String>,
    pub primary_cell: String,
    pub standby_cell: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CellHealthView {
    pub cell_id: String,
    pub region: String,
    pub health: String,
    pub is_standby: bool,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetCellHealthRequest {
    pub health: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgRoutingView {
    pub org_id: String,
    pub home_region: String,
    pub serving_cell: String,
    pub failover: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct FailoverDrillRequest {
    pub org_id: String,
    pub fail_cell: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FailoverDrillResponse {
    pub scenario: String,
    pub success: bool,
    pub elapsed_ms: u64,
    pub within_budget: bool,
    pub budget_ms: u64,
    pub production_rto_secs: u64,
    pub serving_cell: Option<String>,
    pub error: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/control-plane/regions", get(list_regions))
        .route("/api/v1/control-plane/cells", get(list_cells))
        .route(
            "/api/v1/control-plane/cells/{cell_id}/health",
            put(set_cell_health),
        )
        .route(
            "/api/v1/control-plane/orgs/{org_id}/routing",
            get(org_routing),
        )
        .route("/api/v1/control-plane/failover-drill", post(failover_drill))
}

async fn list_regions() -> Json<Vec<RegionPolicyView>> {
    let views = companyos_tenancy::region_catalogue()
        .iter()
        .map(|p| RegionPolicyView {
            region: p.region.as_str().into(),
            jurisdiction: p.jurisdiction.into(),
            home_cell_only_data_plane: p.home_cell_only_data_plane,
            allowed_replicas: p
                .allowed_replicas
                .iter()
                .map(|r| format!("{r:?}").to_ascii_lowercase())
                .collect(),
            primary_cell: CellId::primary_for(p.region).as_str().into(),
            standby_cell: CellId::standby_for(p.region).map(|c| c.as_str().into()),
        })
        .collect();
    Json(views)
}

async fn list_cells(State(state): State<AppState>) -> Json<Vec<CellHealthView>> {
    let plane = state.cell_plane.snapshot().await;
    let views = CellId::ALL
        .iter()
        .map(|c| {
            let h = plane.cell_health(*c);
            CellHealthView {
                cell_id: c.as_str().into(),
                region: c.region().as_str().into(),
                health: match h {
                    CellHealth::Healthy => "healthy",
                    CellHealth::Unhealthy => "unhealthy",
                    CellHealth::Unknown => "unknown",
                }
                .into(),
                is_standby: c.is_standby(),
            }
        })
        .collect();
    Json(views)
}

async fn set_cell_health(
    State(state): State<AppState>,
    user: AuthUser,
    Path(cell_id): Path<String>,
    Json(body): Json<SetCellHealthRequest>,
) -> Result<Json<CellHealthView>, AppError> {
    let request_id = user.ctx.request_id.clone();
    let cell = CellId::parse(&cell_id).map_err(|e| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            e.to_string(),
        )
    })?;
    let health = match body.health.to_ascii_lowercase().as_str() {
        "healthy" => CellHealth::Healthy,
        "unhealthy" => CellHealth::Unhealthy,
        "unknown" => CellHealth::Unknown,
        other => {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                format!("invalid health '{other}'"),
            ))
        }
    };

    {
        let mut plane = state.cell_plane.inner.write().await;
        plane.set_cell_health(cell, health);
    }

    record_routing_audit(
        &state,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "control_plane.cell_health.set",
        serde_json::json!({
            "cell_id": cell.as_str(),
            "health": body.health,
        }),
    )
    .await;

    Ok(Json(CellHealthView {
        cell_id: cell.as_str().into(),
        region: cell.region().as_str().into(),
        health: body.health.to_ascii_lowercase(),
        is_standby: cell.is_standby(),
    }))
}

async fn org_routing(
    State(state): State<AppState>,
    user: AuthUser,
    Path(org_id): Path<String>,
) -> Result<Json<OrgRoutingView>, AppError> {
    let request_id = user.ctx.request_id.clone();

    // Ensure directory knows this org (hydrate from DB if missing).
    hydrate_org_region(&state, &org_id, &request_id).await?;

    let plane = state.cell_plane.snapshot().await;
    let decision = plane.resolve_serving_cell(&org_id).map_err(|e| {
        let code = match e {
            companyos_tenancy::RegionError::FailoverDenied
            | companyos_tenancy::RegionError::CellUnavailable(_) => ErrorCode::ServiceUnavailable,
            companyos_tenancy::RegionError::RegionMismatch { .. } => ErrorCode::ResidencyViolation,
            _ => ErrorCode::NotFound,
        };
        AppError::new(code, request_id, e.to_string())
    })?;

    Ok(Json(OrgRoutingView {
        org_id: decision.org_id,
        home_region: decision.home_region.as_str().into(),
        serving_cell: decision.serving_cell.as_str().into(),
        failover: decision.failover,
        reason: decision.reason,
    }))
}

async fn failover_drill(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<FailoverDrillRequest>,
) -> Result<Json<FailoverDrillResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    let fail_cell = CellId::parse(&body.fail_cell).map_err(|e| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            e.to_string(),
        )
    })?;

    hydrate_org_region(&state, &body.org_id, &request_id).await?;

    let report = {
        let mut plane = state.cell_plane.inner.write().await;
        run_failover_drill(
            &mut plane,
            &body.org_id,
            fail_cell,
            CI_FAILOVER_DRILL_BUDGET,
        )
    };

    record_routing_audit(
        &state,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "control_plane.failover_drill",
        serde_json::json!({
            "org_id": body.org_id,
            "fail_cell": body.fail_cell,
            "success": report.success,
            "elapsed_ms": report.elapsed.as_millis() as u64,
            "error": report.error,
        }),
    )
    .await;

    Ok(Json(FailoverDrillResponse {
        scenario: report.scenario,
        success: report.success,
        elapsed_ms: report.elapsed.as_millis() as u64,
        within_budget: report.within_budget,
        budget_ms: report.budget.as_millis() as u64,
        production_rto_secs: PRODUCTION_REGION_RTO.as_secs(),
        serving_cell: report
            .decision
            .as_ref()
            .map(|d| d.serving_cell.as_str().to_string()),
        error: report.error,
    }))
}

async fn hydrate_org_region(
    state: &AppState,
    org_public_id: &str,
    request_id: &str,
) -> Result<RegionCode, AppError> {
    if let Some(r) = state.cell_plane.snapshot().await.org_region(org_public_id) {
        return Ok(r);
    }
    let row: Option<(String,)> =
        sqlx::query_as("SELECT region FROM organization WHERE public_id = $1")
            .bind(org_public_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let Some((region_raw,)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "organization not found",
        ));
    };
    let region = RegionCode::parse(&region_raw)
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    state.cell_plane.register_org(org_public_id, region).await;
    Ok(region)
}

async fn record_routing_audit(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    action: &str,
    detail: serde_json::Value,
) {
    let id = new_uuid_v7();
    let _ = sqlx::query(
        r#"
        INSERT INTO region_routing_audit (id, org_id, actor_user_id, action, detail)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(actor)
    .bind(action)
    .bind(detail)
    .execute(&state.pool)
    .await;
}

/// Parse optional region from create/register body; default `us`.
pub fn parse_region_or_default(raw: Option<&str>) -> Result<RegionCode, String> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(RegionCode::Us),
        Some(s) => RegionCode::parse(s).map_err(|e| e.to_string()),
    }
}
