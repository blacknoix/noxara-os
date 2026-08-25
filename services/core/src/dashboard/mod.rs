//! Phase 1.3 dashboard BFF — widget descriptors + honest empty payloads.
//!
//! Does **not** query CRM/invoice tables (they do not exist yet). At most one
//! membership count query for the setup checklist.

use axum::extract::{Query, State};
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
    /// checklist | stat | module_empty | feed
    pub kind: String,
    /// ready | empty | unavailable | loading
    pub status: String,
    /// module_not_enabled | coming_in_later_phase | no_data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// Always false for honest empties in Phase 1.3 (pattern present for later).
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

    let widgets = build_widgets(role_layout, member_count, range_label.as_deref());

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
        empty_widget(
            "pipeline",
            "Pipeline",
            "module_empty",
            "unavailable",
            "module_not_enabled",
            range_label,
            json!({ "module": "sales", "message": "CRM pipeline is not enabled yet" }),
        ),
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
