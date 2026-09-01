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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_rate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_group_id: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
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
    /// Optional tax rate public id (`txr_…`) — snapshotted at issue.
    #[serde(default)]
    pub tax_rate_id: Option<String>,
    /// Optional tax group public id (`txg_…`) — resolved as-of issue date.
    #[serde(default)]
    pub tax_group_id: Option<String>,
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
    /// Optional finance entity (`ent_…`); defaults to org default entity.
    #[serde(default)]
    pub entity_id: Option<String>,
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
    /// Filter invoices by finance entity public id (`ent_…`).
    pub entity_id: Option<String>,
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
    /// Empty / omitted for manual → generated UUID.
    #[serde(default)]
    pub source_id: String,
    pub currency: String,
    pub memo: Option<String>,
    pub lines: Vec<JournalLineInput>,
    /// Document date `YYYY-MM-DD`; defaults to today.
    pub entry_date: Option<String>,
    /// Public id of the journal being reversed (`jrn_…`).
    pub reverses_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JournalEntryDto {
    pub id: String,
    pub memo: String,
    pub source_type: String,
    pub source_id: String,
    pub currency: String,
    pub lines: Vec<JournalLineInput>,
    pub entry_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_id: Option<String>,
    /// Finance entity public id (`ent_…`) when the entry is entity-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JournalListResponse {
    pub items: Vec<JournalEntryDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct JournalListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub source_type: Option<String>,
    pub period_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Chart of accounts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LedgerAccountDto {
    pub id: String,
    pub code: String,
    pub name: String,
    pub account_type: String,
    pub normal_balance: String,
    pub parent_id: Option<String>,
    pub is_active: bool,
    pub description: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LedgerAccountNode {
    pub account: LedgerAccountDto,
    #[schema(no_recursion)]
    pub children: Vec<LedgerAccountNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LedgerAccountTreeResponse {
    pub roots: Vec<LedgerAccountNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateLedgerAccountRequest {
    pub code: String,
    pub name: String,
    /// `asset` | `liability` | `equity` | `revenue` | `income` | `expense`
    pub account_type: String,
    pub parent_id: Option<String>,
    pub description: Option<String>,
    pub normal_balance: Option<String>,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateLedgerAccountRequest {
    pub name: Option<String>,
    pub parent_id: Option<String>,
    pub is_active: Option<bool>,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
}

// ---------------------------------------------------------------------------
// Fiscal periods
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FiscalPeriodDto {
    pub id: String,
    pub code: String,
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub status: String,
    #[schema(value_type = Object)]
    pub checklist: serde_json::Value,
    pub closed_at: Option<String>,
    pub reopened_at: Option<String>,
    pub reopen_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FiscalPeriodListResponse {
    pub items: Vec<FiscalPeriodDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateFiscalPeriodRequest {
    pub code: String,
    pub name: String,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClosePeriodRequest {
    /// Optional checklist override applied before close.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub checklist: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReopenPeriodRequest {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateChecklistRequest {
    #[schema(value_type = Object)]
    pub checklist: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TrialBalanceRow {
    pub account_code: String,
    pub account_name: String,
    pub account_type: String,
    pub debit_minor: i64,
    pub credit_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TrialBalanceResponse {
    pub currency: String,
    pub period_id: Option<String>,
    pub rows: Vec<TrialBalanceRow>,
    pub total_debit_minor: i64,
    pub total_credit_minor: i64,
    pub balanced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportLine {
    pub account_code: String,
    pub account_name: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProfitAndLossResponse {
    pub currency: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub period_id: Option<String>,
    pub revenue: Vec<ReportLine>,
    pub expenses: Vec<ReportLine>,
    pub revenue_total_minor: i64,
    pub expense_total_minor: i64,
    pub net_income_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BalanceSheetResponse {
    pub currency: String,
    pub as_of: String,
    pub period_id: Option<String>,
    pub assets: Vec<ReportLine>,
    pub liabilities: Vec<ReportLine>,
    pub equity: Vec<ReportLine>,
    pub assets_total_minor: i64,
    pub liabilities_total_minor: i64,
    pub equity_total_minor: i64,
}

// ---------------------------------------------------------------------------
// Bank
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BankAccountDto {
    pub id: String,
    pub name: String,
    pub currency: String,
    pub ledger_account_id: String,
    pub account_number_mask: Option<String>,
    pub institution: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateBankAccountRequest {
    pub name: String,
    pub currency: String,
    /// Ledger account public id (`acc_…`) or code (e.g. `1000`).
    pub ledger_account_id: String,
    pub account_number_mask: Option<String>,
    pub institution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BankStatementDto {
    pub id: String,
    pub bank_account_id: String,
    pub statement_date: String,
    pub currency: String,
    pub opening_minor: i64,
    pub closing_minor: i64,
    pub source: String,
    pub line_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportStatementRequest {
    pub csv: String,
    pub statement_date: Option<String>,
    pub opening_minor: Option<i64>,
    pub closing_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportStatementResponse {
    pub statement: BankStatementDto,
    pub lines_imported: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatementLineDto {
    pub id: String,
    pub statement_id: String,
    pub line_no: i32,
    pub txn_date: String,
    pub amount_minor: i64,
    pub currency: String,
    pub reference: Option<String>,
    pub description: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconcileRequest {
    /// Optional: restrict auto-match to these statement line UUIDs / public keys.
    pub line_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconcileResponse {
    pub matched: i32,
    pub unmatched: i32,
    pub match_rate: f64,
    pub reconciliations: Vec<ReconciliationDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconciliationDto {
    pub id: String,
    pub bank_account_id: String,
    pub statement_line_id: String,
    pub match_kind: String,
    pub matched_payment_id: Option<String>,
    pub amount_minor: i64,
    pub auto_matched: bool,
}

// ---------------------------------------------------------------------------
// Expense policy / cards / reimbursements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CategoryLimitDto {
    pub category_code: String,
    pub max_amount_minor: i64,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CategoryLimitInput {
    pub category_code: String,
    pub max_amount_minor: i64,
    #[serde(default = "default_base_currency")]
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExpensePolicyDto {
    pub id: String,
    pub name: String,
    pub is_active: bool,
    pub require_receipt_over_minor: i64,
    pub auto_approve_under_minor: i64,
    pub over_limit_action: String,
    pub mileage_unit: String,
    pub mileage_rate_minor: i64,
    pub per_diem_minor: i64,
    pub category_limits: Vec<CategoryLimitDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertExpensePolicyRequest {
    pub name: Option<String>,
    pub require_receipt_over_minor: Option<i64>,
    pub auto_approve_under_minor: Option<i64>,
    pub over_limit_action: Option<String>,
    pub mileage_unit: Option<String>,
    pub mileage_rate_minor: Option<i64>,
    pub per_diem_minor: Option<i64>,
    pub category_limits: Option<Vec<CategoryLimitInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MileageCalculateRequest {
    pub miles_or_km: f64,
    pub currency: Option<String>,
    pub description: Option<String>,
    pub incurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MileageCalculateResponse {
    pub amount_minor: i64,
    pub currency: String,
    pub rate_minor: i64,
    pub miles_or_km: f64,
    pub expense: ExpenseDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PerDiemRequest {
    pub days: i32,
    pub currency: Option<String>,
    pub description: Option<String>,
    pub incurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PerDiemResponse {
    pub amount_minor: i64,
    pub currency: String,
    pub per_diem_minor: i64,
    pub days: i32,
    pub expense: ExpenseDto,
}

// ---------------------------------------------------------------------------
// Vendor bills (Phase 2.5 — procure-to-pay support for inventory-service).
// AP is booked at goods-receipt time; a bill is a record of that liability.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VendorBillDto {
    pub id: String,
    pub supplier_ref: String,
    pub source_type: String,
    pub source_id: Option<String>,
    pub currency: String,
    pub amount_minor: i64,
    pub amount_paid_minor: i64,
    pub status: String,
    pub memo: Option<String>,
    pub payment_journal_public_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VendorBillListResponse {
    pub items: Vec<VendorBillDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateVendorBillRequest {
    pub supplier_ref: String,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub currency: String,
    pub amount_minor: i64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PayVendorBillRequest {
    pub amount_minor: Option<i64>,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CardTransactionDto {
    pub id: String,
    pub txn_date: String,
    pub amount_minor: i64,
    pub currency: String,
    pub merchant: Option<String>,
    pub reference: Option<String>,
    pub description: Option<String>,
    pub status: String,
    pub matched_expense_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportCardCsvRequest {
    pub csv: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportCardResponse {
    pub imported: i32,
    pub items: Vec<CardTransactionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MatchCardsResponse {
    pub matched: i32,
    pub unmatched: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReimbursementBatchDto {
    pub id: String,
    pub status: String,
    pub currency: String,
    pub total_minor: i64,
    pub expense_ids: Vec<String>,
    pub approval_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateReimbursementBatchRequest {
    pub expense_ids: Vec<String>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DecideReimbursementRequest {
    pub approve: bool,
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Phase 3.5 — Tax / dunning / entities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaxGroupDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaxGroupListResponse {
    pub items: Vec<TaxGroupDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTaxGroupRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaxRateDto {
    pub id: String,
    pub name: String,
    pub rate_bps: i64,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub tax_group_id: Option<String>,
    pub supersedes_id: Option<String>,
    pub component_name: Option<String>,
    pub is_component: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaxRateListResponse {
    pub items: Vec<TaxRateDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTaxRateRequest {
    pub name: String,
    pub rate_bps: i64,
    /// Required — start of validity window (YYYY-MM-DD).
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub tax_group_id: Option<String>,
    /// Previous rate version public id (`txr_…`) when creating a successor.
    pub supersedes_id: Option<String>,
    pub component_name: Option<String>,
    #[serde(default)]
    pub is_component: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaxResolveQuery {
    pub group_id: Option<String>,
    pub rate_id: Option<String>,
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaxResolveResponse {
    pub rate_bps: i64,
    pub tax_rate_id: Option<String>,
    pub tax_group_id: Option<String>,
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DunningStepDto {
    pub offset_days: i32,
    pub channel: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DunningProfileDto {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub steps: Vec<DunningStepDto>,
    pub version: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DunningProfileListResponse {
    pub items: Vec<DunningProfileDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDunningProfileRequest {
    pub name: String,
    pub steps: Vec<DunningStepDto>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateDunningProfileRequest {
    pub name: Option<String>,
    pub steps: Option<Vec<DunningStepDto>>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetCustomerDunningProfileRequest {
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DunningScheduleQuery {
    pub invoice_id: Option<String>,
    pub customer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DunningScheduleResponse {
    pub profile_id: String,
    pub schedule_offsets_days: Vec<i32>,
    pub steps: Vec<DunningStepDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FinanceEntityDto {
    pub id: String,
    pub name: String,
    pub code: String,
    pub currency: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FinanceEntityListResponse {
    pub items: Vec<FinanceEntityDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateFinanceEntityRequest {
    pub name: String,
    pub code: String,
    #[serde(default = "default_base_currency")]
    pub currency: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateFinanceEntityRequest {
    pub name: Option<String>,
    pub code: Option<String>,
    pub currency: Option<String>,
    pub is_default: Option<bool>,
}
