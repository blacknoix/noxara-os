//! `ai_action` ledger — every autonomous write is attributable and reversible.

use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;
use crate::types::ToolTraceEntry;

pub const DEFAULT_REVERSIBILITY_WINDOW_SECS: i32 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AiActionView {
    pub id: String,
    pub public_id: String,
    pub run_id: Option<String>,
    pub agent_type: String,
    pub tool_name: String,
    pub permission: String,
    pub model: String,
    pub prompt_template_version: String,
    pub status: String,
    pub reversible: bool,
    pub error: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reversed_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct RecordActionInput<'a> {
    pub run_id: Option<Uuid>,
    pub agent_type: &'a str,
    pub tool_name: &'a str,
    pub permission: &'a str,
    pub model: &'a str,
    pub prompt_template_version: &'a str,
    pub tool_trace: &'a [ToolTraceEntry],
    pub command: Value,
    pub effect: Value,
    pub on_behalf_of: Option<Uuid>,
    pub policy_version: Option<i32>,
    pub error: bool,
    pub error_message: Option<String>,
}

pub async fn record_action(
    state: &AppState,
    org_id: OrgId,
    input: RecordActionInput<'_>,
    request_id: &str,
) -> Result<(Uuid, String), AppError> {
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AiAction, id).as_str();
    let status = if input.error { "failed" } else { "committed" };

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_action
            (id, org_id, public_id, run_id, agent_type, tool_name, permission, model,
             prompt_template_version, tool_trace, command, effect, status, reversible,
             reversibility_window_secs, error, error_message, on_behalf_of, policy_version)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,true,$14,$15,$16,$17,$18)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(input.run_id)
    .bind(input.agent_type)
    .bind(input.tool_name)
    .bind(input.permission)
    .bind(input.model)
    .bind(input.prompt_template_version)
    .bind(json!(input.tool_trace))
    .bind(&input.command)
    .bind(&input.effect)
    .bind(status)
    .bind(DEFAULT_REVERSIBILITY_WINDOW_SECS)
    .bind(input.error)
    .bind(&input.error_message)
    .bind(input.on_behalf_of)
    .bind(input.policy_version)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok((id, public_id))
}

pub async fn record_effect(
    state: &AppState,
    org_id: OrgId,
    action_id: Uuid,
    effect_type: &str,
    resource_type: Option<&str>,
    resource_id: Option<&str>,
    payload: Value,
    request_id: &str,
) -> Result<Uuid, AppError> {
    let id = new_uuid_v7();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_agent_effect
            (id, org_id, action_id, effect_type, resource_type, resource_id, payload, active)
        VALUES ($1,$2,$3,$4,$5,$6,$7,true)
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(action_id)
    .bind(effect_type)
    .bind(resource_type)
    .bind(resource_id)
    .bind(payload)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(id)
}

/// Undo an agent write as a unit within the reversibility window.
pub async fn reverse_action(
    state: &AppState,
    org_id: OrgId,
    action_id: Uuid,
    request_id: &str,
) -> Result<AiActionView, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: Option<(
        String,
        Option<Uuid>,
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        i32,
        bool,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        r#"
        SELECT public_id, run_id, agent_type, tool_name, permission, model,
               prompt_template_version, status, reversible, reversibility_window_secs,
               error, created_at, reversed_at
        FROM ai_action WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(action_id)
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((
        public_id,
        run_id,
        agent_type,
        tool_name,
        permission,
        model,
        prompt_template_version,
        status,
        reversible,
        window_secs,
        error,
        created_at,
        reversed_at,
    )) = row
    else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "ai_action not found",
        ));
    };

    if status == "reversed" || reversed_at.is_some() {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "action already reversed",
        ));
    }
    if !reversible || status != "committed" {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "action is not reversible",
        ));
    }
    let age = (chrono::Utc::now() - created_at).num_seconds();
    if age > i64::from(window_secs) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "reversibility window expired",
        ));
    }

    sqlx::query(
        r#"
        UPDATE ai_agent_effect
        SET active = false, reversed_at = now()
        WHERE org_id = $1 AND action_id = $2 AND active = true
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(action_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE ai_action
        SET status = 'reversed', reversed_at = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(action_id)
    .bind(org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(AiActionView {
        id: action_id.to_string(),
        public_id,
        run_id: run_id.map(|r| r.to_string()),
        agent_type,
        tool_name,
        permission,
        model,
        prompt_template_version,
        status: "reversed".into(),
        reversible,
        error,
        created_at,
        reversed_at: Some(chrono::Utc::now()),
    })
}
