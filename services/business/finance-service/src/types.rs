//! API DTOs for `/api/v1/finance/...`.
//! Money is always `*_minor: i64` + ISO currency — never floats.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FinanceCustomerDto {
    pub id: String,
    pub sales_customer_id: String,
    pub name: String,
    pub email: Option<String>,
    pub currency: String,
    pub outstanding_balance_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FinanceCustomerListResponse {
    pub items: Vec<FinanceCustomerDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvoiceLineDto {
    pub id: String,
    pub description: String,
    pub quantity: i64,
    pub unit_price_minor: i64,
    pub discount_minor: i64,
    pub tax_rate_bps: i64,
    pub tax_minor: i64,
    pub line_total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvoiceDto {
    pub id: String,
    pub customer_id: String,
    pub status: String,
    pub invoice_number: Option<String>,
    pub currency: String,
    pub base_currency: String,
    pub fx_rate_num: Option<i64>,
    pub fx_rate_den: Option<i64>,
    pub fx_rate_date: Option<String>,
    pub subtotal_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub base_total_minor: i64,
    pub amount_paid_minor: i64,
    pub amount_credited_minor: i64,
    pub balance_minor: i64,
    pub issue_date: Option<String>,
    pub due_date: Option<String>,
    pub payment_url: Option<String>,
    pub source_quote_id: Option<String>,
    pub notes: Option<String>,
    pub terms: Option<String>,
    pub version: i32,
    pub lines: Vec<InvoiceLineDto>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvoiceListResponse {
    pub items: Vec<InvoiceDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvoiceLineInput {
    pub description: String,
    pub quantity: i64,
    pub unit_price_minor: i64,
    #[serde(default)]
    pub discount_minor: i64,
    #[serde(default)]
    pub tax_rate_bps: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateInvoiceRequest {
    /// Finance customer public id (`cus_…` projection) or sales customer id.
    pub customer_id: String,
    pub currency: String,
    #[serde(default = "default_base_currency")]
    pub base_currency: String,
    pub lines: Vec<InvoiceLineInput>,
    pub due_date: Option<String>,
    pub notes: Option<String>,
    pub terms: Option<String>,
}

fn default_base_currency() -> String {
    "USD".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateInvoiceRequest {
    pub lines: Option<Vec<InvoiceLineInput>>,
    pub due_date: Option<String>,
    pub notes: Option<String>,
    pub terms: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueInvoiceRequest {
    /// FX rate as rational num/den captured at issue (document currency → base).
    #[serde(default = "default_fx_num")]
    pub fx_rate_num: i64,
    #[serde(default = "default_fx_den")]
    pub fx_rate_den: i64,
    pub fx_rate_date: Option<String>,
    pub issue_date: Option<String>,
    pub due_date: Option<String>,
}

fn default_fx_num() -> i64 {
    1
}
fn default_fx_den() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuoteLineSnapshot {
    pub description: String,
    pub quantity: i64,
    pub unit_price_minor: i64,
    #[serde(default)]
    pub discount_minor: i64,
    #[serde(default)]
    pub tax_rate_bps: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateInvoiceFromQuoteRequest {
    pub quote_id: String,
    pub customer_id: String,
    pub customer_name: String,
    pub currency: String,
    pub lines: Vec<QuoteLineSnapshot>,
    pub terms: Option<String>,
    pub notes: Option<String>,
    pub total_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaymentDto {
    pub id: String,
    pub customer_id: String,
    pub currency: String,
    pub amount_minor: i64,
    pub amount_allocated_minor: i64,
    pub amount_unapplied_minor: i64,
    pub method: String,
    pub provider: Option<String>,
    pub received_at: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecordPaymentRequest {
    pub customer_id: String,
    pub currency: String,
    pub amount_minor: i64,
    pub invoice_id: Option<String>,
    pub notes: Option<String>,
    pub received_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AllocatePaymentRequest {
    pub invoice_id: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreditNoteDto {
    pub id: String,
    pub invoice_id: String,
    pub customer_id: String,
    pub credit_number: String,
    pub currency: String,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub reason: Option<String>,
    pub issued_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateCreditNoteRequest {
    pub invoice_id: String,
    pub lines: Vec<InvoiceLineInput>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExpenseDto {
    pub id: String,
    pub status: String,
    pub currency: String,
    pub amount_minor: i64,
    pub description: String,
    pub category_code: Option<String>,
    pub receipt_url: Option<String>,
    pub incurred_at: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubmitExpenseRequest {
    pub currency: String,
    pub amount_minor: i64,
    pub description: String,
    pub category_code: Option<String>,
    pub receipt_url: Option<String>,
    pub incurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DecideExpenseRequest {
    pub approve: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportSummaryDto {
    pub as_of: String,
    pub currency: String,
    pub revenue_minor: i64,
    pub expenses_minor: i64,
    pub cash_minor: i64,
    pub receivables_minor: i64,
    pub ageing: Vec<AgeingBucket>,
    pub expenses_by_category: Vec<CategoryAmount>,
    pub cash_flow: Vec<CashFlowPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgeingBucket {
    pub label: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CategoryAmount {
    pub category: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CashFlowPoint {
    pub period: String,
    pub inflow_minor: i64,
    pub outflow_minor: i64,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
    pub customer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaymentListResponse {
    pub items: Vec<PaymentDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExpenseListResponse {
    pub items: Vec<ExpenseDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreditNoteListResponse {
    pub items: Vec<CreditNoteDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecurringListResponse {
    pub items: Vec<RecurringInvoiceDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RunDueResponse {
    pub created_invoice_ids: Vec<String>,
    pub processed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StripeWebhookFixture {
    pub id: String,
    #[serde(rename = "type")]
    pub type_field: String,
    pub created: i64,
    pub data: StripeWebhookData,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StripeWebhookData {
    pub object: StripePaymentObject,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StripePaymentObject {
    pub id: String,
    pub amount: i64,
    pub currency: String,
    pub customer_id: String,
    pub invoice_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookAck {
    pub received: bool,
    pub duplicate: bool,
    pub payment_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecurringInvoiceDto {
    pub id: String,
    pub customer_id: String,
    pub cadence: String,
    pub next_run_at: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateRecurringRequest {
    pub customer_id: String,
    pub cadence: String,
    pub next_run_at: String,
    pub template: CreateInvoiceRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApplySalesEventRequest {
    /// Full event envelope JSON (same shape as outbox payload).
    #[schema(value_type = Object)]
    pub envelope: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApplySalesEventResponse {
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JournalLineInput {
    pub account_code: String,
    pub debit_minor: i64,
    pub credit_minor: i64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PostJournalRequest {
    /// `payroll` or `manual`.
    pub source_type: String,
    /// Internal UUID of the source document (payroll run id).
    pub source_id: String,
    pub currency: String,
    pub memo: Option<String>,
    pub lines: Vec<JournalLineInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JournalEntryDto {
    pub id: String,
    pub memo: String,
    pub source_type: String,
    pub source_id: String,
    pub currency: String,
    pub lines: Vec<JournalLineInput>,
}
