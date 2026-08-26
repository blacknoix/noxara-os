//! Shared handler helpers.

use axum::http::HeaderMap;
use companyos_authz::Principal;
use companyos_authz::Role;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::{OrgId, set_session_org_id};
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{
    AiSettings, DataSharingSettings, ModulesEnabled, ProposalView, TokenUsage,
};
use chrono::Utc;

pub(crate) type ProposalRow = (
    uuid::Uuid,
    String,
    String,
    String,
    serde_json::Value,
    String,
    serde_json::Value,
    chrono::DateTime<Utc>,
);

pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.to_string()))
}

pub async fn resolve_principal(
    state: &AppState,
    auth: &AuthCtx,
) -> Result<Principal, AppError> {
    let request_id = auth.ctx.request_id.clone();
    if auth.local_bypass {
        return Ok(Principal::with_roles(vec![Role::Owner]));
    }
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    Ok(principal)
}

pub fn default_modules() -> ModulesEnabled {
    ModulesEnabled {
        copilot: true,
        insights: true,
        document_ai: true,
        ask_mode: true,
    }
}

pub fn default_data_sharing() -> DataSharingSettings {
    DataSharingSettings {
        share_with_provider: false,
        allow_training: false,
    }
}

pub async fn load_settings(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<AiSettings, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: Option<(serde_json::Value, String, serde_json::Value, serde_json::Value, i64, i64, String)> =
        sqlx::query_as(
            r#"
            SELECT modules_enabled, model_preference, auto_execute_allow_list,
                   data_sharing, monthly_token_budget, tokens_used_this_month, budget_month
            FROM ai_org_settings WHERE org_id = $1
            "#,
        )
        .bind(org_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    if let Some((modules, model, allow_list, sharing, budget, used, month)) = row {
        let modules_enabled: ModulesEnabled =
            serde_json::from_value(modules).unwrap_or_else(|_| default_modules());
        let auto_execute_allow_list: Vec<String> =
            serde_json::from_value(allow_list).unwrap_or_default();
        let data_sharing: DataSharingSettings =
            serde_json::from_value(sharing).unwrap_or_else(|_| default_data_sharing());
        return Ok(AiSettings {
            modules_enabled,
            model_preference: model,
            auto_execute_allow_list,
            data_sharing,
            monthly_token_budget: budget,
            tokens_used_this_month: used,
            budget_month: month,
        });
    }

    Ok(AiSettings {
        modules_enabled: default_modules(),
        model_preference: "mock".into(),
        auto_execute_allow_list: Vec::new(),
        data_sharing: default_data_sharing(),
        monthly_token_budget: 500_000,
        tokens_used_this_month: 0,
        budget_month: Utc::now().format("%Y-%m").to_string(),
    })
}

pub async fn ensure_settings_row(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<(), AppError> {
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
        INSERT INTO ai_org_settings (org_id) VALUES ($1)
        ON CONFLICT (org_id) DO NOTHING
        "#,
    )
    .bind(org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}

pub async fn record_token_usage(
    state: &AppState,
    org_id: OrgId,
    tokens: u32,
    request_id: &str,
) -> Result<(), AppError> {
    let month = Utc::now().format("%Y-%m").to_string();
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
        INSERT INTO ai_org_settings (org_id, tokens_used_this_month, budget_month)
        VALUES ($1, $2, $3)
        ON CONFLICT (org_id) DO UPDATE SET
            tokens_used_this_month = CASE
                WHEN ai_org_settings.budget_month = $3
                THEN ai_org_settings.tokens_used_this_month + $2
                ELSE $2
            END,
            budget_month = $3,
            updated_at = now()
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(tokens as i64)
    .bind(&month)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}

pub fn build_usage(state: &AppState, usage: &crate::provider::CompletionResult) -> TokenUsage {
    TokenUsage {
        model: usage.model.clone(),
        prompt_template_version: state.prompt_template_version.clone(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        latency_ms: usage.latency_ms,
        cost_estimate_minor: usage.cost_estimate_minor,
        currency: "USD".into(),
    }
}

pub async fn load_proposal_view(
    state: &AppState,
    org_id: OrgId,
    proposal_id: Uuid,
    request_id: &str,
) -> Result<ProposalView, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: Option<ProposalRow> = sqlx::query_as(
        r#"
        SELECT id, tool_name, action_type, status, command, rendered_diff, citations, created_at
        FROM ai_proposal WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(proposal_id)
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((id, tool_name, action_type, status, command, rendered_diff, citations, created_at)) =
        row
    else {
        return Err(AppError::new(ErrorCode::NotFound, request_id, "proposal not found"));
    };

    let citations: Vec<crate::types::Citation> =
        serde_json::from_value(citations).unwrap_or_default();

    Ok(ProposalView {
        id: id.to_string(),
        tool_name,
        action_type,
        status,
        command,
        rendered_diff,
        citations,
        created_at,
    })
}

pub fn check_token_budget(settings: &AiSettings) -> Result<(), AppError> {
    if settings.tokens_used_this_month >= settings.monthly_token_budget {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "budget",
            "monthly token budget exceeded",
        ));
    }
    Ok(())
}

pub fn enforce_perm(
    principal: &Principal,
    permission: companyos_authz::PermissionId,
    request_id: &str,
) -> Result<(), AppError> {
    enforce(principal, permission, request_id)
}
