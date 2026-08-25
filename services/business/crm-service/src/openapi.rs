//! OpenAPI 3.1 document for the CompanyOS CRM / Sales API (`/api/v1/sales/...`).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{
    activities, contacts, customers, deals, imports, leads, pipelines, products, quotes, reports,
};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        pipelines::list_pipelines,
        pipelines::default_board,
        customers::list_customers,
        customers::create_customer,
        customers::get_customer,
        customers::update_customer,
        customers::delete_customer,
        contacts::list_contacts,
        contacts::create_contact,
        contacts::update_contact,
        leads::list_leads,
        leads::create_lead,
        leads::get_lead,
        leads::update_lead,
        leads::qualify_lead,
        leads::disqualify_lead,
        leads::convert_lead,
        deals::list_deals,
        deals::create_deal,
        deals::get_deal,
        deals::update_deal,
        deals::win_deal,
        deals::lose_deal,
        activities::list_activities,
        activities::create_activity,
        products::list_products,
        products::create_product,
        products::update_product,
        quotes::list_quotes,
        quotes::create_quote,
        quotes::get_quote,
        quotes::update_quote,
        quotes::send_quote,
        quotes::accept_quote,
        quotes::reject_quote,
        quotes::invoice_action,
        imports::preview_customers,
        imports::confirm_customers,
        reports::report_summary,
    ),
    components(schemas(
        PipelineDto,
        PipelineListResponse,
        StageDto,
        BoardStage,
        BoardResponse,
        CustomerDto,
        CustomerListResponse,
        CreateCustomerRequest,
        UpdateCustomerRequest,
        CreateCustomerResponse,
        ContactDto,
        ContactListResponse,
        CreateContactRequest,
        UpdateContactRequest,
        LeadDto,
        LeadListResponse,
        CreateLeadRequest,
        UpdateLeadRequest,
        DisqualifyLeadRequest,
        ConvertLeadRequest,
        ConvertLeadResponse,
        DuplicateMatch,
        DuplicateCheckResponse,
        DealDto,
        DealListResponse,
        CreateDealRequest,
        UpdateDealRequest,
        WinDealRequest,
        LoseDealRequest,
        ActivityDto,
        ActivityListResponse,
        CreateActivityRequest,
        ProductDto,
        ProductListResponse,
        CreateProductRequest,
        UpdateProductRequest,
        QuoteLineDto,
        QuoteDto,
        QuoteListResponse,
        CreateQuoteLineRequest,
        CreateQuoteRequest,
        UpdateQuoteRequest,
        RejectQuoteRequest,
        InvoiceActionResponse,
        ImportPreviewRequest,
        ImportRowPreview,
        ImportPreviewResponse,
        ImportRowInput,
        ImportConfirmRequest,
        ImportConfirmResponse,
        StageSummary,
        WinRateSummary,
        ActivityVolumeItem,
        WeightedForecast,
        ReportSummaryResponse,
        MessageResponse,
    )),
    tags(
        (name = "sales-pipelines", description = "Pipelines, stages, and the kanban board"),
        (name = "sales-customers", description = "Customer accounts"),
        (name = "sales-contacts", description = "Contacts on customer accounts"),
        (name = "sales-leads", description = "Lead capture, qualification, and conversion"),
        (name = "sales-deals", description = "Deals / opportunities"),
        (name = "sales-activities", description = "Calls, meetings, emails, and notes"),
        (name = "sales-products", description = "Quotable product catalogue"),
        (name = "sales-quotes", description = "Quote drafting, versioning, and acceptance"),
        (name = "sales-imports", description = "CSV customer import"),
        (name = "sales-reports", description = "Pipeline / win-rate / forecast reports"),
    ),
    info(
        title = "CompanyOS CRM API",
        version = "0.1.0",
        description = "Phase 1.4 — Sales bounded context: customers, leads, deals, quotes, and reporting."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/sales/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

/// Write the OpenAPI document as pretty JSON (used by `examples/export_openapi.rs`).
pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
