use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::forecast::{
    ForecastInputs, ForecastMethod, ForecastPoint, ForecastRequest, ForecastResponse,
};
use crate::handlers::{
    benchmarks, dashboards, export, facts, forecasts, freshness, ingest, metrics, reconcile,
    reports, schedules,
};
use crate::metrics::{FactSource, MeasureKind, MetricDefinition, MetricUnit};
use crate::query::{QueryFilter, QueryResult, QueryRow, ReportDefinition};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        ingest::ingest,
        facts::invoice_issued,
        reconcile::nightly,
        metrics::metrics,
        metrics::golden,
        reports::list_reports,
        reports::create_report,
        reports::get_report,
        reports::update_report,
        reports::delete_report,
        reports::run_report,
        reports::simulate_query,
        reports::run_query,
        dashboards::list_dashboards,
        dashboards::create_dashboard,
        dashboards::get_dashboard,
        dashboards::update_dashboard,
        dashboards::delete_dashboard,
        dashboards::upsert_widget,
        dashboards::delete_widget,
        forecasts::forecast,
        export::export_report,
        schedules::list_schedules,
        schedules::create_schedule,
        schedules::get_schedule,
        schedules::update_schedule,
        schedules::delete_schedule,
        schedules::fire_schedule,
        freshness::freshness,
        benchmarks::benchmarks,
    ),
    components(schemas(
        InvoiceIssuedFact,
        FactsResponse,
        IngestResponse,
        ReconcileResponse,
        FactSource,
        MeasureKind,
        MetricDefinition,
        MetricUnit,
        MetricListResponse,
        QueryFilter,
        QueryRow,
        QueryResult,
        ReportDefinition,
        ReportDto,
        CreateReportRequest,
        UpdateReportRequest,
        ReportListResponse,
        RunReportRequest,
        RunReportResponse,
        SimulateQueryRequest,
        DashboardDto,
        CreateDashboardRequest,
        UpdateDashboardRequest,
        DashboardListResponse,
        WidgetDto,
        UpsertWidgetRequest,
        ForecastMethod,
        ForecastRequest,
        ForecastPoint,
        ForecastInputs,
        ForecastResponse,
        ExportRequest,
        ExportResponse,
        ScheduleDto,
        CreateScheduleRequest,
        UpdateScheduleRequest,
        FireScheduleRequest,
        FireScheduleResponse,
        FreshnessResponse,
        BenchmarkMetric,
        BenchmarkResponse,
        DrillRequest,
        DrillRecord,
        DrillResponse,
    )),
    tags(
        (name = "analytics", description = "Event-derived analytics and freshness"),
        (name = "analytics-internal", description = "Event ingest"),
        (name = "analytics-metrics", description = "Governed semantic metrics"),
        (name = "analytics-reports", description = "Saved and ad-hoc reports"),
        (name = "analytics-dashboards", description = "Dashboards and governed widgets"),
        (name = "analytics-forecasts", description = "Explainable forecasts"),
        (name = "analytics-schedules", description = "Scheduled report delivery"),
    ),
    info(
        title = "CompanyOS Analytics API",
        version = "0.3.2",
        description = "Phase 3.2 analytics and reporting; facts derive only from events (ADR-011)."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/analytics/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
