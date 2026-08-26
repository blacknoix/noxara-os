//! Phase 1.6 dashboard BFF — widget descriptors + live CRM/Finance/Operations aggregates.
//!
//! CRM pipeline and Finance revenue/expenses/cash/receivables are fetched from
//! their services when reachable. Staleness is labeled via `as_of` (no Redis
//! cache yet — Redis is in the stack but unused for dashboard).

use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use companyos_authz::{is_allowed, perms, Role};
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use crate::auth::extract::AuthUser;
use crate::state::AppState;
use crate::workspace::load_principal;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardResponse {
    /// RFC3339 timestamp when this snapshot was produced.
    pub as_of: String,
    /// Requested period window (e.g. `30d`); accepted but does not invent metrics.
    pub period: String,
    /// Derived from the caller's primary role.
    pub role_layout: String,
    pub widgets: Vec<DashboardWidget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardWidget {
    pub id: String,
    pub title: String,
    /// checklist | stat | module_empty | feed | pipeline
    pub kind: String,
    /// ready | empty | unavailable | loading
    pub status: String,
    /// module_not_enabled | coming_in_later_phase | no_data | crm_unreachable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// Always false for honest empties in Phase 1.4 (pattern present for later).
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_label: Option<String>,
    /// Widget-specific JSON body (checklist items, empty lists, module stubs).
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct DashboardQuery {
    /// Period window hint (default `30d`). Metrics are not invented from this.
    #[serde(default = "default_period")]
    pub period: String,
}

fn default_period() -> String {
    "30d".into()
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/dashboard", get(get_dashboard))
}

/// GET /api/v1/dashboard — first-paint widget descriptors for the shell home.
#[utoipa::path(
    get,
    path = "/api/v1/dashboard",
    params(DashboardQuery),
    responses(
        (status = 200, description = "Dashboard widget snapshot", body = DashboardResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    ),
    tag = "dashboard"
)]
pub async fn get_dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Query(q): Query<DashboardQuery>,
) -> Result<Json<DashboardResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    // Use full principal (role defaults + role_permission statements) so explicit
    // deny / custom roles are honoured — same path as capabilities.
    let (principal, _, _) = load_principal(
        &state.pool,
        user.ctx.org_id,
        user.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    if !is_allowed(&principal, &perms::workspace_dashboard_read()) {
        state
            .perm_cache
            .invalidate_membership(&user.membership_id.to_string());
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            format!(
                "missing permission {}",
                perms::workspace_dashboard_read().as_str()
            ),
        ));
    }

    let period = if q.period.trim().is_empty() {
        default_period()
    } else {
        q.period.trim().to_string()
    };
    let role_layout = derive_role_layout(&user.roles);
    let range_label = period_range_label(&period);

    // Single cheap aggregator: active membership count for this tenant only.
    let member_count = count_active_members(&state, &user).await?;

    let mut widgets = build_widgets(role_layout, member_count, range_label.as_deref());
    let pipeline_widget = build_pipeline_widget(&headers, range_label.as_deref()).await;
    replace_widget(&mut widgets, pipeline_widget);
    let finance_widgets = build_finance_widgets(&headers, range_label.as_deref()).await;
    for w in finance_widgets {
        replace_widget(&mut widgets, w);
    }
    let ops_widgets = build_operations_widgets(&headers, range_label.as_deref()).await;
    for w in ops_widgets {
        replace_widget(&mut widgets, w);
    }

    tracing::info!(
        request_id = %user.ctx.request_id,
        org_id = %user.ctx.org_id,
        role_layout,
        period = %period,
        widget_count = widgets.len(),
        "dashboard snapshot"
    );

    Ok(Json(DashboardResponse {
        as_of: Utc::now().to_rfc3339(),
        period,
        role_layout: role_layout.to_string(),
        widgets,
    }))
}

fn derive_role_layout(roles: &[String]) -> &'static str {
    let parsed: Vec<Role> = roles.iter().filter_map(|r| Role::parse(r)).collect();
    // Priority for primary layout when multiple roles are present.
    const ORDER: &[(Role, &str)] = &[
        (Role::Owner, "owner"),
        (Role::Admin, "admin"),
        (Role::Finance, "finance"),
        (Role::Sales, "sales"),
        (Role::Manager, "ops"),
        (Role::Member, "member"),
        (Role::ReadOnly, "read_only"),
    ];
    for (role, layout) in ORDER {
        if parsed.contains(role) {
            return layout;
        }
    }
    "member"
}

fn period_range_label(period: &str) -> Option<String> {
    match period {
        "7d" => Some("Last 7 days".into()),
        "30d" => Some("Last 30 days".into()),
        "90d" => Some("Last 90 days".into()),
        "ytd" => Some("Year to date".into()),
        other => Some(format!("Period: {other}")),
    }
}

async fn count_active_members(state: &AppState, user: &AuthUser) -> Result<i64, AppError> {
    let request_id = user.ctx.request_id.clone();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM membership
        WHERE org_id = $1 AND status = 'active'
        "#,
    )
    .bind(user.ctx.org_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(count)
}

fn build_widgets(
    _role_layout: &str,
    member_count: i64,
    range_label: Option<&str>,
) -> Vec<DashboardWidget> {
    // Honest empties for every role layout. Later phases can reorder/filter by layout.
    vec![
        DashboardWidget {
            id: "setup_checklist".into(),
            title: "Workspace setup".into(),
            kind: "checklist".into(),
            status: "ready".into(),
            reason_code: None,
            stale: false,
            range_label: None,
            payload: json!({
                "items": [
                    {
                        "id": "org_exists",
                        "label": "Organization created",
                        "done": true
                    },
                    {
                        "id": "members",
                        "label": "Invite your team",
                        "done": member_count >= 2,
                        "member_count": member_count
                    }
                ]
            }),
        },
        empty_widget(
            "my_work",
            "My work",
            "module_empty",
            "empty",
            "coming_in_later_phase",
            None,
            json!({ "items": [] }),
        ),
        empty_widget(
            "inbox",
            "Inbox",
            "module_empty",
            "empty",
            "no_data",
            None,
            json!({ "items": [] }),
        ),
        empty_widget(
            "approvals",
            "Approvals",
            "module_empty",
            "empty",
            "coming_in_later_phase",
            None,
            json!({ "items": [] }),
        ),
        unavailable_pipeline_placeholder(range_label),
        empty_widget(
            "revenue",
            "Revenue",
            "stat",
            "unavailable",
            "module_not_enabled",
            range_label,
            json!({ "module": "finance", "message": "Revenue metrics are not available yet" }),
        ),
        empty_widget(
            "expenses",
            "Expenses",
            "stat",
            "unavailable",
            "module_not_enabled",
            range_label,
            json!({ "module": "finance", "message": "Expense metrics are not available yet" }),
        ),
        empty_widget(
            "cash",
            "Cash",
            "stat",
            "unavailable",
            "module_not_enabled",
            range_label,
            json!({ "module": "finance", "message": "Cash metrics are not available yet" }),
        ),
        empty_widget(
            "receivables",
            "Receivables",
            "stat",
            "unavailable",
            "module_not_enabled",
            range_label,
            json!({ "module": "finance", "message": "Receivables metrics are not available yet" }),
        ),
        empty_widget(
            "team_activity",
            "Team activity",
            "feed",
            "empty",
            "no_data",
            range_label,
            json!({ "items": [] }),
        ),
    ]
}

fn unavailable_pipeline_placeholder(range_label: Option<&str>) -> DashboardWidget {
    empty_widget(
        "pipeline",
        "Pipeline",
        "module_empty",
        "unavailable",
        "crm_unreachable",
        range_label,
        json!({ "module": "sales", "message": "CRM unavailable" }),
    )
}

fn replace_widget(widgets: &mut Vec<DashboardWidget>, widget: DashboardWidget) {
    if let Some(idx) = widgets.iter().position(|w| w.id == widget.id) {
        widgets[idx] = widget;
    } else {
        widgets.push(widget);
    }
}

fn unavailable_finance_stat(id: &str, title: &str, range_label: Option<&str>) -> DashboardWidget {
    empty_widget(
        id,
        title,
        "stat",
        "unavailable",
        "finance_unreachable",
        range_label,
        json!({ "module": "finance", "message": format!("{title} unavailable") }),
    )
}

async fn build_finance_widgets(
    headers: &HeaderMap,
    range_label: Option<&str>,
) -> Vec<DashboardWidget> {
    let finance_url =
        std::env::var("FINANCE_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8083".into());
    let url = format!(
        "{}/api/v1/finance/reports/summary",
        finance_url.trim_end_matches('/')
    );

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return vec![
                unavailable_finance_stat("revenue", "Revenue", range_label),
                unavailable_finance_stat("expenses", "Expenses", range_label),
                unavailable_finance_stat("cash", "Cash", range_label),
                unavailable_finance_stat("receivables", "Receivables", range_label),
            ];
        }
    };

    let mut req = client.get(&url);
    if let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        req = req.header(axum::http::header::AUTHORIZATION, auth);
    }
    for name in [
        "x-companyos-dev-org-id",
        "x-companyos-dev-user-id",
        "x-companyos-org-id",
        "x-companyos-user-id",
        "x-companyos-session-id",
        "x-request-id",
    ] {
        if let Some(val) = headers.get(name).and_then(|v| v.to_str().ok()) {
            req = req.header(name, val);
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => {
            return vec![
                unavailable_finance_stat("revenue", "Revenue", range_label),
                unavailable_finance_stat("expenses", "Expenses", range_label),
                unavailable_finance_stat("cash", "Cash", range_label),
                unavailable_finance_stat("receivables", "Receivables", range_label),
            ];
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        let empty = |id: &str, title: &str| {
            empty_widget(
                id,
                title,
                "stat",
                "empty",
                "no_data",
                range_label,
                json!({ "amount_minor": 0, "currency": "USD", "message": "No finance report access" }),
            )
        };
        return vec![
            empty("revenue", "Revenue"),
            empty("expenses", "Expenses"),
            empty("cash", "Cash"),
            empty("receivables", "Receivables"),
        ];
    }
    if !status.is_success() {
        return vec![
            unavailable_finance_stat("revenue", "Revenue", range_label),
            unavailable_finance_stat("expenses", "Expenses", range_label),
            unavailable_finance_stat("cash", "Cash", range_label),
            unavailable_finance_stat("receivables", "Receivables", range_label),
        ];
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => {
            return vec![
                unavailable_finance_stat("revenue", "Revenue", range_label),
                unavailable_finance_stat("expenses", "Expenses", range_label),
                unavailable_finance_stat("cash", "Cash", range_label),
                unavailable_finance_stat("receivables", "Receivables", range_label),
            ];
        }
    };

    let currency = body
        .get("currency")
        .and_then(|c| c.as_str())
        .unwrap_or("USD");
    let as_of = body
        .get("as_of")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let stat = |id: &str, title: &str, key: &str| {
        let amount = body.get(key).and_then(|v| v.as_i64()).unwrap_or(0);
        DashboardWidget {
            id: id.into(),
            title: title.into(),
            kind: "stat".into(),
            status: if amount == 0 { "empty" } else { "ready" }.into(),
            reason_code: None,
            stale: false,
            range_label: range_label.map(|s| s.to_string()),
            payload: json!({
                "amount_minor": amount,
                "currency": currency,
                "as_of": as_of,
            }),
        }
    };

    vec![
        stat("revenue", "Revenue", "revenue_minor"),
        stat("expenses", "Expenses", "expenses_minor"),
        stat("cash", "Cash", "cash_minor"),
        stat("receivables", "Receivables", "receivables_minor"),
    ]
}

async fn build_pipeline_widget(headers: &HeaderMap, range_label: Option<&str>) -> DashboardWidget {
    let crm_url =
        std::env::var("CRM_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8082".into());
    let url = format!(
        "{}/api/v1/sales/reports/summary",
        crm_url.trim_end_matches('/')
    );

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return unavailable_pipeline_placeholder(range_label),
    };

    let mut req = client.get(&url);
    if let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        req = req.header(axum::http::header::AUTHORIZATION, auth);
    }
    for name in [
        "x-companyos-dev-org-id",
        "x-companyos-dev-user-id",
        "x-companyos-org-id",
        "x-companyos-user-id",
        "x-companyos-session-id",
        "x-request-id",
    ] {
        if let Some(val) = headers.get(name).and_then(|v| v.to_str().ok()) {
            req = req.header(name, val);
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => return unavailable_pipeline_placeholder(range_label),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        return empty_widget(
            "pipeline",
            "Pipeline",
            "module_empty",
            "empty",
            "no_data",
            range_label,
            json!({ "open_deal_count": 0, "message": "No sales report access" }),
        );
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return empty_widget(
            "pipeline",
            "Pipeline",
            "module_empty",
            "empty",
            "no_data",
            range_label,
            json!({ "open_deal_count": 0 }),
        );
    }
    if !status.is_success() {
        return unavailable_pipeline_placeholder(range_label);
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return unavailable_pipeline_placeholder(range_label),
    };

    let pipeline_by_stage = body.get("pipeline_by_stage").cloned().unwrap_or(json!([]));
    let open_deal_count = pipeline_by_stage
        .as_array()
        .map(|stages| {
            stages
                .iter()
                .filter_map(|s| s.get("open_deal_count").and_then(|c| c.as_i64()))
                .sum::<i64>()
        })
        .unwrap_or(0);

    let status = if open_deal_count == 0 {
        "empty"
    } else {
        "ready"
    };

    DashboardWidget {
        id: "pipeline".into(),
        title: "Pipeline".into(),
        kind: "pipeline".into(),
        status: status.into(),
        reason_code: None,
        stale: false,
        range_label: range_label.map(|s| s.to_string()),
        payload: json!({
            "pipeline_by_stage": pipeline_by_stage,
            "win_rate": body.get("win_rate").cloned().unwrap_or(json!({})),
            "weighted_forecast": body.get("weighted_forecast").cloned().unwrap_or(json!({})),
            "activity_volume": body.get("activity_volume").cloned().unwrap_or(json!([])),
            "open_deal_count": open_deal_count,
        }),
    }
}

async fn build_operations_widgets(
    headers: &HeaderMap,
    range_label: Option<&str>,
) -> Vec<DashboardWidget> {
    let project_url =
        std::env::var("PROJECT_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8084".into());
    let url = format!(
        "{}/api/v1/operations/summary",
        project_url.trim_end_matches('/')
    );

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return vec![
                empty_widget(
                    "my_work",
                    "My work",
                    "stat",
                    "unavailable",
                    "operations_unreachable",
                    range_label,
                    json!({ "module": "operations", "message": "Projects unavailable" }),
                ),
                empty_widget(
                    "tasks",
                    "Tasks",
                    "stat",
                    "unavailable",
                    "operations_unreachable",
                    range_label,
                    json!({ "module": "operations", "message": "Tasks unavailable" }),
                ),
            ];
        }
    };

    let mut req = client.get(&url);
    if let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        req = req.header(axum::http::header::AUTHORIZATION, auth);
    }
    for name in [
        "x-companyos-dev-org-id",
        "x-companyos-dev-user-id",
        "x-companyos-org-id",
        "x-companyos-user-id",
        "x-companyos-session-id",
        "x-request-id",
    ] {
        if let Some(val) = headers.get(name).and_then(|v| v.to_str().ok()) {
            req = req.header(name, val);
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => {
            return vec![
                empty_widget(
                    "my_work",
                    "My work",
                    "stat",
                    "unavailable",
                    "operations_unreachable",
                    range_label,
                    json!({ "module": "operations", "message": "Projects unavailable" }),
                ),
                empty_widget(
                    "tasks",
                    "Tasks",
                    "stat",
                    "unavailable",
                    "operations_unreachable",
                    range_label,
                    json!({ "module": "operations", "message": "Tasks unavailable" }),
                ),
            ];
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        return vec![
            empty_widget(
                "my_work",
                "My work",
                "stat",
                "empty",
                "no_data",
                range_label,
                json!({ "count": 0, "message": "No task access" }),
            ),
            empty_widget(
                "tasks",
                "Tasks",
                "stat",
                "empty",
                "no_data",
                range_label,
                json!({ "count": 0, "message": "No task access" }),
            ),
        ];
    }
    if !status.is_success() {
        return vec![
            empty_widget(
                "my_work",
                "My work",
                "stat",
                "unavailable",
                "operations_unreachable",
                range_label,
                json!({ "module": "operations", "message": "Projects unavailable" }),
            ),
            empty_widget(
                "tasks",
                "Tasks",
                "stat",
                "unavailable",
                "operations_unreachable",
                range_label,
                json!({ "module": "operations", "message": "Tasks unavailable" }),
            ),
        ];
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => {
            return vec![
                empty_widget(
                    "my_work",
                    "My work",
                    "stat",
                    "unavailable",
                    "operations_unreachable",
                    range_label,
                    json!({ "module": "operations", "message": "Projects unavailable" }),
                ),
                empty_widget(
                    "tasks",
                    "Tasks",
                    "stat",
                    "unavailable",
                    "operations_unreachable",
                    range_label,
                    json!({ "module": "operations", "message": "Tasks unavailable" }),
                ),
            ];
        }
    };

    let my_open = body
        .get("my_open_tasks")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let open_tasks = body.get("open_tasks").and_then(|v| v.as_i64()).unwrap_or(0);
    let overdue = body.get("overdue").and_then(|v| v.as_i64()).unwrap_or(0);

    vec![
        DashboardWidget {
            id: "my_work".into(),
            title: "My work".into(),
            kind: "stat".into(),
            status: if my_open == 0 { "empty" } else { "ready" }.into(),
            reason_code: None,
            stale: false,
            range_label: range_label.map(|s| s.to_string()),
            payload: json!({
                "count": my_open,
                "overdue": overdue,
                "href": "/my-work",
            }),
        },
        DashboardWidget {
            id: "tasks".into(),
            title: "Tasks".into(),
            kind: "stat".into(),
            status: if open_tasks == 0 { "empty" } else { "ready" }.into(),
            reason_code: None,
            stale: false,
            range_label: range_label.map(|s| s.to_string()),
            payload: json!({
                "count": open_tasks,
                "overdue": overdue,
                "href": "/ops/tasks",
            }),
        },
    ]
}

fn empty_widget(
    id: &str,
    title: &str,
    kind: &str,
    status: &str,
    reason_code: &str,
    range_label: Option<&str>,
    payload: serde_json::Value,
) -> DashboardWidget {
    DashboardWidget {
        id: id.into(),
        title: title.into(),
        kind: kind.into(),
        status: status.into(),
        reason_code: Some(reason_code.into()),
        stale: false,
        range_label: range_label.map(|s| s.to_string()),
        payload,
    }
}
