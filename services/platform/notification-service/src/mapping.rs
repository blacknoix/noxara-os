//! Event → permission mapping and ingest helpers.

use companyos_authz::PermissionId;
use companyos_events::{Context, EventEnvelope};

/// Map event context+aggregate to the permission a recipient must hold.
pub fn required_permission_for_event(envelope: &EventEnvelope) -> Option<PermissionId> {
    let key = format!("{}.{}", envelope.context.as_str(), envelope.aggregate);
    let perm = match (envelope.context, envelope.aggregate.as_str()) {
        (Context::Sales, "customer") => "sales.customer.read",
        (Context::Sales, "deal") => "sales.deal.read",
        (Context::Sales, "lead") => "sales.lead.read",
        (Context::Sales, "quote") => "sales.quote.read",
        (Context::Finance, "invoice") => "finance.invoice.read",
        (Context::Finance, "expense") => "finance.expense.read",
        (Context::Operations, "task") => "operations.task.read",
        (Context::Operations, "project") => "operations.project.read",
        (Context::Operations, "approval") => "operations.approval.read",
        _ => {
            tracing::debug!(%key, "no permission mapping for event; skip notify");
            return None;
        }
    };
    Some(PermissionId::from(perm))
}

/// Build in-app title/body from an envelope (template override later).
pub fn render_in_app(envelope: &EventEnvelope) -> (String, String, Option<String>) {
    let title = format!(
        "{}.{} {}",
        envelope.context.as_str(),
        envelope.aggregate,
        envelope.event_type
    );
    let body = envelope
        .payload
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| title.clone());
    let href = envelope
        .payload
        .get("href")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (title, body, href)
}

pub fn resource_refs(envelope: &EventEnvelope) -> (Option<String>, Option<String>) {
    let resource_type = Some(envelope.aggregate.clone());
    let resource_id = envelope
        .payload
        .get(format!("{}_id", envelope.aggregate))
        .or_else(|| envelope.payload.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (resource_type, resource_id)
}
