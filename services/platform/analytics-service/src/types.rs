use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub use crate::forecast::{
    ForecastInputs, ForecastMethod, ForecastPoint, ForecastRequest, ForecastResponse,
};
pub use crate::query::{QueryFilter, QueryResult, QueryRow, ReportDefinition};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvoiceIssuedFact {
    pub event_id: String,
    pub org_id: String,
    pub invoice_id: String,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub issued_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FactsResponse {
    pub facts: Vec<InvoiceIssuedFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IngestResponse {
    pub accepted: bool,
    pub duplicate: bool,
    pub fact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconcileResponse {
    pub mirror_count: i64,
    pub expected_count: i64,
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MetricListResponse {
    pub metrics: Vec<crate::metrics::MetricDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportDto {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub description: String,
    pub definition: ReportDefinition,
    pub visualization: String,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateReportRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub definition: ReportDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateReportRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub definition: Option<ReportDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportListResponse {
    pub reports: Vec<ReportDto>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct RunReportRequest {
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RunReportResponse {
    pub run_id: String,
    pub report_id: Option<String>,
    pub result: QueryResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SimulateQueryRequest {
    pub definition: ReportDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardDto {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub description: String,
    pub layout: serde_json::Value,
    pub widgets: Vec<WidgetDto>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDashboardRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_layout")]
    pub layout: serde_json::Value,
}

fn default_layout() -> serde_json::Value {
    serde_json::Value::Array(vec![])
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateDashboardRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub layout: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardListResponse {
    pub dashboards: Vec<DashboardDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WidgetDto {
    pub id: String,
    pub dashboard_id: String,
    pub title: String,
    pub metric_name: String,
    pub visualization: String,
    pub config: serde_json::Value,
    pub position: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertWidgetRequest {
    pub id: Option<String>,
    pub title: String,
    pub metric_name: String,
    #[serde(default = "default_widget_visualization")]
    pub visualization: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub position: i32,
}

fn default_widget_visualization() -> String {
    "stat".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ScheduleDto {
    pub id: String,
    pub report_id: String,
    pub cron: String,
    pub timezone: String,
    pub channel: String,
    pub recipients: Vec<String>,
    pub export_format: String,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateScheduleRequest {
    pub report_id: String,
    pub cron: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_channel")]
    pub channel: String,
    #[serde(default)]
    pub recipients: Vec<String>,
    #[serde(default = "default_export_format")]
    pub export_format: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateScheduleRequest {
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub channel: Option<String>,
    pub recipients: Option<Vec<String>>,
    pub export_format: Option<String>,
    pub enabled: Option<bool>,
}

fn default_timezone() -> String {
    "UTC".into()
}

fn default_channel() -> String {
    "notification".into()
}

fn default_export_format() -> String {
    "csv".into()
}

const fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct FireScheduleRequest {
    pub export_format: Option<String>,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FireScheduleResponse {
    pub schedule_id: String,
    pub run_id: String,
    pub workflow_id: String,
    pub workflow_type: String,
    pub state: String,
    pub export: ExportResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExportRequest {
    #[serde(default = "default_export_format")]
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExportResponse {
    pub run_id: String,
    pub report_id: String,
    pub format: String,
    pub content_type: String,
    pub file_id: String,
    pub content: String,
    pub row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FreshnessResponse {
    pub org_id: String,
    pub last_event_at: Option<DateTime<Utc>>,
    pub last_ingest_at: Option<DateTime<Utc>>,
    pub lag_seconds: i64,
    pub eventually_consistent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BenchmarkMetric {
    pub metric: String,
    pub display_name: String,
    pub unit: crate::metrics::MetricUnit,
    pub current_value: i64,
    pub previous_value: i64,
    pub trend_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BenchmarkResponse {
    pub org_id: String,
    pub window_days: u32,
    pub benchmarks: Vec<BenchmarkMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DrillRequest {
    pub definition: ReportDefinition,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DrillRecord {
    pub record_id: String,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DrillResponse {
    pub metric: String,
    pub records: Vec<DrillRecord>,
    pub filtered_by_permission: bool,
}
