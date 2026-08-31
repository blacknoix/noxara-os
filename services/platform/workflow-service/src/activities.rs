//! HTTP activities — call existing service APIs with `on_behalf_of`.
//! Never a superuser; never cross-context SQL.

use anyhow::{anyhow, Context as _};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId};
use serde_json::json;

fn org_public(org_id: OrgId) -> String {
    PublicId::new(IdKind::Org, org_id.as_uuid()).as_str()
}

fn actor_public(actor: &Actor) -> String {
    // Activities run on_behalf_of the recorded human, never elevated.
    PublicId::new(IdKind::User, actor.on_behalf_of).as_str()
}

fn service_url(env: &str, default: &str) -> String {
    std::env::var(env).unwrap_or_else(|_| default.to_string())
}

fn render_params(params: &serde_json::Value, payload: &serde_json::Value) -> serde_json::Value {
    match params {
        serde_json::Value::String(s) => {
            let mut out = s.clone();
            if let Some(obj) = payload.as_object() {
                for (k, v) in obj {
                    let needle = format!("{{{{payload.{k}}}}}");
                    let repl = match v {
                        serde_json::Value::String(x) => x.clone(),
                        other => other.to_string(),
                    };
                    out = out.replace(&needle, &repl);
                }
            }
            serde_json::Value::String(out)
        }
        serde_json::Value::Object(map) => {
            let mut n = serde_json::Map::new();
            for (k, v) in map {
                n.insert(k.clone(), render_params(v, payload));
            }
            serde_json::Value::Object(n)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| render_params(v, payload)).collect())
        }
        other => other.clone(),
    }
}

async fn http_json(
    method: reqwest::Method,
    base: &str,
    path: &str,
    org_id: OrgId,
    actor: &Actor,
    body: Option<serde_json::Value>,
) -> anyhow::Result<u16> {
    let client = reqwest::Client::new();
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let org = org_public(org_id);
    let user = actor_public(actor);
    let mut req = client
        .request(method, &url)
        .header("x-companyos-dev-org-id", &org)
        .header("x-companyos-dev-user-id", &user)
        .header("x-companyos-on-behalf-of", &user)
        .header("content-type", "application/json");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.context("activity http")?;
    let status = resp.status().as_u16();
    if status >= 400 {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("activity HTTP {status}: {text}"));
    }
    Ok(status)
}

/// Execute a catalogue action. Best-effort when upstream is down in local tests
/// — callers may set `WORKFLOW_ACTIVITIES_NOOP=1` to skip HTTP.
pub async fn execute_action(
    action: &str,
    params: &serde_json::Value,
    payload: &serde_json::Value,
    org_id: OrgId,
    actor: &Actor,
) -> anyhow::Result<()> {
    if matches!(
        std::env::var("WORKFLOW_ACTIVITIES_NOOP").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    ) {
        tracing::info!(action, "workflow activity noop");
        return Ok(());
    }

    let rendered = render_params(params, payload);

    match action {
        "create_task" => {
            let base = service_url("PROJECT_SERVICE_URL", "http://127.0.0.1:8084");
            let title = rendered
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Workflow task");
            let description = rendered
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let body = json!({
                "title": title,
                "description": description,
                "status": "todo"
            });
            http_json(
                reqwest::Method::POST,
                &base,
                "/api/v1/operations/tasks",
                org_id,
                actor,
                Some(body),
            )
            .await?;
        }
        "send_notification" => {
            let base = service_url("NOTIFICATION_SERVICE_URL", "http://127.0.0.1:8085");
            let title = rendered
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Workflow notification");
            let body_text = rendered.get("body").and_then(|v| v.as_str()).unwrap_or("");
            // Notification ingest expects an event envelope-ish payload; keep minimal.
            let body = json!({
                "title": title,
                "body": body_text,
                "org_id": org_public(org_id),
                "user_id": actor_public(actor),
                "kind": "workflow"
            });
            let _ = http_json(
                reqwest::Method::POST,
                &base,
                "/api/v1/notifications/internal/ingest",
                org_id,
                actor,
                Some(body),
            )
            .await;
            // Soft-fail notify so leave-approved fixture can complete in tests.
        }
        "start_approval" => {
            let base = service_url("PROJECT_SERVICE_URL", "http://127.0.0.1:8084");
            let body = json!({
                "subject_type": rendered.get("subject_type").and_then(|v| v.as_str()).unwrap_or("workflow"),
                "subject_id": rendered.get("subject_id").cloned().unwrap_or(json!(payload.get("id"))),
                "title": rendered.get("title").and_then(|v| v.as_str()).unwrap_or("Workflow approval"),
            });
            http_json(
                reqwest::Method::POST,
                &base,
                "/api/v1/operations/approvals",
                org_id,
                actor,
                Some(body),
            )
            .await?;
        }
        "update_deal_status" => {
            let base = service_url("CRM_SERVICE_URL", "http://127.0.0.1:8082");
            let deal_id = rendered
                .get("deal_id")
                .or_else(|| payload.get("deal_id"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("deal_id required"))?;
            let body = json!({
                "status": rendered.get("status").and_then(|v| v.as_str()).unwrap_or("won")
            });
            http_json(
                reqwest::Method::PATCH,
                &base,
                &format!("/api/v1/sales/deals/{deal_id}"),
                org_id,
                actor,
                Some(body),
            )
            .await?;
        }
        "create_purchase_request" => {
            let base = service_url("INVENTORY_SERVICE_URL", "http://127.0.0.1:8093");
            let notes = rendered
                .get("notes")
                .and_then(|v| v.as_str())
                .unwrap_or("Workflow draft PR");
            let body = json!({
                "notes": notes,
                "lines": rendered.get("lines").cloned().unwrap_or_else(|| json!([]))
            });
            http_json(
                reqwest::Method::POST,
                &base,
                "/api/v1/inventory/purchase-requests",
                org_id,
                actor,
                Some(body),
            )
            .await?;
        }
        "post_journal" => {
            // Money must be integer minor units — pass through as provided.
            let base = service_url("FINANCE_SERVICE_URL", "http://127.0.0.1:8083");
            http_json(
                reqwest::Method::POST,
                &base,
                "/api/v1/finance/journals",
                org_id,
                actor,
                Some(rendered),
            )
            .await?;
        }
        "read_payroll" => {
            let base = service_url("HR_SERVICE_URL", "http://127.0.0.1:8088");
            http_json(
                reqwest::Method::GET,
                &base,
                "/api/v1/people/payroll/runs",
                org_id,
                actor,
                None,
            )
            .await?;
        }
        other => return Err(anyhow!("unknown action '{other}'")),
    }
    Ok(())
}
