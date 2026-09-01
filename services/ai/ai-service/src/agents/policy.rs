//! Versioned org-scoped agent policy.

use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

/// Document body for an agent policy version.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentPolicyDoc {
    pub name: String,
    pub agent_types: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub allowed_permissions: Vec<String>,
    pub spend_budget_tokens: i64,
    pub max_steps: i32,
    /// Permissions (or thresholds) that always require a human even if listed.
    pub require_human_above: Value,
    pub allowed_resource_scopes: Vec<String>,
}

impl Default for AgentPolicyDoc {
    fn default() -> Self {
        Self {
            name: "default".into(),
            agent_types: vec!["receivables_chase".into()],
            allowed_tools: vec![
                "list_overdue_invoices".into(),
                "send_invoice_reminder".into(),
                "escalate_exception".into(),
            ],
            allowed_permissions: vec![
                "finance.invoice.read".into(),
                "finance.invoice.send".into(),
                "platform.notification.read".into(),
                "operations.task.create".into(),
            ],
            spend_budget_tokens: 100_000,
            max_steps: 50,
            require_human_above: json!({
                "permissions": ["finance.invoice.void", "finance.journal.post", "hr.payroll.run"],
                "amount_minor": 1_000_000
            }),
            allowed_resource_scopes: vec!["finance.invoices".into(), "notifications".into()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolicySnapshot {
    pub id: Uuid,
    pub public_id: String,
    pub version: i32,
    pub doc: AgentPolicyDoc,
}

pub fn policy_allows_tool(doc: &AgentPolicyDoc, tool: &str) -> bool {
    doc.allowed_tools.iter().any(|t| t == tool)
}

pub fn policy_allows_permission(doc: &AgentPolicyDoc, perm: &str) -> bool {
    if human_required_permission(doc, perm) {
        return false;
    }
    doc.allowed_permissions.iter().any(|p| p == perm)
}

pub fn human_required_permission(doc: &AgentPolicyDoc, perm: &str) -> bool {
    doc.require_human_above
        .get("permissions")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|p| p.as_str() == Some(perm)))
        .unwrap_or(false)
}

pub async fn load_active_policy(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<Option<PolicySnapshot>, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: Option<(
        Uuid,
        String,
        i32,
        String,
        Value,
        Value,
        Value,
        i64,
        i32,
        Value,
        Value,
    )> = sqlx::query_as(
        r#"
        SELECT id, public_id, version, name, agent_types, allowed_tools, allowed_permissions,
               spend_budget_tokens, max_steps, require_human_above, allowed_resource_scopes
        FROM ai_agent_policy
        WHERE org_id = $1 AND status = 'active'
        ORDER BY version DESC
        LIMIT 1
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(row.map(
        |(
            id,
            public_id,
            version,
            name,
            agent_types,
            allowed_tools,
            allowed_permissions,
            spend_budget_tokens,
            max_steps,
            require_human_above,
            allowed_resource_scopes,
        )| {
            let doc = AgentPolicyDoc {
                name,
                agent_types: serde_json::from_value(agent_types).unwrap_or_default(),
                allowed_tools: serde_json::from_value(allowed_tools).unwrap_or_default(),
                allowed_permissions: serde_json::from_value(allowed_permissions)
                    .unwrap_or_default(),
                spend_budget_tokens,
                max_steps,
                require_human_above,
                allowed_resource_scopes: serde_json::from_value(allowed_resource_scopes)
                    .unwrap_or_default(),
            };
            PolicySnapshot {
                id,
                public_id,
                version,
                doc,
            }
        },
    ))
}

pub async fn load_policy_version(
    state: &AppState,
    org_id: OrgId,
    version: i32,
    request_id: &str,
) -> Result<Option<PolicySnapshot>, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: Option<(
        Uuid,
        String,
        i32,
        String,
        Value,
        Value,
        Value,
        i64,
        i32,
        Value,
        Value,
    )> = sqlx::query_as(
        r#"
        SELECT id, public_id, version, name, agent_types, allowed_tools, allowed_permissions,
               spend_budget_tokens, max_steps, require_human_above, allowed_resource_scopes
        FROM ai_agent_policy
        WHERE org_id = $1 AND version = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(row.map(
        |(
            id,
            public_id,
            version,
            name,
            agent_types,
            allowed_tools,
            allowed_permissions,
            spend_budget_tokens,
            max_steps,
            require_human_above,
            allowed_resource_scopes,
        )| {
            let doc = AgentPolicyDoc {
                name,
                agent_types: serde_json::from_value(agent_types).unwrap_or_default(),
                allowed_tools: serde_json::from_value(allowed_tools).unwrap_or_default(),
                allowed_permissions: serde_json::from_value(allowed_permissions)
                    .unwrap_or_default(),
                spend_budget_tokens,
                max_steps,
                require_human_above,
                allowed_resource_scopes: serde_json::from_value(allowed_resource_scopes)
                    .unwrap_or_default(),
            };
            PolicySnapshot {
                id,
                public_id,
                version,
                doc,
            }
        },
    ))
}

pub async fn publish_policy(
    state: &AppState,
    org_id: OrgId,
    created_by: Uuid,
    doc: &AgentPolicyDoc,
    request_id: &str,
) -> Result<PolicySnapshot, AppError> {
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AgentPolicy, id).as_str();

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let next_version: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM ai_agent_policy WHERE org_id = $1",
    )
    .bind(org_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    // Supersede previous active versions.
    sqlx::query(
        "UPDATE ai_agent_policy SET status = 'superseded' WHERE org_id = $1 AND status = 'active'",
    )
    .bind(org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_agent_policy
            (id, org_id, public_id, version, name, status, agent_types, allowed_tools,
             allowed_permissions, spend_budget_tokens, max_steps, require_human_above,
             allowed_resource_scopes, created_by)
        VALUES ($1,$2,$3,$4,$5,'active',$6,$7,$8,$9,$10,$11,$12,$13)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(next_version)
    .bind(&doc.name)
    .bind(json!(doc.agent_types))
    .bind(json!(doc.allowed_tools))
    .bind(json!(doc.allowed_permissions))
    .bind(doc.spend_budget_tokens)
    .bind(doc.max_steps)
    .bind(&doc.require_human_above)
    .bind(json!(doc.allowed_resource_scopes))
    .bind(created_by)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(PolicySnapshot {
        id,
        public_id,
        version: next_version,
        doc: doc.clone(),
    })
}

pub async fn list_policies(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<Vec<PolicySnapshot>, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let rows: Vec<(
        Uuid,
        String,
        i32,
        String,
        Value,
        Value,
        Value,
        i64,
        i32,
        Value,
        Value,
    )> = sqlx::query_as(
        r#"
        SELECT id, public_id, version, name, agent_types, allowed_tools, allowed_permissions,
               spend_budget_tokens, max_steps, require_human_above, allowed_resource_scopes
        FROM ai_agent_policy
        WHERE org_id = $1
        ORDER BY version DESC
        LIMIT 50
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                public_id,
                version,
                name,
                agent_types,
                allowed_tools,
                allowed_permissions,
                spend_budget_tokens,
                max_steps,
                require_human_above,
                allowed_resource_scopes,
            )| PolicySnapshot {
                id,
                public_id,
                version,
                doc: AgentPolicyDoc {
                    name,
                    agent_types: serde_json::from_value(agent_types).unwrap_or_default(),
                    allowed_tools: serde_json::from_value(allowed_tools).unwrap_or_default(),
                    allowed_permissions: serde_json::from_value(allowed_permissions)
                        .unwrap_or_default(),
                    spend_budget_tokens,
                    max_steps,
                    require_human_above,
                    allowed_resource_scopes: serde_json::from_value(allowed_resource_scopes)
                        .unwrap_or_default(),
                },
            },
        )
        .collect())
}
