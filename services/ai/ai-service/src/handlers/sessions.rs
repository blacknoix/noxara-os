//! Session list and detail.

use axum::extract::{Path, State};
use axum::{Json, Router};
use axum::routing::get;
use companyos_authz::perms;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::handlers::common::{resolve_principal, enforce_perm};
use crate::state::AppState;
use crate::types::{ChatResponse, SessionDetail, SessionSummary, SessionsListResponse, TokenUsage};
use companyos_errors::{AppError, ErrorCode};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/sessions", get(list_sessions))
        .route("/api/v1/ai/sessions/{id}", get(get_session))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/sessions",
    responses((status = 200, body = SessionsListResponse)),
    tag = "ai"
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<SessionsListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_copilot_use(), &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let rows: Vec<(Uuid, String, Option<String>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        r#"
        SELECT id, title, page_scope, updated_at
        FROM ai_session
        WHERE org_id = $1 AND user_id = $2
        ORDER BY updated_at DESC
        LIMIT 50
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let items = rows
        .into_iter()
        .map(|(id, title, page_scope, updated_at)| SessionSummary {
            id: id.to_string(),
            title,
            page_scope,
            updated_at,
        })
        .collect();

    Ok(Json(SessionsListResponse { items }))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/sessions/{id}",
    responses((status = 200, body = SessionDetail)),
    tag = "ai"
)]
pub async fn get_session(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_copilot_use(), &request_id)?;

    let session_id = Uuid::parse_str(&id).map_err(|_| {
        AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid session id")
    })?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let session_row: Option<(String, Option<String>, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            r#"
            SELECT title, page_scope, updated_at
            FROM ai_session WHERE id = $1 AND org_id = $2 AND user_id = $3
            "#,
        )
        .bind(session_id)
        .bind(org_id.as_uuid())
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((title, page_scope, updated_at)) = session_row else {
        return Err(AppError::new(ErrorCode::NotFound, &request_id, "session not found"));
    };

    let interaction_rows: Vec<(
        Uuid,
        String,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
        Option<String>,
        Option<String>,
        i32,
        i32,
        i32,
        i64,
        String,
    )> = sqlx::query_as(
        r#"
        SELECT id, content, citations, follow_ups, tool_trace, model, prompt_template_version,
               input_tokens, output_tokens, latency_ms, cost_estimate_minor, currency
        FROM ai_interaction
        WHERE session_id = $1 AND org_id = $2
        ORDER BY created_at
        "#,
    )
    .bind(session_id)
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let interactions = interaction_rows
        .into_iter()
        .map(|(iid, content, citations, _follow_ups, tool_trace, model, ptv, in_t, out_t, latency, cost, currency)| {
            let citations: Vec<crate::types::Citation> =
                serde_json::from_value(citations).unwrap_or_default();
            let tool_trace: Vec<crate::types::ToolTraceEntry> =
                serde_json::from_value(tool_trace).unwrap_or_default();
            ChatResponse {
                session_id: session_id.to_string(),
                interaction_id: iid.to_string(),
                role: "assistant".into(),
                content,
                citations,
                follow_ups: Vec::new(),
                proposals: Vec::new(),
                tool_trace,
                usage: TokenUsage {
                    model: model.unwrap_or_else(|| "mock".into()),
                    prompt_template_version: ptv.unwrap_or_else(|| "ai.chat.v1".into()),
                    input_tokens: in_t as u32,
                    output_tokens: out_t as u32,
                    latency_ms: latency as u32,
                    cost_estimate_minor: cost,
                    currency,
                },
            }
        })
        .collect();

    Ok(Json(SessionDetail {
        id: session_id.to_string(),
        title,
        page_scope,
        updated_at,
        interactions,
    }))
}
