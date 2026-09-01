//! Phase 4.3 agent HTTP API — policy, kill switch, runs, NL workflow, review.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_ids::PublicId;
use serde::Deserialize;
use uuid::Uuid;

use crate::agents::action::{self, AiActionView};
use crate::agents::kill_switch::{self, KillSwitchView, SetKillSwitchRequest};
use crate::agents::nl_workflow::{self, NlWorkflowDraft, NlWorkflowRequest};
use crate::agents::policy::{self, AgentPolicyDoc};
use crate::agents::prompt_pack::{self, PromptPackDoc, PromptPackView};
use crate::agents::review::{self, AgentReviewReport};
use crate::agents::runtime::{self, AgentRunOutcome, AgentRunView, StartRunRequest};
use crate::auth::AuthCtx;
use crate::handlers::common::{enforce_perm, resolve_principal};
use crate::state::AppState;
use companyos_errors::{AppError, ErrorCode};
use serde::Serialize;
use utoipa::ToSchema;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/agents/policy",
            get(get_policy).post(publish_policy),
        )
        .route("/api/v1/ai/agents/policies", get(list_policies))
        .route(
            "/api/v1/ai/agents/kill-switch",
            get(get_kill).post(set_kill),
        )
        .route("/api/v1/ai/agents/runs", get(list_runs).post(start_run))
        .route("/api/v1/ai/agents/runs/{id}", get(get_run))
        .route(
            "/api/v1/ai/agents/actions/{id}/reverse",
            post(reverse_action),
        )
        .route(
            "/api/v1/ai/agents/prompt-pack",
            get(get_prompt_pack).post(put_prompt_pack),
        )
        .route(
            "/api/v1/ai/agents/workflows/propose",
            post(propose_workflow),
        )
        .route("/api/v1/ai/agents/review", get(get_review))
        .route(
            "/api/v1/ai/agents/review/seed-fixture",
            post(seed_review_fixture),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyView {
    pub id: String,
    pub public_id: String,
    pub version: i32,
    pub policy: AgentPolicyDoc,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyListResponse {
    pub items: Vec<PolicyView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RunsListResponse {
    pub items: Vec<AgentRunView>,
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/agents/policy",
    responses((status = 200, body = PolicyView)),
    tag = "ai-agents"
)]
pub async fn get_policy(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PolicyView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_read(), &request_id)?;

    let snap = policy::load_active_policy(&state, auth.ctx.org_id, &request_id)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, &request_id, "no active agent policy"))?;
    Ok(Json(PolicyView {
        id: snap.id.to_string(),
        public_id: snap.public_id,
        version: snap.version,
        policy: snap.doc,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/agents/policies",
    responses((status = 200, body = PolicyListResponse)),
    tag = "ai-agents"
)]
pub async fn list_policies(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PolicyListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_read(), &request_id)?;
    let items = policy::list_policies(&state, auth.ctx.org_id, &request_id)
        .await?
        .into_iter()
        .map(|snap| PolicyView {
            id: snap.id.to_string(),
            public_id: snap.public_id,
            version: snap.version,
            policy: snap.doc,
        })
        .collect();
    Ok(Json(PolicyListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/agents/policy",
    request_body = AgentPolicyDoc,
    responses((status = 200, body = PolicyView)),
    tag = "ai-agents"
)]
pub async fn publish_policy(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(doc): Json<AgentPolicyDoc>,
) -> Result<Json<PolicyView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_manage(), &request_id)?;
    let snap = policy::publish_policy(
        &state,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &doc,
        &request_id,
    )
    .await?;
    Ok(Json(PolicyView {
        id: snap.id.to_string(),
        public_id: snap.public_id,
        version: snap.version,
        policy: snap.doc,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/agents/kill-switch",
    responses((status = 200, body = KillSwitchView)),
    tag = "ai-agents"
)]
pub async fn get_kill(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<KillSwitchView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_read(), &request_id)?;
    Ok(Json(
        kill_switch::get_kill_switch(&state, auth.ctx.org_id, &request_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/agents/kill-switch",
    request_body = SetKillSwitchRequest,
    responses((status = 200, body = KillSwitchView)),
    tag = "ai-agents"
)]
pub async fn set_kill(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<SetKillSwitchRequest>,
) -> Result<Json<KillSwitchView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_kill(), &request_id)?;
    Ok(Json(
        kill_switch::set_kill_switch(
            &state,
            auth.ctx.org_id,
            auth.ctx.actor.user_id,
            &req,
            &request_id,
        )
        .await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/agents/runs",
    responses((status = 200, body = RunsListResponse)),
    tag = "ai-agents"
)]
pub async fn list_runs(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<RunsListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_read(), &request_id)?;
    let items = runtime::list_runs(&state, auth.ctx.org_id, &request_id).await?;
    Ok(Json(RunsListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/agents/runs",
    request_body = StartRunRequest,
    responses((status = 200, body = AgentRunOutcome)),
    tag = "ai-agents"
)]
pub async fn start_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<StartRunRequest>,
) -> Result<Json<AgentRunOutcome>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_run(), &request_id)?;

    let org_public = PublicId::new(companyos_ids::IdKind::Org, auth.ctx.org_id.as_uuid()).as_str();
    let outcome = runtime::start_and_run(
        &state,
        auth.ctx.org_id,
        &org_public,
        auth.ctx.actor.user_id,
        &principal,
        &req,
        &request_id,
    )
    .await?;
    Ok(Json(outcome))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/agents/runs/{id}",
    responses((status = 200, body = AgentRunView)),
    tag = "ai-agents"
)]
pub async fn get_run(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<AgentRunView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_read(), &request_id)?;
    let run_id = Uuid::parse_str(&id)
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid run id"))?;
    Ok(Json(
        runtime::get_run(&state, auth.ctx.org_id, run_id, &request_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/agents/actions/{id}/reverse",
    responses((status = 200, body = AiActionView)),
    tag = "ai-agents"
)]
pub async fn reverse_action(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<AiActionView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_manage(), &request_id)?;
    let action_id = Uuid::parse_str(&id).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "invalid action id",
        )
    })?;
    Ok(Json(
        action::reverse_action(&state, auth.ctx.org_id, action_id, &request_id).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/agents/prompt-pack",
    responses((status = 200, body = PromptPackDoc)),
    tag = "ai-agents"
)]
pub async fn get_prompt_pack(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PromptPackDoc>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_read(), &request_id)?;
    Ok(Json(
        prompt_pack::load_active_prompt_pack(&state, auth.ctx.org_id, &request_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/agents/prompt-pack",
    request_body = PromptPackDoc,
    responses((status = 200, body = PromptPackView)),
    tag = "ai-agents"
)]
pub async fn put_prompt_pack(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(doc): Json<PromptPackDoc>,
) -> Result<Json<PromptPackView>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_manage(), &request_id)?;
    Ok(Json(
        prompt_pack::upsert_prompt_pack(&state, auth.ctx.org_id, &doc, &request_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/agents/workflows/propose",
    request_body = NlWorkflowRequest,
    responses((status = 200, body = NlWorkflowDraft)),
    tag = "ai-agents"
)]
pub async fn propose_workflow(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(req): Json<NlWorkflowRequest>,
) -> Result<Json<NlWorkflowDraft>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_run(), &request_id)?;
    // Also need workflow write to land a draft the builder can open.
    enforce_perm(&principal, perms::operations_workflow_write(), &request_id)?;
    Ok(Json(
        nl_workflow::propose_workflow(
            &state,
            auth.ctx.org_id,
            auth.ctx.actor.user_id,
            &principal,
            &req,
            &request_id,
        )
        .await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct ReviewQuery {
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
}

#[utoipa::path(
    get,
    path = "/api/v1/ai/agents/review",
    responses((status = 200, body = AgentReviewReport)),
    tag = "ai-agents"
)]
pub async fn get_review(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ReviewQuery>,
) -> Result<Json<AgentReviewReport>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_read(), &request_id)?;
    let to = q.to.unwrap_or_else(chrono::Utc::now);
    let from = q.from.unwrap_or_else(|| to - chrono::Duration::days(90));
    Ok(Json(
        review::compute_review(&state, auth.ctx.org_id, from, to, &request_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/ai/agents/review/seed-fixture",
    responses((status = 200, body = AgentReviewReport)),
    tag = "ai-agents"
)]
pub async fn seed_review_fixture(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<AgentReviewReport>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let principal = resolve_principal(&state, &auth).await?;
    enforce_perm(&principal, perms::ai_agent_manage(), &request_id)?;
    review::seed_review_fixture(&state, auth.ctx.org_id, &request_id).await?;
    let to = chrono::Utc::now() + chrono::Duration::hours(1);
    let from = to - chrono::Duration::days(1);
    Ok(Json(
        review::compute_review(&state, auth.ctx.org_id, from, to, &request_id).await?,
    ))
}
