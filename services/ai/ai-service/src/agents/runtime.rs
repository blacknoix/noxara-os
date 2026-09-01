//! Agent run lifecycle — Temporal-shaped IDs, kill switch, budget, policy pin.

use companyos_authz::Principal;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::handlers::common::{check_token_budget, load_settings, record_token_usage};
use crate::state::AppState;
use crate::types::ToolTraceEntry;

use super::kill_switch;
use super::policy::{self, PolicySnapshot};
use super::principal::effective_principal;
use super::prompt_pack;
use super::receivables::{self, ChaseStepResult};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StartRunRequest {
    pub agent_type: String,
    /// When true, run uses the active policy as scheduled principal (still no superuser).
    #[serde(default)]
    pub scheduled: bool,
    /// Optional per-step delay (ms) — used by kill-switch CI tests.
    #[serde(default)]
    pub step_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentRunView {
    pub id: String,
    pub public_id: String,
    pub agent_type: String,
    pub status: String,
    pub policy_version: i32,
    pub temporal_workflow_id: String,
    pub steps_taken: i32,
    pub tokens_used: i32,
    pub cost_estimate_minor: i64,
    pub last_actions: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentRunOutcome {
    pub run: AgentRunView,
    pub tool_trace: Vec<ToolTraceEntry>,
    pub action_ids: Vec<String>,
}

pub fn temporal_workflow_id(org_public: &str, run_public: &str) -> String {
    format!("{org_public}:AgentRun:{run_public}")
}

pub async fn start_and_run(
    state: &AppState,
    org_id: OrgId,
    org_public: &str,
    on_behalf_of_user: Uuid,
    on_behalf_principal: &Principal,
    req: &StartRunRequest,
    request_id: &str,
) -> Result<AgentRunOutcome, AppError> {
    if kill_switch::is_killed(state, org_id, &req.agent_type, request_id).await? {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "agent kill switch engaged — new runs refuse",
        ));
    }

    let settings = load_settings(state, org_id, request_id).await?;
    check_token_budget(&settings)?;

    let policy = policy::load_active_policy(state, org_id, request_id)
        .await?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                "no active agent policy — unattended writes denied (propose-then-commit remains default)",
            )
        })?;

    if !policy.doc.agent_types.iter().any(|t| t == &req.agent_type) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("agent type {} not allowed by policy", req.agent_type),
        ));
    }

    let effective = effective_principal(on_behalf_principal, &policy.doc);
    let pack = prompt_pack::load_active_prompt_pack(state, org_id, request_id).await?;

    let run_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AgentRun, run_id).as_str();
    let twf = temporal_workflow_id(org_public, &public_id);

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
        INSERT INTO ai_agent_run
            (id, org_id, public_id, agent_type, status, policy_id, policy_version,
             on_behalf_of, scheduled_policy, temporal_workflow_id)
        VALUES ($1,$2,$3,$4,'running',$5,$6,$7,$8,$9)
        "#,
    )
    .bind(run_id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(&req.agent_type)
    .bind(policy.id)
    .bind(policy.version)
    .bind(on_behalf_of_user)
    .bind(req.scheduled)
    .bind(&twf)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let outcome = match req.agent_type.as_str() {
        receivables::AGENT_TYPE => {
            run_receivables(
                state,
                org_id,
                run_id,
                &public_id,
                &twf,
                &policy,
                &effective,
                Some(on_behalf_of_user),
                &pack,
                req.step_delay_ms,
                request_id,
            )
            .await?
        }
        other => {
            mark_run(
                state,
                org_id,
                run_id,
                "failed",
                0,
                0,
                json!([]),
                Some(format!("unknown agent type: {other}")),
                request_id,
            )
            .await?;
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                format!("unknown agent type: {other}"),
            ));
        }
    };

    // Token accounting shares the org monthly budget.
    let tokens = (outcome.tool_trace.len() as u32).saturating_mul(50).max(1);
    let _ = record_token_usage(state, org_id, tokens, request_id).await;

    Ok(outcome)
}

async fn run_receivables(
    state: &AppState,
    org_id: OrgId,
    run_id: Uuid,
    public_id: &str,
    twf: &str,
    policy: &PolicySnapshot,
    effective: &Principal,
    on_behalf_of: Option<Uuid>,
    pack: &prompt_pack::PromptPackDoc,
    step_delay_ms: u64,
    request_id: &str,
) -> Result<AgentRunOutcome, AppError> {
    let invoices = receivables::fixture_overdue();
    let mut traces = Vec::new();
    let mut action_ids = Vec::new();
    let mut last_actions = Vec::new();
    let mut steps = 0i32;
    let mut killed = false;

    for inv in &invoices {
        if steps >= policy.doc.max_steps {
            break;
        }
        // Budget hard-stop mid-run.
        let settings = load_settings(state, org_id, request_id).await?;
        if check_token_budget(&settings).is_err() {
            last_actions.push(json!({"event": "budget_exhausted"}));
            break;
        }
        if kill_switch::is_killed(state, org_id, receivables::AGENT_TYPE, request_id).await? {
            killed = true;
            break;
        }

        let step: ChaseStepResult = receivables::chase_invoice(
            state,
            org_id,
            run_id,
            policy,
            effective,
            on_behalf_of,
            pack,
            inv,
            step_delay_ms,
            request_id,
        )
        .await?;

        steps += 1;
        traces.push(step.trace.clone());
        last_actions.push(json!({
            "tool": step.tool_name,
            "invoice_id": inv.invoice_id,
            "denied": step.denied,
            "reason": step.reason,
            "action_id": step.action_id.map(|a| a.to_string()),
        }));
        if let Some(aid) = step.action_id {
            action_ids.push(aid.to_string());
        }
        if step.reason == "kill_switch" {
            killed = true;
            break;
        }
    }

    let status = if killed {
        "killed"
    } else if traces.iter().any(|t| t.decision == "deny")
        && action_ids.is_empty()
        && !traces.iter().any(|t| t.decision == "allow")
    {
        "failed"
    } else {
        "completed"
    };

    let view = mark_run(
        state,
        org_id,
        run_id,
        status,
        steps,
        (steps as u32 * 50) as i32,
        json!(last_actions),
        if killed {
            Some("kill switch engaged".into())
        } else {
            None
        },
        request_id,
    )
    .await?;

    Ok(AgentRunOutcome {
        run: AgentRunView {
            id: run_id.to_string(),
            public_id: public_id.to_string(),
            agent_type: receivables::AGENT_TYPE.into(),
            status: view.status,
            policy_version: policy.version,
            temporal_workflow_id: twf.to_string(),
            steps_taken: steps,
            tokens_used: steps * 50,
            cost_estimate_minor: 0,
            last_actions: json!(last_actions),
            error_message: view.error_message,
            started_at: view.started_at,
            finished_at: view.finished_at,
        },
        tool_trace: traces,
        action_ids,
    })
}

struct Marked {
    status: String,
    error_message: Option<String>,
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn mark_run(
    state: &AppState,
    org_id: OrgId,
    run_id: Uuid,
    status: &str,
    steps: i32,
    tokens: i32,
    last_actions: Value,
    error_message: Option<String>,
    request_id: &str,
) -> Result<Marked, AppError> {
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
        UPDATE ai_agent_run
        SET status = $1, steps_taken = $2, tokens_used = $3, last_actions = $4,
            error_message = $5, finished_at = now()
        WHERE id = $6 AND org_id = $7
        "#,
    )
    .bind(status)
    .bind(steps)
    .bind(tokens)
    .bind(&last_actions)
    .bind(&error_message)
    .bind(run_id)
    .bind(org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: (
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT started_at, finished_at FROM ai_agent_run WHERE id = $1 AND org_id = $2",
    )
    .bind(run_id)
    .bind(org_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(Marked {
        status: status.to_string(),
        error_message,
        started_at: row.0,
        finished_at: row.1,
    })
}

pub async fn list_runs(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<Vec<AgentRunView>, AppError> {
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
        String,
        String,
        i32,
        String,
        i32,
        i32,
        i64,
        Value,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        r#"
        SELECT id, public_id, agent_type, status, policy_version, temporal_workflow_id,
               steps_taken, tokens_used, cost_estimate_minor, last_actions, error_message,
               started_at, finished_at
        FROM ai_agent_run
        WHERE org_id = $1
        ORDER BY started_at DESC
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
                agent_type,
                status,
                policy_version,
                temporal_workflow_id,
                steps_taken,
                tokens_used,
                cost_estimate_minor,
                last_actions,
                error_message,
                started_at,
                finished_at,
            )| AgentRunView {
                id: id.to_string(),
                public_id,
                agent_type,
                status,
                policy_version,
                temporal_workflow_id,
                steps_taken,
                tokens_used,
                cost_estimate_minor,
                last_actions,
                error_message,
                started_at,
                finished_at,
            },
        )
        .collect())
}

pub async fn get_run(
    state: &AppState,
    org_id: OrgId,
    run_id: Uuid,
    request_id: &str,
) -> Result<AgentRunView, AppError> {
    let runs = list_runs(state, org_id, request_id).await?;
    runs.into_iter()
        .find(|r| r.id == run_id.to_string())
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, request_id, "agent run not found"))
}
