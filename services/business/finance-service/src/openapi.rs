//! OpenAPI 3.1 document for the CompanyOS Finance API (`/api/v1/finance/...`).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{
    credit_notes, customers, events, expenses, invoices, journals, payments, recurring, reports,
    webhooks,
};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        customers::list_customers,
        customers::get_customer,
        invoices::list_invoices,
        invoices::create_invoice,
        invoices::get_invoice,
        invoices::update_invoice,
        invoices::issue_invoice,
        invoices::send_invoice,
        invoices::void_invoice,
        invoices::create_from_quote,
        payments::list_payments,
        payments::record_payment,
        payments::allocate_payment,
        credit_notes::create_credit_note,
        expenses::list_expenses,
        expenses::submit_expense,
        expenses::decide_expense,
        reports::report_summary,
        webhooks::stripe_webhook,
        recurring::create_recurring,
        recurring::run_due,
        events::apply_sales_event_handler,
        journals::post_journal_handler,
    ),
    components(schemas(
        FinanceCustomerDto,
        FinanceCustomerListResponse,
        InvoiceLineDto,
        InvoiceDto,
        InvoiceListResponse,
        InvoiceLineInput,
        CreateInvoiceRequest,
        UpdateInvoiceRequest,
        IssueInvoiceRequest,
        QuoteLineSnapshot,
        CreateInvoiceFromQuoteRequest,
        PaymentDto,
        PaymentListResponse,
        RecordPaymentRequest,
        AllocatePaymentRequest,
        CreditNoteDto,
        CreditNoteListResponse,
        CreateCreditNoteRequest,
        ExpenseDto,
        ExpenseListResponse,
        SubmitExpenseRequest,
        DecideExpenseRequest,
        ReportSummaryDto,
        AgeingBucket,
        CategoryAmount,
        CashFlowPoint,
        StripeWebhookFixture,
        StripeWebhookData,
        StripePaymentObject,
        WebhookAck,
        RecurringInvoiceDto,
        RecurringListResponse,
        CreateRecurringRequest,
        RunDueResponse,
        ApplySalesEventRequest,
        ApplySalesEventResponse,
        ListQuery,
        JournalLineInput,
        PostJournalRequest,
        JournalEntryDto,
    )),
    tags(
        (name = "finance-customers", description = "Projected finance customers"),
        (name = "finance-invoices", description = "Invoices: draft, issue, send, void"),
        (name = "finance-payments", description = "Payments and allocations"),
        (name = "finance-credit-notes", description = "Credit notes"),
        (name = "finance-expenses", description = "Expenses and approvals"),
        (name = "finance-reports", description = "Finance summary reports"),
        (name = "finance-webhooks", description = "Provider webhooks"),
        (name = "finance-recurring", description = "Recurring invoice templates"),
        (name = "finance-events", description = "In-process sales event apply"),
        (name = "finance-journals", description = "Balanced journal posting (payroll)"),
    ),
    info(
        title = "CompanyOS Finance API",
        version = "0.1.0",
        description = "Phase 1.5–2.3 — Finance: invoices, payments, expenses, payroll journals."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/finance/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

/// Write the OpenAPI document as pretty JSON (used by `examples/export_openapi.rs`).
pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
