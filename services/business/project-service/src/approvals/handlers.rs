//! HTTP handlers for `/api/v1/operations/approvals/...` and policy CRUD.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::Deserialize;
use uuid::Uuid;

use super::policy::{
    fetch_policy_dto, find_matching_policy, insert_policy, publish_new_version, PolicyRow,
};
use super::routing::{build_routing_snapshot, RouteContext};
use super::seed::ensure_default_policies;
use super::temporal::{self, signal_decide, start_approval_process};
use super::types::*;
use crate::approvals::workflow_logic::ProcessState;
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::handlers::{
    conflict, internal, normalize_paging, not_found, parse_public_id, validation,
};
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/operations/approvals",
            get(list_approvals).post(create_approval),
        )
        .route(
            "/api/v1/operations/approvals/inbox/summary",
            get(inbox_summary),
        )
        .route(
            "/api/v1/operations/approvals/bulk-decide",
            post(bulk_decide),
        )
        .route("/api/v1/operations/approvals/{id}", get(get_approval))
        .route(
            "/api/v1/operations/approvals/{id}/decide",
            post(decide_approval),
        )
        .route(
            "/api/v1/operations/approvals/{id}/escalate",
            post(escalate_approval),
        )
        .route(
            "/api/v1/operations/approvals/delegations",
            post(create_delegation).get(list_delegations),
        )
        .route(
            "/api/v1/operations/approval-policies",
            get(list_policies).post(create_policy),
        )
        .route(
            "/api/v1/operations/approval-policies/{id}",
            get(get_policy).post(update_policy),
        )
}

#[derive(Debug, Deserialize, Default)]
pub struct ListApprovalsQuery {
    pub status: Option<String>,
    pub pending_for_me: Option<bool>,
    pub subject_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn user_pub(u: Uuid) -> String {
    PublicId::new(IdKind::User, u).as_str()
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

#[derive(Debug, sqlx::FromRow)]
struct ApprovalRow {
    id: Uuid,
    public_id: String,
    subject_type: String,
    subject_id: String,
    status: String,
    requester_user_id: Uuid,
    amount_minor: Option<i64>,
    currency: Option<String>,
    category: Option<String>,
    title: String,
    summary: Option<String>,
    mode: String,
    current_step: i32,
    #[allow(dead_code)]
    policy_id: Uuid,
    policy_version: i32,
    routing_snapshot: serde_json::Value,
    decided_at: Option<DateTime<Utc>>,
    decided_by: Option<Uuid>,
    decision_note: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    policy_public_id: String,
}

async fn load_steps<'e, E>(
    executor: E,
    org_id: Uuid,
    approval_id: Uuid,
) -> Result<Vec<ApprovalStepDto>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        i32,
        String,
        Option<String>,
        Vec<Uuid>,
        Option<i32>,
        Option<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        Option<Uuid>,
    )> = sqlx::query_as(
        r#"
        SELECT step_order, status, approver_role, assignee_user_ids, sla_seconds,
               escalate_to_role, escalated_at, decided_at, decided_by
        FROM operations_approval_step
        WHERE org_id = $1 AND approval_id = $2
        ORDER BY step_order
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                order,
                status,
                approver_role,
                assignees,
                sla,
                escalate,
                escalated_at,
                decided_at,
                decided_by,
            )| ApprovalStepDto {
                order,
                status,
                approver_role,
                assignee_user_ids: assignees.into_iter().map(user_pub).collect(),
                sla_seconds: sla,
                escalate_to_role: escalate,
                escalated_at: escalated_at.map(ts),
                decided_at: decided_at.map(ts),
                decided_by: decided_by.map(user_pub),
            },
        )
        .collect())
}

fn row_to_dto(row: ApprovalRow, steps: Vec<ApprovalStepDto>) -> ApprovalDto {
    let snapshot: RoutingSnapshot =
        serde_json::from_value(row.routing_snapshot.clone()).unwrap_or(RoutingSnapshot {
            policy_public_id: row.policy_public_id.clone(),
            policy_name: String::new(),
            policy_version: row.policy_version,
            mode: ApprovalMode::parse(&row.mode).unwrap_or(ApprovalMode::Any),
            match_criteria: Default::default(),
            steps: vec![],
            rationale: String::new(),
        });
    ApprovalDto {
        id: row.public_id,
        subject_type: row.subject_type,
        subject_id: row.subject_id,
        status: row.status,
        requester_user_id: user_pub(row.requester_user_id),
        amount_minor: row.amount_minor,
        currency: row.currency,
        category: row.category,
        title: row.title,
        summary: row.summary,
        mode: row.mode,
        current_step: row.current_step,
        policy_id: row.policy_public_id,
        policy_version: row.policy_version,
        routing_snapshot: snapshot,
        steps,
        decided_at: row.decided_at.map(ts),
        decided_by: row.decided_by.map(user_pub),
        decision_note: row.decision_note,
        created_at: ts(row.created_at),
        updated_at: ts(row.updated_at),
    }
}

async fn fetch_approval_dto(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    approval_id: Uuid,
) -> Result<Option<ApprovalDto>, sqlx::Error> {
    let row: Option<ApprovalRow> = sqlx::query_as(
        r#"
        SELECT a.id, a.public_id, a.subject_type, a.subject_id, a.status,
               a.requester_user_id, a.amount_minor, a.currency, a.category,
               a.title, a.summary, a.mode, a.current_step, a.policy_id, a.policy_version,
               a.routing_snapshot, a.decided_at, a.decided_by, a.decision_note,
               a.created_at, a.updated_at, p.public_id AS policy_public_id
        FROM operations_approval a
        JOIN operations_approval_policy p ON p.id = a.policy_id
        WHERE a.org_id = $1 AND a.id = $2
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let steps = load_steps(&mut **tx, org_id, approval_id).await?;
    Ok(Some(row_to_dto(row, steps)))
}

/// Users who may act on a step: assignees + active delegates.
async fn actor_can_decide_step(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    approval_id: Uuid,
    step_order: i32,
    actor: Uuid,
) -> Result<bool, sqlx::Error> {
    let assignees: Option<Vec<Uuid>> = sqlx::query_scalar(
        r#"
        SELECT assignee_user_ids FROM operations_approval_step
        WHERE org_id = $1 AND approval_id = $2 AND step_order = $3
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .bind(step_order)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(assignees) = assignees else {
        return Ok(false);
    };
    if assignees.contains(&actor) {
        return Ok(true);
    }
    // Delegation: someone in assignees delegated to actor (window or this request).
    let delegated: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
          SELECT 1 FROM operations_approval_delegation d
          WHERE d.org_id = $1
            AND d.to_user_id = $2
            AND d.from_user_id = ANY($3)
            AND d.revoked_at IS NULL
            AND (d.ends_at IS NULL OR d.ends_at > now())
            AND (d.approval_id IS NULL OR d.approval_id = $4)
        )
        "#,
    )
    .bind(org_id)
    .bind(actor)
    .bind(&assignees)
    .bind(approval_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(delegated)
}

/// GET inbox summary
#[utoipa::path(get, path = "/api/v1/operations/approvals/inbox/summary", tag = "operations-approvals",
    responses((status = 200, body = InboxSummaryDto)))]
pub async fn inbox_summary(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<InboxSummaryDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT a.id)
        FROM operations_approval a
        JOIN operations_approval_step s
          ON s.approval_id = a.id AND s.org_id = a.org_id AND s.step_order = a.current_step
        WHERE a.org_id = $1
          AND a.status = 'pending'
          AND s.status IN ('active', 'pending')
          AND (
            $2 = ANY(s.assignee_user_ids)
            OR EXISTS (
              SELECT 1 FROM operations_approval_delegation d
              WHERE d.org_id = a.org_id
                AND d.to_user_id = $2
                AND d.from_user_id = ANY(s.assignee_user_ids)
                AND d.revoked_at IS NULL
                AND (d.ends_at IS NULL OR d.ends_at > now())
                AND (d.approval_id IS NULL OR d.approval_id = a.id)
            )
          )
        "#,
    )
    .bind(org_id)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(InboxSummaryDto {
        pending_for_me: count,
    }))
}

/// GET list
#[utoipa::path(get, path = "/api/v1/operations/approvals", tag = "operations-approvals",
    responses((status = 200, body = ApprovalListResponse)))]
pub async fn list_approvals(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListApprovalsQuery>,
) -> Result<Json<ApprovalListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_read(),
        &request_id,
    )?;
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let pending_for_me = q.pending_for_me.unwrap_or(true);
    let status = q.status.as_deref().unwrap_or("pending");

    let rows: Vec<ApprovalRow> = if pending_for_me {
        sqlx::query_as(
            r#"
            SELECT a.id, a.public_id, a.subject_type, a.subject_id, a.status,
                   a.requester_user_id, a.amount_minor, a.currency, a.category,
                   a.title, a.summary, a.mode, a.current_step, a.policy_id, a.policy_version,
                   a.routing_snapshot, a.decided_at, a.decided_by, a.decision_note,
                   a.created_at, a.updated_at, p.public_id AS policy_public_id
            FROM operations_approval a
            JOIN operations_approval_policy p ON p.id = a.policy_id
            JOIN operations_approval_step s
              ON s.approval_id = a.id AND s.org_id = a.org_id AND s.step_order = a.current_step
            WHERE a.org_id = $1
              AND a.status = $2
              AND (
                $3 = ANY(s.assignee_user_ids)
                OR EXISTS (
                  SELECT 1 FROM operations_approval_delegation d
                  WHERE d.org_id = a.org_id AND d.to_user_id = $3
                    AND d.from_user_id = ANY(s.assignee_user_ids)
                    AND d.revoked_at IS NULL
                    AND (d.ends_at IS NULL OR d.ends_at > now())
                    AND (d.approval_id IS NULL OR d.approval_id = a.id)
                )
              )
              AND ($4::text IS NULL OR a.subject_type = $4)
            ORDER BY a.created_at DESC
            LIMIT $5 OFFSET $6
            "#,
        )
        .bind(org_id)
        .bind(status)
        .bind(actor)
        .bind(q.subject_type.as_deref())
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    } else {
        sqlx::query_as(
            r#"
            SELECT a.id, a.public_id, a.subject_type, a.subject_id, a.status,
                   a.requester_user_id, a.amount_minor, a.currency, a.category,
                   a.title, a.summary, a.mode, a.current_step, a.policy_id, a.policy_version,
                   a.routing_snapshot, a.decided_at, a.decided_by, a.decision_note,
                   a.created_at, a.updated_at, p.public_id AS policy_public_id
            FROM operations_approval a
            JOIN operations_approval_policy p ON p.id = a.policy_id
            WHERE a.org_id = $1
              AND ($2::text IS NULL OR a.status = $2)
              AND ($3::text IS NULL OR a.subject_type = $3)
            ORDER BY a.created_at DESC
            LIMIT $4 OFFSET $5
            "#,
        )
        .bind(org_id)
        .bind(q.status.as_deref())
        .bind(q.subject_type.as_deref())
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    };

    let total = rows.len() as i64;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let steps = load_steps(&mut *tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?;
        items.push(row_to_dto(row, steps));
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(ApprovalListResponse { items, total }))
}

/// GET one
#[utoipa::path(get, path = "/api/v1/operations/approvals/{id}", tag = "operations-approvals",
    responses((status = 200, body = ApprovalDto)))]
pub async fn get_approval(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ApprovalDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let approval_id = parse_public_id(IdKind::Approval, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let dto = fetch_approval_dto(&mut tx, org_id, approval_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "approval"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST create / request
#[utoipa::path(post, path = "/api/v1/operations/approvals", tag = "operations-approvals",
    request_body = CreateApprovalRequest,
    responses((status = 201, body = ApprovalDto)))]
pub async fn create_approval(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateApprovalRequest>,
) -> Result<axum::response::Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    // Creating a request is allowed for members who can read approvals (callers
    // are finance/CRM services acting as the requester).
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_read(),
        &request_id,
    )?;

    if body.subject_type.trim().is_empty() || body.subject_id.trim().is_empty() {
        return Err(validation(
            &request_id,
            "subject_type and subject_id are required",
        ));
    }

    let _ = ensure_default_policies(&state.pool, org_id).await;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "approval.create", &key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    // Idempotent subject: return existing open approval.
    let existing: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id FROM operations_approval
        WHERE org_id = $1 AND subject_type = $2 AND subject_id = $3
        "#,
    )
    .bind(org_id)
    .bind(&body.subject_type)
    .bind(&body.subject_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    if let Some(existing_id) = existing {
        let dto = fetch_approval_dto(&mut tx, org_id, existing_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "approval"))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok((StatusCode::OK, Json(dto)).into_response());
    }

    let discount_bps: Option<i64> = None;
    let ctx = RouteContext::from_request(&body, discount_bps);
    // For quote_discount, callers may pass discount_bps in summary JSON — also
    // accept category as "discount_bps:<n>".
    let ctx = if body.subject_type == "quote_discount" {
        let bps = body
            .summary
            .as_deref()
            .and_then(|s| {
                s.strip_prefix("discount_bps:")
                    .and_then(|n| n.trim().parse().ok())
            })
            .or_else(|| {
                body.category
                    .as_deref()
                    .and_then(|c| c.strip_prefix("bps:").and_then(|n| n.parse().ok()))
            });
        RouteContext {
            discount_bps: bps,
            ..ctx
        }
    } else {
        ctx
    };

    let matched = find_matching_policy(&mut tx, org_id, &body.subject_type, &ctx)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            conflict(
                &request_id,
                format!(
                    "no active approval policy matched subject_type={}",
                    body.subject_type
                ),
            )
        })?;
    let (policy_row, version_id, def): (PolicyRow, Uuid, PolicyDefinition) = matched;

    let snapshot = build_routing_snapshot(
        &mut tx,
        org_id,
        &policy_row.public_id,
        &policy_row.name,
        policy_row.current_version,
        &def,
        &ctx,
    )
    .await
    .map_err(internal(&request_id))?;

    if snapshot.steps.is_empty() {
        return Err(conflict(
            &request_id,
            "policy has no steps after resolution",
        ));
    }

    let public_id = PublicId::generate(IdKind::Approval);
    let approval_id = public_id.uuid();
    let title = body
        .title
        .clone()
        .unwrap_or_else(|| format!("{} approval", body.subject_type));
    let org_public = auth.ctx.org_id.to_public().as_str();
    let wf_id = ProcessState::workflow_id(&org_public, &public_id.as_str());
    let snapshot_json = serde_json::to_value(&snapshot).unwrap_or_default();

    sqlx::query(
        r#"
        INSERT INTO operations_approval (
            id, org_id, public_id, subject_type, subject_id, status,
            requester_user_id, requester_role, amount_minor, currency, category,
            department_id, policy_id, policy_version, policy_version_id,
            routing_snapshot, mode, current_step, title, summary, temporal_workflow_id
        ) VALUES (
            $1,$2,$3,$4,$5,'pending',$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,1,$17,$18,$19
        )
        "#,
    )
    .bind(approval_id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&body.subject_type)
    .bind(&body.subject_id)
    .bind(actor)
    .bind(&body.requester_role)
    .bind(body.amount_minor)
    .bind(body.currency.as_deref())
    .bind(body.category.as_deref())
    .bind(ctx.department_id)
    .bind(policy_row.id)
    .bind(policy_row.current_version)
    .bind(version_id)
    .bind(&snapshot_json)
    .bind(def.mode.as_str())
    .bind(&title)
    .bind(body.summary.as_deref())
    .bind(&wf_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for step in &snapshot.steps {
        let status = if step.order == snapshot.steps[0].order {
            "active"
        } else {
            "pending"
        };
        sqlx::query(
            r#"
            INSERT INTO operations_approval_step (
                id, org_id, approval_id, step_order, status, approver_role,
                assignee_user_ids, sla_seconds, escalate_to_role
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(approval_id)
        .bind(step.order)
        .bind(status)
        .bind(step.approver_role.as_deref())
        .bind(&step.assignee_user_ids)
        .bind(step.sla_seconds)
        .bind(step.escalate_to_role.as_deref())
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let requested_env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "approval",
        "requested",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "subject_type": body.subject_type,
            "subject_id": body.subject_id,
            "policy_version": policy_row.current_version,
            "routing_rationale": snapshot.rationale,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &requested_env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        actor,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.approval.request",
        "approval",
        &public_id.as_str(),
        serde_json::json!({
            "policy_version": policy_row.current_version,
            "subject_type": body.subject_type,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_approval_dto(&mut tx, org_id, approval_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "approval missing after insert",
            )
        })?;

    if let Some(key) = idempotency::header_key(&headers) {
        idempotency::put(
            &mut *tx,
            org_id,
            "approval.create",
            &key,
            201,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;

    let sla = snapshot
        .steps
        .first()
        .and_then(|s| s.sla_seconds)
        .unwrap_or(86_400);
    let _ = start_approval_process(ApprovalProcessInput {
        org_id: org_public,
        approval_id: approval_id.to_string(),
        approval_public_id: public_id.as_str(),
        sla_seconds: sla,
        current_step: 1,
        mode: def.mode.as_str().to_string(),
    })
    .await;

    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// POST decide — Idempotency-Key; duplicate decide is a no-op.
#[utoipa::path(post, path = "/api/v1/operations/approvals/{id}/decide", tag = "operations-approvals",
    request_body = DecideApprovalRequest,
    responses((status = 200, body = ApprovalDto)))]
pub async fn decide_approval(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<DecideApprovalRequest>,
) -> Result<Json<ApprovalDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let approval_id = parse_public_id(IdKind::Approval, &id, &request_id)?;
    let actor = auth.ctx.actor.user_id;
    let on_behalf_of = auth.ctx.actor.on_behalf_of;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_decide(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idempotency::header_key(&headers) {
        if let Some((_status, stored)) =
            idempotency::get(&mut *tx, org_id, &format!("approval.decide.{id}"), &key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let dto: ApprovalDto = serde_json::from_value(stored).map_err(|e| {
                AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
            })?;
            return Ok(Json(dto));
        }
    }

    let row: Option<(String, i32, String)> = sqlx::query_as(
        r#"
        SELECT status, current_step, public_id FROM operations_approval
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some((status, current_step, public_id)) = row else {
        return Err(not_found(&request_id, "approval"));
    };

    // Already decided → no-op (return current).
    if status != "pending" {
        let dto = fetch_approval_dto(&mut tx, org_id, approval_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "approval"))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(dto));
    }

    let can = actor_can_decide_step(&mut tx, org_id, approval_id, current_step, actor)
        .await
        .map_err(internal(&request_id))?;
    if !can {
        // Still allow users with decide permission who are Org Owner/Admin via
        // empty assignee fallback: if assignees empty, any decide-permitted user.
        let assignees: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT UNNEST(assignee_user_ids) FROM operations_approval_step
            WHERE org_id = $1 AND approval_id = $2 AND step_order = $3
            "#,
        )
        .bind(org_id)
        .bind(approval_id)
        .bind(current_step)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if !assignees.is_empty() {
            return Err(AppError::new(
                ErrorCode::Forbidden,
                request_id.clone(),
                "not an assignee (or delegate) for the active approval step",
            ));
        }
    }

    let new_status = if body.approve { "approved" } else { "rejected" };
    let decision = if body.approve { "approve" } else { "reject" };

    let updated: u64 = sqlx::query(
        r#"
        UPDATE operations_approval SET
            status = $3,
            decided_at = now(),
            decided_by = $4,
            decision_note = $5,
            updated_at = now()
        WHERE org_id = $1 AND id = $2 AND status = 'pending'
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .bind(new_status)
    .bind(actor)
    .bind(body.comment.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .rows_affected();

    if updated == 0 {
        // Race: another decide won — return current (no-op).
        let dto = fetch_approval_dto(&mut tx, org_id, approval_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "approval"))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(dto));
    }

    sqlx::query(
        r#"
        UPDATE operations_approval_step SET
            status = $4,
            decided_at = now(),
            decided_by = $5
        WHERE org_id = $1 AND approval_id = $2 AND step_order = $3
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .bind(current_step)
    .bind(new_status)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let idem_key = idempotency::header_key(&headers);
    sqlx::query(
        r#"
        INSERT INTO operations_approval_decision (
            id, org_id, approval_id, step_order, decision,
            actor_user_id, on_behalf_of, comment, idempotency_key
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .bind(approval_id)
    .bind(current_step)
    .bind(decision)
    .bind(actor)
    .bind(on_behalf_of)
    .bind(body.comment.as_deref())
    .bind(idem_key.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let decided_env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "approval",
        "decided",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id,
            "decision": decision,
            "actor_user_id": user_pub(actor),
            "on_behalf_of": user_pub(on_behalf_of),
            "comment": body.comment,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &decided_env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        actor,
        on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.approval.decide",
        "approval",
        &public_id,
        serde_json::json!({
            "decision": decision,
            "on_behalf_of": user_pub(on_behalf_of),
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_approval_dto(&mut tx, org_id, approval_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "approval"))?;

    if let Some(key) = idem_key {
        idempotency::put(
            &mut *tx,
            org_id,
            &format!("approval.decide.{id}"),
            &key,
            200,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;

    let org_public = auth.ctx.org_id.to_public().as_str();
    let wf_id = temporal::workflow_id(&org_public, &public_id);
    let _ = signal_decide(
        &wf_id,
        DecideSignal {
            approve: body.approve,
            actor_user_id: user_pub(actor),
            on_behalf_of: user_pub(on_behalf_of),
            comment: body.comment.clone(),
            idempotency_key: idempotency::header_key(&headers),
        },
    )
    .await;

    // Side-effect callback to finance/CRM (bounded context: HTTP, not table reads).
    let _ = apply_subject_side_effect(&state, &auth, &dto).await;

    Ok(Json(dto))
}

async fn apply_subject_side_effect(
    _state: &AppState,
    auth: &AuthCtx,
    dto: &ApprovalDto,
) -> anyhow::Result<()> {
    let approve = dto.status == "approved";
    match dto.subject_type.as_str() {
        "expense" => {
            let finance_url = std::env::var("FINANCE_SERVICE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8083".into());
            let url = format!(
                "{}/api/v1/finance/expenses/{}/decide",
                finance_url.trim_end_matches('/'),
                dto.subject_id
            );
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()?;
            let mut req = client.post(&url).json(&serde_json::json!({
                "approve": approve,
                "note": dto.decision_note,
            }));
            // Forward local-auth headers so finance sees the same actor.
            req = req
                .header(
                    "x-companyos-dev-org-id",
                    auth.ctx.org_id.to_public().as_str(),
                )
                .header(
                    "x-companyos-dev-user-id",
                    PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
                )
                .header(
                    "x-companyos-on-behalf-of",
                    user_pub(auth.ctx.actor.on_behalf_of),
                );
            let _ = req.send().await;
        }
        "quote_discount" => {
            let crm_url =
                std::env::var("CRM_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8082".into());
            let path = if approve {
                format!(
                    "{}/api/v1/sales/quotes/{}/approval-complete",
                    crm_url.trim_end_matches('/'),
                    dto.subject_id
                )
            } else {
                format!(
                    "{}/api/v1/sales/quotes/{}/approval-reject",
                    crm_url.trim_end_matches('/'),
                    dto.subject_id
                )
            };
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()?;
            let mut req = client.post(&path).json(&serde_json::json!({}));
            req = req
                .header(
                    "x-companyos-dev-org-id",
                    auth.ctx.org_id.to_public().as_str(),
                )
                .header(
                    "x-companyos-dev-user-id",
                    PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
                );
            let _ = req.send().await;
        }
        "leave_request" => {
            let hr_url =
                std::env::var("HR_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8088".into());
            let url = format!(
                "{}/api/v1/people/leave-requests/{}/decide",
                hr_url.trim_end_matches('/'),
                dto.subject_id
            );
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()?;
            let mut req = client.post(&url).json(&serde_json::json!({
                "approve": approve,
                "note": dto.decision_note,
            }));
            req = req
                .header(
                    "x-companyos-dev-org-id",
                    auth.ctx.org_id.to_public().as_str(),
                )
                .header(
                    "x-companyos-dev-user-id",
                    PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
                )
                .header(
                    "x-companyos-on-behalf-of",
                    user_pub(auth.ctx.actor.on_behalf_of),
                );
            let _ = req.send().await;
        }
        "payroll_run" => {
            let hr_url =
                std::env::var("HR_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8088".into());
            let url = format!(
                "{}/api/v1/people/payroll/runs/{}/decide",
                hr_url.trim_end_matches('/'),
                dto.subject_id
            );
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()?;
            let mut req = client.post(&url).json(&serde_json::json!({
                "approve": approve,
                "note": dto.decision_note,
            }));
            req = req
                .header(
                    "x-companyos-dev-org-id",
                    auth.ctx.org_id.to_public().as_str(),
                )
                .header(
                    "x-companyos-dev-user-id",
                    PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
                )
                .header(
                    "x-companyos-on-behalf-of",
                    user_pub(auth.ctx.actor.on_behalf_of),
                );
            let _ = req.send().await;
        }
        "purchase_request" => {
            let inventory_url = std::env::var("INVENTORY_SERVICE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8093".into());
            let url = format!(
                "{}/api/v1/inventory/purchase-requests/{}/decide",
                inventory_url.trim_end_matches('/'),
                dto.subject_id
            );
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()?;
            let mut req = client.post(&url).json(&serde_json::json!({
                "approve": approve,
                "note": dto.decision_note,
            }));
            req = req
                .header(
                    "x-companyos-dev-org-id",
                    auth.ctx.org_id.to_public().as_str(),
                )
                .header(
                    "x-companyos-dev-user-id",
                    PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
                )
                .header(
                    "x-companyos-on-behalf-of",
                    user_pub(auth.ctx.actor.on_behalf_of),
                );
            let _ = req.send().await;
        }
        _ => {}
    }
    Ok(())
}

/// Internal escalate (Temporal activity / SLA).
#[utoipa::path(post, path = "/api/v1/operations/approvals/{id}/escalate", tag = "operations-approvals",
    responses((status = 200, body = ApprovalDto)))]
pub async fn escalate_approval(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ApprovalDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let approval_id = parse_public_id(IdKind::Approval, &id, &request_id)?;
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_decide(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<(String, i32)> = sqlx::query_as(
        "SELECT status, current_step FROM operations_approval WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(approval_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some((status, current_step)) = row else {
        return Err(not_found(&request_id, "approval"));
    };
    if status != "pending" {
        let dto = fetch_approval_dto(&mut tx, org_id, approval_id)
            .await
            .map_err(internal(&request_id))?
            .ok_or_else(|| not_found(&request_id, "approval"))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Ok(Json(dto));
    }

    let escalate_role: Option<String> = sqlx::query_scalar(
        r#"
        SELECT escalate_to_role FROM operations_approval_step
        WHERE org_id = $1 AND approval_id = $2 AND step_order = $3
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .bind(current_step)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .flatten();

    let max_step: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(step_order), 0) FROM operations_approval_step WHERE org_id = $1 AND approval_id = $2",
    )
    .bind(org_id)
    .bind(approval_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    sqlx::query(
        r#"
        UPDATE operations_approval_step SET status = 'escalated', escalated_at = now()
        WHERE org_id = $1 AND approval_id = $2 AND step_order = $3
        "#,
    )
    .bind(org_id)
    .bind(approval_id)
    .bind(current_step)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if current_step < max_step {
        let next = current_step + 1;
        sqlx::query(
            r#"
            UPDATE operations_approval_step SET status = 'active'
            WHERE org_id = $1 AND approval_id = $2 AND step_order = $3
            "#,
        )
        .bind(org_id)
        .bind(approval_id)
        .bind(next)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        sqlx::query(
            "UPDATE operations_approval SET current_step = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(approval_id)
        .bind(next)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    } else if let Some(role) = escalate_role {
        // Expand assignees on current step to escalate_to_role members.
        let users: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT m.user_id FROM membership m
            JOIN org_role r ON r.id = m.role_id
            WHERE m.org_id = $1 AND m.revoked_at IS NULL AND lower(r.system_key) = lower($2)
            "#,
        )
        .bind(org_id)
        .bind(&role)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        sqlx::query(
            r#"
            UPDATE operations_approval_step SET
                status = 'active',
                assignee_user_ids = $4,
                approver_role = $5
            WHERE org_id = $1 AND approval_id = $2 AND step_order = $3
            "#,
        )
        .bind(org_id)
        .bind(approval_id)
        .bind(current_step)
        .bind(&users)
        .bind(&role)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        sqlx::query(
            "UPDATE operations_approval SET status = 'escalated', updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(approval_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        // Keep pending for decision after escalation.
        sqlx::query(
            "UPDATE operations_approval SET status = 'pending' WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(approval_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    sqlx::query(
        r#"
        INSERT INTO operations_approval_decision (
            id, org_id, approval_id, step_order, decision,
            actor_user_id, on_behalf_of, comment
        ) VALUES ($1,$2,$3,$4,'escalate',$5,$6,'sla timeout')
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .bind(approval_id)
    .bind(current_step)
    .bind(actor)
    .bind(auth.ctx.actor.on_behalf_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        actor,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.approval.escalate",
        "approval",
        &id,
        serde_json::json!({ "from_step": current_step }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_approval_dto(&mut tx, org_id, approval_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "approval"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// POST bulk decide
#[utoipa::path(post, path = "/api/v1/operations/approvals/bulk-decide", tag = "operations-approvals",
    request_body = BulkDecideRequest,
    responses((status = 200, body = BulkDecideResponse)))]
pub async fn bulk_decide(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<BulkDecideRequest>,
) -> Result<Json<BulkDecideResponse>, AppError> {
    let mut decided = Vec::new();
    let mut skipped = Vec::new();
    for id in &body.ids {
        match decide_approval(
            State(state.clone()),
            auth.clone(),
            Path(id.clone()),
            headers.clone(),
            Json(DecideApprovalRequest {
                approve: body.approve,
                comment: body.comment.clone(),
            }),
        )
        .await
        {
            Ok(Json(dto)) => decided.push(dto),
            Err(_) => skipped.push(id.clone()),
        }
    }
    Ok(Json(BulkDecideResponse { decided, skipped }))
}

/// POST delegation
#[utoipa::path(post, path = "/api/v1/operations/approvals/delegations", tag = "operations-approvals",
    request_body = CreateDelegationRequest,
    responses((status = 201, body = DelegationDto)))]
pub async fn create_delegation(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateDelegationRequest>,
) -> Result<(StatusCode, Json<DelegationDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_decide(),
        &request_id,
    )?;

    let to_user = if let Ok(u) = Uuid::parse_str(&body.to_user_id) {
        u
    } else {
        parse_public_id(IdKind::User, &body.to_user_id, &request_id)?
    };
    let approval_uuid = match body.approval_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::Approval, s, &request_id)?),
        None => None,
    };
    let ends_at = body
        .ends_at
        .as_deref()
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()
        .map_err(|_| validation(&request_id, "ends_at must be RFC3339"))?;

    let public_id = PublicId::generate(IdKind::ApprovalDelegation);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO operations_approval_delegation (
            id, org_id, public_id, from_user_id, to_user_id, approval_id, ends_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(actor)
    .bind(to_user)
    .bind(approval_uuid)
    .bind(ends_at)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        actor,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.approval.delegate",
        "delegation",
        &public_id.as_str(),
        serde_json::json!({ "to_user_id": user_pub(to_user) }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    Ok((
        StatusCode::CREATED,
        Json(DelegationDto {
            id: public_id.as_str(),
            from_user_id: user_pub(actor),
            to_user_id: user_pub(to_user),
            approval_id: body.approval_id,
            starts_at: Utc::now().to_rfc3339(),
            ends_at: ends_at.map(|t| t.to_rfc3339()),
            revoked_at: None,
        }),
    ))
}

#[utoipa::path(get, path = "/api/v1/operations/approvals/delegations", tag = "operations-approvals",
    responses((status = 200)))]
pub async fn list_delegations(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<serde_json::Value>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_read(),
        &request_id,
    )?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        Uuid,
        Uuid,
        Option<Uuid>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        r#"
        SELECT public_id, from_user_id, to_user_id, approval_id, starts_at, ends_at, revoked_at
        FROM operations_approval_delegation
        WHERE org_id = $1 AND (from_user_id = $2 OR to_user_id = $2) AND revoked_at IS NULL
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .bind(org_id)
    .bind(actor)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    let items: Vec<_> = rows
        .into_iter()
        .map(
            |(public_id, from_user_id, to_user_id, approval_id, starts_at, ends_at, revoked_at)| {
                serde_json::json!({
                    "id": public_id,
                    "from_user_id": user_pub(from_user_id),
                    "to_user_id": user_pub(to_user_id),
                    "approval_id": approval_id.map(|u| PublicId::new(IdKind::Approval, u).as_str()),
                    "starts_at": starts_at.to_rfc3339(),
                    "ends_at": ends_at.map(|t| t.to_rfc3339()),
                    "revoked_at": revoked_at.map(|t| t.to_rfc3339()),
                })
            },
        )
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

/// Policy list/create/get/update
#[utoipa::path(get, path = "/api/v1/operations/approval-policies", tag = "operations-approvals",
    responses((status = 200, body = PolicyListResponse)))]
pub async fn list_policies(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PolicyListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_read(),
        &request_id,
    )?;
    let _ = ensure_default_policies(&state.pool, org_id).await;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let rows: Vec<PolicyRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, subject_type, is_active, current_version, created_at, updated_at
        FROM operations_approval_policy WHERE org_id = $1 ORDER BY created_at
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let mut items = Vec::new();
    for row in rows {
        if let Some(dto) = fetch_policy_dto(&mut tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?
        {
            items.push(dto);
        }
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(PolicyListResponse { items }))
}

#[utoipa::path(post, path = "/api/v1/operations/approval-policies", tag = "operations-approvals",
    request_body = CreatePolicyRequest,
    responses((status = 201, body = ApprovalPolicyDto)))]
pub async fn create_policy(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreatePolicyRequest>,
) -> Result<(StatusCode, Json<ApprovalPolicyDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_policy_manage(),
        &request_id,
    )?;
    if body.definition.steps.is_empty() {
        return Err(validation(
            &request_id,
            "policy must have at least one step",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let (policy_id, _) = insert_policy(
        &mut tx,
        org_id,
        &body.name,
        &body.subject_type,
        &body.definition,
        actor,
    )
    .await
    .map_err(internal(&request_id))?;
    let dto = fetch_policy_dto(&mut tx, org_id, policy_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "policy"))?;
    insert_audit(
        &mut *tx,
        org_id,
        actor,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.approval.policy.create",
        "approval_policy",
        &dto.id,
        serde_json::json!({ "version": 1 }),
    )
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

#[utoipa::path(get, path = "/api/v1/operations/approval-policies/{id}", tag = "operations-approvals",
    responses((status = 200, body = ApprovalPolicyDto)))]
pub async fn get_policy(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ApprovalPolicyDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let policy_id = parse_public_id(IdKind::ApprovalPolicy, &id, &request_id)?;
    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_read(),
        &request_id,
    )?;
    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let dto = fetch_policy_dto(&mut tx, org_id, policy_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "policy"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

#[utoipa::path(post, path = "/api/v1/operations/approval-policies/{id}", tag = "operations-approvals",
    request_body = UpdatePolicyRequest,
    responses((status = 200, body = ApprovalPolicyDto)))]
pub async fn update_policy(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdatePolicyRequest>,
) -> Result<Json<ApprovalPolicyDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let policy_id = parse_public_id(IdKind::ApprovalPolicy, &id, &request_id)?;
    let actor = auth.ctx.actor.user_id;
    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_approval_policy_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(name) = &body.name {
        sqlx::query(
            "UPDATE operations_approval_policy SET name = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(policy_id)
        .bind(name)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }
    if let Some(active) = body.is_active {
        sqlx::query(
            "UPDATE operations_approval_policy SET is_active = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(policy_id)
        .bind(active)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }
    if let Some(def) = &body.definition {
        let ver = publish_new_version(&mut tx, org_id, policy_id, def, actor)
            .await
            .map_err(internal(&request_id))?;
        insert_audit(
            &mut *tx,
            org_id,
            actor,
            auth.ctx.actor.on_behalf_of,
            auth.ctx.actor.is_ai,
            "operations.approval.policy.publish",
            "approval_policy",
            &id,
            serde_json::json!({ "version": ver }),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    let dto = fetch_policy_dto(&mut tx, org_id, policy_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "policy"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
