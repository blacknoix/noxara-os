//! Proposal list, confirm, and cancel.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method};
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::gateway_client::forward_user_request;
use crate::handlers::common::{
    enforce_perm, extract_bearer, load_proposal_view, resolve_principal, ProposalRow,
};
use crate::state::AppState;
use crate::tools::find_tool;
use crate::types::{ConfirmProposalRequest, MessageResponse, ProposalView, ProposalsListResponse};
use companyos_errors::{AppError, ErrorCode};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/proposals", get(list_proposals))
        .route("/api/v1/ai/proposals/{id}/confirm", post(confirm_proposal))
        .route("/api/v1/ai/proposals/{id}/cancel", post(cancel_proposal))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/proposals",
    responses((status = 200, body = ProposalsListResponse)),
    tag = "ai"
)]
pub async fn list_proposals(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ProposalsListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_proposal_create(), &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let rows: Vec<ProposalRow> = sqlx::query_as(
        r#"
            SELECT id, tool_name, action_type, status, command, rendered_diff, citations, created_at
            FROM ai_proposal
            WHERE org_id = $1 AND user_id = $2
            ORDER BY created_at DESC
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
        .map(
            |(
                id,
                tool_name,
                action_type,
                status,
                command,
                rendered_diff,
                citations,
                created_at,
            )| {
                let citations: Vec<crate::types::Citation> =
                    serde_json::from_value(citations).unwrap_or_default();
                ProposalView {
                    id: id.to_string(),
                    tool_name,
                    action_type,
                    status,
                    command,
                    rendered_diff,
                    citations,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(ProposalsListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/proposals/{id}/confirm",
    request_body = ConfirmProposalRequest,
    responses((status = 200, body = ProposalView)),
    tag = "ai"
)]
pub async fn confirm_proposal(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(_req): Json<ConfirmProposalRequest>,
) -> Result<Json<ProposalView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;
    let bearer = extract_bearer(&headers).unwrap_or_default();

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_proposal_commit(), &request_id)?;

    let proposal_id = Uuid::parse_str(&id).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "invalid proposal id",
        )
    })?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let row: Option<(String, String, String, serde_json::Value, String, String)> = sqlx::query_as(
        r#"
            SELECT tool_name, action_type, status, domain_body, domain_path, domain_method
            FROM ai_proposal WHERE id = $1 AND org_id = $2 AND user_id = $3
            "#,
    )
    .bind(proposal_id)
    .bind(org_id.as_uuid())
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((tool_name, _action_type, status, domain_body, domain_path, domain_method)) = row
    else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "proposal not found",
        ));
    };

    if status != "pending" {
        return Err(AppError::new(
            ErrorCode::Conflict,
            &request_id,
            "proposal is not pending",
        ));
    }

    if let Some(def) = find_tool(&tool_name) {
        enforce_perm(&principal, (def.permission)(), &request_id)?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let method = match domain_method.as_str() {
        "GET" => Method::GET,
        "PATCH" => Method::PATCH,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => Method::POST,
    };

    let (status_code, result) = forward_user_request(
        &state,
        &bearer,
        method,
        &domain_path,
        Some(domain_body),
        true,
        user_id,
        &request_id,
    )
    .await?;

    if !status_code.is_success() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            format!("domain API returned {}", status_code.as_u16()),
        ));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE ai_proposal
        SET status = 'committed', result = $1, decided_at = now()
        WHERE id = $2 AND org_id = $3
        "#,
    )
    .bind(&result)
    .bind(proposal_id)
    .bind(org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let view = load_proposal_view(&state, org_id, proposal_id, &request_id).await?;
    Ok(Json(view))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/proposals/{id}/cancel",
    responses((status = 200, body = MessageResponse)),
    tag = "ai"
)]
pub async fn cancel_proposal(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_proposal_create(), &request_id)?;

    let proposal_id = Uuid::parse_str(&id).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "invalid proposal id",
        )
    })?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let updated = sqlx::query(
        r#"
        UPDATE ai_proposal
        SET status = 'cancelled', decided_at = now()
        WHERE id = $1 AND org_id = $2 AND user_id = $3 AND status = 'pending'
        "#,
    )
    .bind(proposal_id)
    .bind(org_id.as_uuid())
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "proposal not found or not pending",
        ));
    }

    Ok(Json(MessageResponse {
        message: "proposal cancelled".into(),
    }))
}
