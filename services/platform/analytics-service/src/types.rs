use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconcileResponse {
    pub mirror_count: i64,
    pub expected_count: i64,
    pub matched: bool,
}
