//! HTTP client for `companyos-project`'s approvals API
//! (`/api/v1/operations/approvals`) — used to route purchase requests to a
//! human approver.
//!
//! Phase 2.5 budget checking is intentionally thin: inventory-service does
//! not have a budget-remaining view, so it always forwards `amount_minor`
//! (+ `budget_account_code` as `category`) to the approval and lets the
//! human approver see the requested amount. A later phase can add a real
//! budget-remaining lookup against finance without changing this contract.

use companyos_ids::{IdKind, PublicId};

use crate::auth::AuthCtx;

fn project_base_url() -> String {
    std::env::var("PROJECT_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8084".into())
}

pub struct RequestApprovalInput<'a> {
    pub subject_type: &'a str,
    pub subject_id: &'a str,
    pub title: String,
    pub summary: Option<String>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub category: Option<String>,
}

/// Best-effort: request an approval and return its public id on success.
/// Mirrors `hr-service`'s `request_payroll_approval` — failures here are
/// logged and swallowed rather than blocking the submit, since the caller
/// still transitions the purchase request to `pending_approval` and the
/// approval can be created out-of-band if the approvals service is briefly
/// unavailable.
pub async fn request_approval(auth: &AuthCtx, input: RequestApprovalInput<'_>) -> Option<String> {
    let url = format!(
        "{}/api/v1/operations/approvals",
        project_base_url().trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let mut req = client.post(&url).json(&serde_json::json!({
        "subject_type": input.subject_type,
        "subject_id": input.subject_id,
        "title": input.title,
        "summary": input.summary,
        "amount_minor": input.amount_minor,
        "currency": input.currency,
        "category": input.category,
    }));
    req = req
        .header(
            "x-companyos-dev-org-id",
            auth.ctx.org_id.to_public().as_str(),
        )
        .header(
            "x-companyos-dev-user-id",
            PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
        )
        .header(
            "x-companyos-on-behalf-of",
            PublicId::new(IdKind::User, auth.ctx.actor.on_behalf_of).as_str(),
        );
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "approvals request failed");
            return None;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "approvals request returned error status");
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("id").and_then(|v| v.as_str()).map(str::to_string)
}
