//! API DTOs for the CompanyOS CRM / Sales service (`/api/v1/sales/...`).
//!
//! Conventions:
//! - Public ids are prefixed strings (`cus_…`, `dl_…`, `qte_…`, …).
//! - Money is always `amount_minor: i64` + `currency: String` — never floats.
//! - Timestamps are RFC3339 strings.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ---------------------------------------------------------------------------
// Pipelines / stages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PipelineDto {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PipelineListResponse {
    pub items: Vec<PipelineDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StageDto {
    pub id: String,
    pub pipeline_id: String,
    pub name: String,
    pub position: i32,
    pub probability: i32,
    pub is_won: bool,
    pub is_lost: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BoardStage {
    pub stage: StageDto,
    pub deals: Vec<DealDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BoardResponse {
    pub pipeline: PipelineDto,
    pub stages: Vec<BoardStage>,
}

// ---------------------------------------------------------------------------
// Customers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomerDto {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub billing_address: Option<String>,
    pub notes: Option<String>,
    pub owner_user_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomerListResponse {
    pub items: Vec<CustomerDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateCustomerRequest {
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub billing_address: Option<String>,
    pub notes: Option<String>,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateCustomerRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub billing_address: Option<String>,
    pub notes: Option<String>,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateCustomerResponse {
    pub customer: CustomerDto,
    #[serde(default)]
    pub duplicate_warnings: Vec<DuplicateMatch>,
}

// ---------------------------------------------------------------------------
// Contacts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ContactDto {
    pub id: String,
    pub customer_id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    pub is_primary: bool,
    pub owner_user_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ContactListResponse {
    pub items: Vec<ContactDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateContactRequest {
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateContactRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub title: Option<String>,
    pub is_primary: Option<bool>,
    pub owner_user_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Leads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeadDto {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub source: Option<String>,
    pub status: String,
    pub score: i32,
    pub owner_user_id: Option<String>,
    pub notes: Option<String>,
    pub converted_customer_id: Option<String>,
    pub converted_deal_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeadListResponse {
    pub items: Vec<LeadDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateLeadRequest {
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub source: Option<String>,
    pub owner_user_id: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateLeadRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub source: Option<String>,
    pub status: Option<String>,
    pub score: Option<i32>,
    pub owner_user_id: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DisqualifyLeadRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ConvertLeadRequest {
    pub deal_name: Option<String>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConvertLeadResponse {
    pub lead: LeadDto,
    pub customer: CustomerDto,
    pub deal: DealDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DuplicateMatch {
    pub customer_id: Option<String>,
    pub lead_id: Option<String>,
    pub name: String,
    pub email: Option<String>,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DuplicateCheckResponse {
    pub matches: Vec<DuplicateMatch>,
}

// ---------------------------------------------------------------------------
// Deals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DealDto {
    pub id: String,
    pub pipeline_id: String,
    pub stage_id: String,
    pub customer_id: Option<String>,
    pub lead_id: Option<String>,
    pub name: String,
    pub amount_minor: i64,
    pub currency: String,
    pub probability: Option<i32>,
    pub expected_close_date: Option<String>,
    pub owner_user_id: Option<String>,
    pub status: String,
    pub won_reason: Option<String>,
    pub lost_reason: Option<String>,
    pub won_at: Option<String>,
    pub lost_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DealListResponse {
    pub items: Vec<DealDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDealRequest {
    pub pipeline_id: Option<String>,
    pub stage_id: Option<String>,
    pub customer_id: Option<String>,
    pub lead_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub amount_minor: i64,
    pub currency: Option<String>,
    pub probability: Option<i32>,
    pub expected_close_date: Option<String>,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateDealRequest {
    pub stage_id: Option<String>,
    pub name: Option<String>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub probability: Option<i32>,
    pub expected_close_date: Option<String>,
    pub owner_user_id: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct WinDealRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct LoseDealRequest {
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Activities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActivityDto {
    pub id: String,
    pub kind: String,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub occurred_at: String,
    pub customer_id: Option<String>,
    pub deal_id: Option<String>,
    pub lead_id: Option<String>,
    pub owner_user_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActivityListResponse {
    pub items: Vec<ActivityDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateActivityRequest {
    pub kind: String,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub occurred_at: Option<String>,
    pub customer_id: Option<String>,
    pub deal_id: Option<String>,
    pub lead_id: Option<String>,
    pub owner_user_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProductDto {
    pub id: String,
    pub name: String,
    pub sku: Option<String>,
    pub unit_price_minor: Option<i64>,
    pub currency: Option<String>,
    pub tax_group: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProductListResponse {
    pub items: Vec<ProductDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateProductRequest {
    pub name: String,
    pub sku: Option<String>,
    pub unit_price_minor: Option<i64>,
    pub currency: Option<String>,
    pub tax_group: Option<String>,
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProductRequest {
    pub name: Option<String>,
    pub sku: Option<String>,
    pub unit_price_minor: Option<i64>,
    pub currency: Option<String>,
    pub tax_group: Option<String>,
    pub active: Option<bool>,
}

// ---------------------------------------------------------------------------
// Quotes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuoteLineDto {
    pub id: String,
    pub position: i32,
    pub product_id: Option<String>,
    pub description: String,
    pub quantity: i32,
    pub unit_price_minor: i64,
    pub discount_minor: i64,
    pub tax_rate_bps: i32,
    pub tax_minor: i64,
    pub line_total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuoteDto {
    pub id: String,
    pub deal_id: Option<String>,
    pub customer_id: String,
    pub quote_number: String,
    pub status: String,
    pub version_number: i32,
    pub previous_quote_id: Option<String>,
    pub currency: String,
    pub subtotal_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub notes: Option<String>,
    pub valid_until: Option<String>,
    pub accepted_at: Option<String>,
    pub owner_user_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
    pub lines: Vec<QuoteLineDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuoteListResponse {
    pub items: Vec<QuoteDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateQuoteLineRequest {
    pub product_id: Option<String>,
    #[serde(default)]
    pub description: String,
    pub quantity: i32,
    pub unit_price_minor: i64,
    #[serde(default)]
    pub discount_minor: i64,
    #[serde(default)]
    pub tax_rate_bps: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateQuoteRequest {
    pub deal_id: Option<String>,
    pub customer_id: String,
    pub quote_number: Option<String>,
    pub currency: Option<String>,
    pub notes: Option<String>,
    pub valid_until: Option<String>,
    pub owner_user_id: Option<String>,
    #[serde(default)]
    pub lines: Vec<CreateQuoteLineRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateQuoteRequest {
    pub notes: Option<String>,
    pub valid_until: Option<String>,
    pub currency: Option<String>,
    pub owner_user_id: Option<String>,
    pub lines: Option<Vec<CreateQuoteLineRequest>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct RejectQuoteRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvoiceActionResponse {
    pub available: bool,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportPreviewRequest {
    /// Raw CSV text (header row + data rows).
    pub csv: String,
    /// Optional header alias overrides, e.g. `{"Company Name": "company"}`.
    #[serde(default)]
    pub mapping: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportRowPreview {
    pub row_number: i32,
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    #[serde(default)]
    pub duplicates: Vec<DuplicateMatch>,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportPreviewResponse {
    pub rows: Vec<ImportRowPreview>,
    pub exact_duplicate_count: usize,
    pub near_duplicate_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportRowInput {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ImportConfirmRequest {
    /// Raw CSV text — alternative to `rows`.
    pub csv: Option<String>,
    /// Inline rows — alternative to `csv`.
    pub rows: Option<Vec<ImportRowInput>>,
    #[serde(default)]
    pub mapping: std::collections::HashMap<String, String>,
    /// Skip rows that look like exact-email duplicates of existing customers.
    #[serde(default)]
    pub skip_exact_duplicates: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportConfirmResponse {
    pub job_id: String,
    pub imported: i64,
    pub skipped: i64,
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StageSummary {
    pub stage_id: String,
    pub stage_name: String,
    pub open_deal_count: i64,
    pub open_amount_minor: i64,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WinRateSummary {
    pub won_count: i64,
    pub lost_count: i64,
    pub win_rate_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActivityVolumeItem {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WeightedForecast {
    pub amount_minor: i64,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportSummaryResponse {
    pub pipeline_by_stage: Vec<StageSummary>,
    pub win_rate: WinRateSummary,
    pub activity_volume: Vec<ActivityVolumeItem>,
    pub weighted_forecast: WeightedForecast,
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct DealListQuery {
    pub q: Option<String>,
    pub stage_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ActivityListQuery {
    pub customer_id: Option<String>,
    pub deal_id: Option<String>,
    pub lead_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
