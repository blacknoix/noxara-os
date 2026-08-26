//! Document extract and review endpoints.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_tenancy::set_session_org_id;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::document::{build_proposal_command, extract_from_text, persist_review};
use crate::handlers::common::{enforce_perm, resolve_principal};
use crate::state::AppState;
use crate::tools::{persist_proposal, ProposalDraft};
use crate::types::{DocumentExtractRequest, DocumentReview};
use companyos_errors::{AppError, ErrorCode};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/documents/extract", post(extract_document))
        .route(
            "/api/v1/ai/documents/reviews/{id}",
            get(get_document_review),
        )
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/documents/extract",
    request_body = DocumentExtractRequest,
    responses((status = 200, body = DocumentReview)),
    tag = "ai"
)]
pub async fn extract_document(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<DocumentExtractRequest>,
) -> Result<Json<DocumentReview>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_document_extract(), &request_id)?;

    if req.kind != "expense" && req.kind != "invoice" {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "kind must be expense or invoice",
        ));
    }

    let text = req.text.clone().unwrap_or_default();
    if text.is_empty() && req.file_id.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "text or file_id required",
        ));
    }

    let fields = extract_from_text(&text, &req.kind);
    let (tool_name, command, rendered_diff) = {
        let (t, cmd, diff) = build_proposal_command(&req.kind, &fields);
        (t, cmd, diff)
    };

    let draft = ProposalDraft {
        tool_name: tool_name.clone(),
        action_type: tool_name,
        command,
        rendered_diff,
        citations: Vec::new(),
        domain_path: if req.kind == "invoice" {
            "/api/v1/finance/invoices".into()
        } else {
            "/api/v1/finance/expenses".into()
        },
        domain_method: "POST".into(),
        domain_body: json!({
            "currency": fields.currency,
            "amount_minor": fields.amount_minor,
            "description": fields.vendor.clone().unwrap_or_else(|| "Extracted document".into()),
        }),
    };

    let proposal_id =
        persist_proposal(&state, org_id.as_uuid(), user_id, None, &draft, &request_id).await?;

    let review = persist_review(
        &state,
        org_id.as_uuid(),
        user_id,
        &req,
        &fields,
        Some(proposal_id),
        &request_id,
    )
    .await?;

    Ok(Json(review))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/documents/reviews/{id}",
    responses((status = 200, body = DocumentReview)),
    tag = "ai"
)]
pub async fn get_document_review(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<DocumentReview>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;

    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_document_extract(), &request_id)?;

    let review_id = Uuid::parse_str(&id).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "invalid review id",
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

    let row: Option<(String, f64, serde_json::Value, Option<Uuid>, String)> = sqlx::query_as(
        r#"
        SELECT kind, confidence, extracted, proposal_id, status
        FROM ai_document_review WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(review_id)
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some((kind, confidence, extracted, proposal_id, status)) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "review not found",
        ));
    };

    Ok(Json(DocumentReview {
        id: review_id.to_string(),
        kind,
        confidence,
        extracted,
        proposal_id: proposal_id.map(|u| u.to_string()),
        status,
    }))
}
