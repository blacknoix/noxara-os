pub mod benchmarks;
pub mod dashboards;
pub mod export;
pub mod facts;
pub mod forecasts;
pub mod freshness;
pub mod ingest;
pub mod metrics;
pub mod reconcile;
pub mod reports;
pub mod schedules;

use axum::routing::{delete, get, post};
use axum::Router;
use companyos_authz::{PermissionId, Principal};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::OrgId;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/analytics/internal/ingest", post(ingest::ingest))
        .route(
            "/api/v1/analytics/facts/invoice-issued",
            get(facts::invoice_issued),
        )
        .route(
            "/api/v1/analytics/reconcile/nightly",
            post(reconcile::nightly),
        )
        .route("/api/v1/analytics/metrics", get(metrics::metrics))
        .route("/api/v1/analytics/metrics/golden", get(metrics::golden))
        .route(
            "/api/v1/analytics/reports",
            get(reports::list_reports).post(reports::create_report),
        )
        .route(
            "/api/v1/analytics/reports/simulate",
            post(reports::simulate_query),
        )
        .route(
            "/api/v1/analytics/reports/{id}",
            get(reports::get_report)
                .patch(reports::update_report)
                .delete(reports::delete_report),
        )
        .route(
            "/api/v1/analytics/reports/{id}/run",
            post(reports::run_report),
        )
        .route(
            "/api/v1/analytics/reports/{id}/export",
            post(export::export_report),
        )
        .route("/api/v1/analytics/query/run", post(reports::run_query))
        .route(
            "/api/v1/analytics/dashboards",
            get(dashboards::list_dashboards).post(dashboards::create_dashboard),
        )
        .route(
            "/api/v1/analytics/dashboards/{id}",
            get(dashboards::get_dashboard)
                .patch(dashboards::update_dashboard)
                .delete(dashboards::delete_dashboard),
        )
        .route(
            "/api/v1/analytics/dashboards/{id}/widgets",
            post(dashboards::upsert_widget),
        )
        .route(
            "/api/v1/analytics/dashboards/{id}/widgets/{widget_id}",
            delete(dashboards::delete_widget),
        )
        .route("/api/v1/analytics/forecasts", post(forecasts::forecast))
        .route("/api/v1/analytics/freshness", get(freshness::freshness))
        .route("/api/v1/analytics/benchmarks", get(benchmarks::benchmarks))
        .route(
            "/api/v1/analytics/schedules",
            get(schedules::list_schedules).post(schedules::create_schedule),
        )
        .route(
            "/api/v1/analytics/schedules/{id}",
            get(schedules::get_schedule)
                .patch(schedules::update_schedule)
                .delete(schedules::delete_schedule),
        )
        .route(
            "/api/v1/analytics/schedules/{id}/fire",
            post(schedules::fire_schedule),
        )
}

pub(crate) fn internal(request_id: &str) -> impl Fn(sqlx::Error) -> AppError + '_ {
    move |error| AppError::new(ErrorCode::Internal, request_id, error.to_string())
}

pub(crate) fn not_found(request_id: &str, resource: &str) -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        request_id,
        format!("{resource} not found"),
    )
}

pub(crate) fn parse_id(kind: IdKind, raw: &str, request_id: &str) -> Result<Uuid, AppError> {
    let public: PublicId = raw.parse().map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("invalid {} id", kind.prefix()),
        )
    })?;
    if public.kind() != kind {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("id must use {} prefix", kind.prefix()),
        ));
    }
    Ok(public.uuid())
}

pub(crate) fn user_public(user_id: Uuid) -> String {
    PublicId::new(IdKind::User, user_id).as_str()
}

pub(crate) async fn set_org(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    request_id: &str,
) -> Result<(), AppError> {
    companyos_tenancy::set_session_org_id(tx, org_id)
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))
}

pub(crate) async fn authorize(
    state: &AppState,
    auth: &AuthCtx,
    permission: PermissionId,
) -> Result<Option<Principal>, AppError> {
    if auth.local_bypass {
        return Ok(None);
    }
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        &auth.ctx.request_id,
    )
    .await?;
    enforce(&principal, permission, &auth.ctx.request_id)?;
    Ok(Some(principal))
}

pub(crate) fn ensure_human(auth: &AuthCtx) -> Result<(), AppError> {
    if auth.ctx.actor.is_ai {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &auth.ctx.request_id,
            "AI actors cannot persist analytics configuration",
        ));
    }
    Ok(())
}
