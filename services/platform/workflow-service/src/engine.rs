//! Runtime execution helpers — bounds, Temporal workflow id, step advancement.
//!
//! Activities call existing service APIs with `on_behalf_of` the recorded actor.
//! Full Temporal SDK registration remains best-effort (catalogue state machine
//! in workflow-host); this module owns durable instance state in Postgres.

#![allow(clippy::too_many_arguments)]

use chrono::{Duration, Utc};
use companyos_authz::Principal;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::definition::{HumanStepKind, WorkflowGraph, WorkflowNode};
use crate::permissions::enforce_action_permission;
use crate::types::OrgBoundsDto;

pub const DEFAULT_MAX_CONCURRENT: i32 = 50;
pub const DEFAULT_MAX_STEPS: i32 = 100;

/// Temporal workflow id: `{org_id}:UserWorkflow:{definition_id}:{instance_id}`.
pub fn user_workflow_id(
    org_public: &str,
    definition_public: &str,
    instance_public: &str,
) -> String {
    format!("{org_public}:UserWorkflow:{definition_public}:{instance_public}")
}

pub async fn load_or_default_bounds(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
) -> Result<OrgBoundsDto, sqlx::Error> {
    let row: Option<(i32, i32)> = sqlx::query_as(
        r#"
        SELECT max_concurrent, max_steps_per_instance
        FROM workflow_org_bounds WHERE org_id = $1
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(match row {
        Some((c, s)) => OrgBoundsDto {
            max_concurrent: c,
            max_steps_per_instance: s,
        },
        None => OrgBoundsDto {
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            max_steps_per_instance: DEFAULT_MAX_STEPS,
        },
    })
}

pub async fn count_active_instances(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let (n,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint FROM workflow_instance
        WHERE org_id = $1 AND status IN ('running', 'waiting')
        "#,
    )
    .bind(org_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(n)
}

pub async fn enforce_concurrency_cap(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    request_id: &str,
) -> Result<OrgBoundsDto, AppError> {
    let bounds = load_or_default_bounds(tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let active = count_active_instances(tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    if active >= i64::from(bounds.max_concurrent) {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            format!(
                "org concurrency cap exceeded: {active} active instances (max {})",
                bounds.max_concurrent
            ),
        ));
    }
    Ok(bounds)
}

/// Advance one step synchronously for simple action/end graphs (tests + start).
/// Timer/human steps leave the instance in `waiting`.
pub async fn advance_instance(
    pool: &PgPool,
    org_id: OrgId,
    instance_id: Uuid,
    graph: &WorkflowGraph,
    principal: &Principal,
    actor: &Actor,
    request_id: &str,
    dry_run_http: bool,
) -> Result<(), AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    companyos_tenancy::set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let bounds = load_or_default_bounds(&mut tx, org_id.as_uuid())
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    #[allow(clippy::type_complexity)]
    let row: Option<(
        String,
        Option<String>,
        i32,
        String,
        serde_json::Value,
        Uuid,
        Uuid,
        i32,
    )> = sqlx::query_as(
        r#"
        SELECT public_id, current_node_id, step_count, status, trigger_payload,
               definition_id, version_id, version_number
        FROM workflow_instance WHERE id = $1 AND org_id = $2
        FOR UPDATE
        "#,
    )
    .bind(instance_id)
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((
        instance_public,
        current_node,
        step_count,
        status,
        payload,
        def_uuid,
        ver_uuid,
        ver_num,
    )) = row
    else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "workflow instance not found",
        ));
    };

    if status != "running" {
        return Ok(());
    }

    let Some(node_id) = current_node else {
        return fail_instance(
            &mut tx,
            org_id,
            instance_id,
            &instance_public,
            def_uuid,
            ver_uuid,
            ver_num,
            actor,
            "missing current_node_id",
            request_id,
        )
        .await;
    };

    if step_count >= bounds.max_steps_per_instance {
        return fail_instance(
            &mut tx,
            org_id,
            instance_id,
            &instance_public,
            def_uuid,
            ver_uuid,
            ver_num,
            actor,
            &format!(
                "iteration/step cap exceeded ({} steps); run failed closed",
                bounds.max_steps_per_instance
            ),
            request_id,
        )
        .await;
    }

    let Some(node) = graph.node(&node_id) else {
        return fail_instance(
            &mut tx,
            org_id,
            instance_id,
            &instance_public,
            def_uuid,
            ver_uuid,
            ver_num,
            actor,
            &format!("unknown node '{node_id}'"),
            request_id,
        )
        .await;
    };

    let next_step = step_count + 1;

    match node {
        WorkflowNode::End { id } => {
            record_step(
                &mut tx,
                org_id.as_uuid(),
                instance_id,
                next_step,
                id,
                "end",
                None,
                "ok",
                json!({"completed": true}),
                None,
                None,
            )
            .await?;
            complete_instance(
                &mut tx,
                org_id,
                instance_id,
                &instance_public,
                def_uuid,
                ver_uuid,
                ver_num,
                actor,
                request_id,
            )
            .await?;
        }
        WorkflowNode::Action {
            id,
            action,
            params,
            next,
        } => {
            let perm = match enforce_action_permission(principal, action, request_id) {
                Ok(p) => p,
                Err(e) => {
                    record_step(
                        &mut tx,
                        org_id.as_uuid(),
                        instance_id,
                        next_step,
                        id,
                        "action",
                        Some(action),
                        "denied",
                        json!({"error": e.detail}),
                        Some(action),
                        Some(false),
                    )
                    .await?;
                    return fail_instance(
                        &mut tx,
                        org_id,
                        instance_id,
                        &instance_public,
                        def_uuid,
                        ver_uuid,
                        ver_num,
                        actor,
                        &e.detail,
                        request_id,
                    )
                    .await;
                }
            };

            if !dry_run_http {
                if let Err(e) =
                    crate::activities::execute_action(action, params, &payload, org_id, actor).await
                {
                    record_step(
                        &mut tx,
                        org_id.as_uuid(),
                        instance_id,
                        next_step,
                        id,
                        "action",
                        Some(action),
                        "failed",
                        json!({"error": e.to_string()}),
                        Some(perm.as_str()),
                        Some(true),
                    )
                    .await?;
                    return fail_instance(
                        &mut tx,
                        org_id,
                        instance_id,
                        &instance_public,
                        def_uuid,
                        ver_uuid,
                        ver_num,
                        actor,
                        &e.to_string(),
                        request_id,
                    )
                    .await;
                }
            }

            record_step(
                &mut tx,
                org_id.as_uuid(),
                instance_id,
                next_step,
                id,
                "action",
                Some(action),
                "ok",
                json!({"dry_run_http": dry_run_http}),
                Some(perm.as_str()),
                Some(true),
            )
            .await?;

            sqlx::query(
                r#"
                UPDATE workflow_instance
                SET current_node_id = $1, step_count = $2, updated_at = now()
                WHERE id = $3 AND org_id = $4
                "#,
            )
            .bind(next.as_ref())
            .bind(next_step)
            .bind(instance_id)
            .bind(org_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        }
        WorkflowNode::Condition {
            id,
            path,
            equals,
            then_next,
            else_next,
        } => {
            let path_key = path.strip_prefix("payload.").unwrap_or(path);
            let matched = payload.pointer(&format!("/{path_key}")).or_else(|| {
                let mut cur = &payload;
                for part in path_key.split('.') {
                    cur = cur.get(part)?;
                }
                Some(cur)
            }) == Some(equals);
            let chosen = if matched { then_next } else { else_next };
            record_step(
                &mut tx,
                org_id.as_uuid(),
                instance_id,
                next_step,
                id,
                "condition",
                None,
                "ok",
                json!({"matched": matched, "next": chosen}),
                None,
                None,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE workflow_instance
                SET current_node_id = $1, step_count = $2, updated_at = now()
                WHERE id = $3 AND org_id = $4
                "#,
            )
            .bind(chosen)
            .bind(next_step)
            .bind(instance_id)
            .bind(org_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        }
        WorkflowNode::Branch {
            id,
            arms,
            default_next,
        } => {
            let mut chosen = default_next.clone();
            for arm in arms {
                let path = arm.path.strip_prefix("payload.").unwrap_or(&arm.path);
                let mut cur = &payload;
                let mut ok = true;
                for part in path.split('.') {
                    match cur.get(part) {
                        Some(v) => cur = v,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok && cur == &arm.equals {
                    chosen = arm.next.clone();
                    break;
                }
            }
            record_step(
                &mut tx,
                org_id.as_uuid(),
                instance_id,
                next_step,
                id,
                "branch",
                None,
                "ok",
                json!({"next": chosen}),
                None,
                None,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE workflow_instance
                SET current_node_id = $1, step_count = $2, updated_at = now()
                WHERE id = $3 AND org_id = $4
                "#,
            )
            .bind(&chosen)
            .bind(next_step)
            .bind(instance_id)
            .bind(org_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        }
        WorkflowNode::Timer {
            id,
            duration_secs,
            next,
        } => {
            let until = Utc::now() + Duration::seconds(*duration_secs as i64);
            record_step(
                &mut tx,
                org_id.as_uuid(),
                instance_id,
                next_step,
                id,
                "timer",
                None,
                "waiting",
                json!({"until": until.to_rfc3339(), "resume_node": next}),
                None,
                None,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE workflow_instance
                SET status = 'waiting', current_node_id = $1, step_count = $2,
                    waiting_until = $3, updated_at = now()
                WHERE id = $4 AND org_id = $5
                "#,
            )
            .bind(next)
            .bind(next_step)
            .bind(until)
            .bind(instance_id)
            .bind(org_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            emit_instance_event(
                &mut tx,
                org_id,
                actor,
                "waiting",
                &instance_public,
                def_uuid,
                ver_uuid,
                ver_num,
            )
            .await?;
        }
        WorkflowNode::Human {
            id,
            kind,
            params,
            on_approve,
            on_reject: _,
        } => {
            if matches!(kind, HumanStepKind::Approval) {
                if let Err(e) = enforce_action_permission(principal, "start_approval", request_id) {
                    record_step(
                        &mut tx,
                        org_id.as_uuid(),
                        instance_id,
                        next_step,
                        id,
                        "human",
                        Some("start_approval"),
                        "denied",
                        json!({"error": e.detail}),
                        Some("operations.approval.read"),
                        Some(false),
                    )
                    .await?;
                    return fail_instance(
                        &mut tx,
                        org_id,
                        instance_id,
                        &instance_public,
                        def_uuid,
                        ver_uuid,
                        ver_num,
                        actor,
                        &e.detail,
                        request_id,
                    )
                    .await;
                }
                if !dry_run_http {
                    let _ = crate::activities::execute_action(
                        "start_approval",
                        params,
                        &payload,
                        org_id,
                        actor,
                    )
                    .await;
                }
            }
            record_step(
                &mut tx,
                org_id.as_uuid(),
                instance_id,
                next_step,
                id,
                "human",
                Some(match kind {
                    HumanStepKind::Approval => "start_approval",
                    HumanStepKind::Inbox => "inbox",
                }),
                "waiting",
                json!({"resume_on_approve": on_approve}),
                None,
                None,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE workflow_instance
                SET status = 'waiting', current_node_id = $1, step_count = $2, updated_at = now()
                WHERE id = $3 AND org_id = $4
                "#,
            )
            .bind(on_approve)
            .bind(next_step)
            .bind(instance_id)
            .bind(org_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            emit_instance_event(
                &mut tx,
                org_id,
                actor,
                "waiting",
                &instance_public,
                def_uuid,
                ver_uuid,
                ver_num,
            )
            .await?;
        }
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}

async fn record_step(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    instance_id: Uuid,
    step_index: i32,
    node_id: &str,
    node_type: &str,
    action_key: Option<&str>,
    status: &str,
    detail: serde_json::Value,
    permission: Option<&str>,
    allowed: Option<bool>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO workflow_instance_step (
            id, org_id, instance_id, step_index, node_id, node_type, action_key,
            status, detail, permission_checked, permission_allowed
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .bind(instance_id)
    .bind(step_index)
    .bind(node_id)
    .bind(node_type)
    .bind(action_key)
    .bind(status)
    .bind(detail)
    .bind(permission)
    .bind(allowed)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, "step", e.to_string()))?;
    Ok(())
}

async fn fail_instance(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    instance_id: Uuid,
    instance_public: &str,
    def_uuid: Uuid,
    ver_uuid: Uuid,
    ver_num: i32,
    actor: &Actor,
    message: &str,
    request_id: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE workflow_instance
        SET status = 'failed', error_message = $1, completed_at = now(), updated_at = now()
        WHERE id = $2 AND org_id = $3
        "#,
    )
    .bind(message)
    .bind(instance_id)
    .bind(org_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    emit_instance_event(
        tx,
        org_id,
        actor,
        "failed",
        instance_public,
        def_uuid,
        ver_uuid,
        ver_num,
    )
    .await?;
    Ok(())
}

async fn complete_instance(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    instance_id: Uuid,
    instance_public: &str,
    def_uuid: Uuid,
    ver_uuid: Uuid,
    ver_num: i32,
    actor: &Actor,
    request_id: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE workflow_instance
        SET status = 'completed', current_node_id = NULL, completed_at = now(), updated_at = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(instance_id)
    .bind(org_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    emit_instance_event(
        tx,
        org_id,
        actor,
        "completed",
        instance_public,
        def_uuid,
        ver_uuid,
        ver_num,
    )
    .await?;
    Ok(())
}

async fn emit_instance_event(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    actor: &Actor,
    event_type: &str,
    instance_public: &str,
    def_uuid: Uuid,
    ver_uuid: Uuid,
    ver_num: i32,
) -> Result<(), AppError> {
    let def_public = PublicId::new(IdKind::WorkflowDefinition, def_uuid).as_str();
    let ver_public = PublicId::new(IdKind::WorkflowVersion, ver_uuid).as_str();
    let envelope = EventEnvelope::new(
        org_id,
        Context::Workflow,
        "instance",
        event_type,
        1,
        actor.clone(),
        json!({
            "id": instance_public,
            "definition_id": def_public,
            "version_id": ver_public,
            "version": ver_num,
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "outbox", e.to_string()))?;
    Ok(())
}

/// Drive an instance until waiting/terminal (bounded by max steps).
pub async fn run_until_idle(
    pool: &PgPool,
    org_id: OrgId,
    instance_id: Uuid,
    graph: &WorkflowGraph,
    principal: &Principal,
    actor: &Actor,
    request_id: &str,
    dry_run_http: bool,
) -> Result<(), AppError> {
    for _ in 0..DEFAULT_MAX_STEPS + 5 {
        let status: Option<String> = {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            companyos_tenancy::set_session_org_id(&mut tx, org_id)
                .await
                .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            let s: Option<(String,)> = sqlx::query_as(
                "SELECT status FROM workflow_instance WHERE id = $1 AND org_id = $2",
            )
            .bind(instance_id)
            .bind(org_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            tx.commit()
                .await
                .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            s.map(|x| x.0)
        };
        match status.as_deref() {
            Some("running") => {
                advance_instance(
                    pool,
                    org_id,
                    instance_id,
                    graph,
                    principal,
                    actor,
                    request_id,
                    dry_run_http,
                )
                .await?;
            }
            _ => break,
        }
    }
    Ok(())
}
