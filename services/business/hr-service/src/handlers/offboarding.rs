//! POST `/api/v1/people/employees/{id}/offboard` + GET access-audit.
//!
//! Steps: mark_offboarding → reassign_reports → create_return_tasks →
//! revoke_access → terminate → notify. `fail_after` injects compensation.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::employees::{
    enforce_employee_scope, fetch_employee_row, insert_timeline, parse_optional_date,
};
use super::{conflict, internal, not_found, parse_public_id, user_public, validation};
use crate::access::{audit_access_cleared, revoke_all_access_paths};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    AccessAuditResponse, AccessChecklistItem, OffboardRequest, OffboardResponse, TaskDto,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/people/employees/{id}/offboard",
            post(offboard),
        )
        .route(
            "/api/v1/people/employees/{id}/access-audit",
            get(access_audit),
        )
}

const STEPS: &[&str] = &[
    "mark_offboarding",
    "reassign_reports",
    "create_return_tasks",
    "revoke_access",
    "terminate",
    "notify",
];

fn check_fail_after(fail_after: Option<&str>, step: &str, request_id: &str) -> Result<(), AppError> {
    if fail_after == Some(step) {
        Err(conflict(
            request_id,
            format!("injected failure after step '{step}'"),
        ))
    } else {
        Ok(())
    }
}

async fn insert_offboard_task(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    kind: &str,
    title: &str,
    workflow_id: &str,
    assignee: Option<Uuid>,
) -> Result<TaskDto, sqlx::Error> {
    let pid = PublicId::generate(IdKind::HrTask);
    let id = pid.uuid();
    sqlx::query(
        r#"
        INSERT INTO people_task (
            id, org_id, public_id, employee_id, kind, title, status, assignee_user_id, workflow_id
        ) VALUES ($1,$2,$3,$4,$5,$6,'pending',$7,$8)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(pid.as_str())
    .bind(employee_id)
    .bind(kind)
    .bind(title)
    .bind(assignee)
    .bind(workflow_id)
    .execute(&mut **tx)
    .await?;
    Ok(TaskDto {
        id: pid.as_str(),
        employee_id: PublicId::new(IdKind::Employee, employee_id).as_str(),
        kind: kind.to_string(),
        title: title.to_string(),
        status: "pending".into(),
        assignee_user_id: assignee.map(user_public),
        due_at: None,
        completed_at: None,
        workflow_id: Some(workflow_id.to_string()),
    })
}

struct ReassignUndo {
    report_id: Uuid,
    previous_manager: Option<Uuid>,
}

#[allow(clippy::too_many_arguments)]
async fn compensate_offboarding(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    workflow_id: &str,
    actor: Uuid,
    fail_step: &str,
    previous_status: &str,
    reassigns: &[ReassignUndo],
    revoked_user: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE people_task
        SET status = 'compensated', updated_at = now()
        WHERE org_id = $1 AND employee_id = $2 AND workflow_id = $3 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(workflow_id)
    .execute(&mut **tx)
    .await?;

    for r in reassigns {
        sqlx::query(
            r#"
            UPDATE people_employee
            SET manager_employee_id = $3, updated_at = now(), version = version + 1
            WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
            "#,
        )
        .bind(org_id)
        .bind(r.report_id)
        .bind(r.previous_manager)
        .execute(&mut **tx)
        .await?;
    }

    if let Some(uid) = revoked_user {
        sqlx::query(
            r#"
            UPDATE membership
            SET status = 'active', revoked_at = NULL, updated_at = now(),
                policy_version = policy_version + 1
            WHERE org_id = $1 AND user_id = $2 AND status = 'revoked'
            "#,
        )
        .bind(org_id)
        .bind(uid)
        .execute(&mut **tx)
        .await?;
        // Best-effort: clear session revoke markers applied in this offboard.
        sqlx::query(
            r#"
            UPDATE auth_session
            SET revoked_at = NULL, revoke_reason = NULL
            WHERE user_id = $1 AND org_id = $2
              AND revoke_reason = 'employee_offboarding'
            "#,
        )
        .bind(uid)
        .bind(org_id)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query(
        r#"
        UPDATE people_employee
        SET status = $3, end_date = NULL, updated_at = now(), version = version + 1
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(previous_status)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE people_workflow_run
        SET status = 'compensated', error_detail = $3, updated_at = now()
        WHERE org_id = $1 AND workflow_id = $2
        "#,
    )
    .bind(org_id)
    .bind(workflow_id)
    .bind(format!("compensated after fail_after={fail_step}"))
    .execute(&mut **tx)
    .await?;

    insert_timeline(
        tx,
        org_id,
        employee_id,
        "employee.offboarding.compensated",
        &format!("Offboarding compensated after step '{fail_step}'"),
        serde_json::json!({ "fail_after": fail_step, "workflow_id": workflow_id }),
        Some(actor),
    )
    .await?;
    Ok(())
}

/// POST /api/v1/people/employees/{id}/offboard
#[utoipa::path(
    post,
    path = "/api/v1/people/employees/{id}/offboard",
    tag = "people-offboarding",
    request_body = OffboardRequest,
    params(("id" = String, Path)),
    responses((status = 200, body = OffboardResponse), (status = 404), (status = 409))
)]
pub async fn offboard(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<OffboardRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers)
        .ok_or_else(|| validation(&request_id, "Idempotency-Key header is required"))?;

    if let Some(ref fa) = body.fail_after {
        if !STEPS.contains(&fa.as_str()) {
            return Err(validation(
                &request_id,
                format!("fail_after must be one of: {}", STEPS.join("|")),
            ));
        }
    }

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_employee_offboard(),
        &request_id,
    )?;

    let end_date = parse_optional_date(body.end_date.as_deref(), "end_date", &request_id)?;
    let reassign_to = match body.reassign_manager_to.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            Some(parse_public_id(IdKind::Employee, s, &request_id)?)
        }
        _ => None,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some((status_code, stored)) =
        idempotency::get(&mut *tx, org_id, "employee.offboard", &idem_key)
            .await
            .map_err(internal(&request_id))?
    {
        tx.commit().await.map_err(internal(&request_id))?;
        let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::OK);
        return Ok((code, Json(stored)).into_response());
    }

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_offboard(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let previous_status = emp.status.clone();
    let emp_public = emp.public_id.clone();
    let linked_user = emp.user_id;
    let org_public = auth.ctx.org_id.to_public().as_str();
    let workflow_id = format!("{org_public}:EmployeeOffboarding:{emp_public}");
    let fail_after = body.fail_after.as_deref();
    let mut reassigns: Vec<ReassignUndo> = Vec::new();
    let mut checklist: Vec<AccessChecklistItem> = Vec::new();
    let mut revoked_user: Option<Uuid> = None;

    sqlx::query(
        r#"
        INSERT INTO people_workflow_run (
            id, org_id, employee_id, workflow_type, workflow_id, status
        ) VALUES ($1,$2,$3,'EmployeeOffboarding',$4,'running')
        ON CONFLICT (org_id, workflow_id) DO UPDATE SET updated_at = now()
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .bind(employee_id)
    .bind(&workflow_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    // ---- mark_offboarding ----
    sqlx::query(
        r#"
        UPDATE people_employee
        SET status = 'offboarding', end_date = COALESCE($3, end_date),
            updated_at = now(), version = version + 1
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(end_date)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Err(e) = check_fail_after(fail_after, "mark_offboarding", &request_id) {
        compensate_offboarding(
            &mut tx,
            org_id,
            employee_id,
            &workflow_id,
            auth.ctx.actor.user_id,
            "mark_offboarding",
            &previous_status,
            &reassigns,
            revoked_user,
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- reassign_reports ----
    if let Some(new_mgr) = reassign_to {
        if new_mgr == employee_id {
            return Err(validation(
                &request_id,
                "reassign_manager_to must not be the offboarding employee",
            ));
        }
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM people_employee WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(new_mgr)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if exists.is_none() {
            return Err(validation(&request_id, "reassign_manager_to not found"));
        }

        let reports: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(
            r#"
            SELECT id, manager_employee_id FROM people_employee
            WHERE org_id = $1 AND manager_employee_id = $2 AND deleted_at IS NULL
            "#,
        )
        .bind(org_id)
        .bind(employee_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        for (rid, prev) in reports {
            reassigns.push(ReassignUndo {
                report_id: rid,
                previous_manager: prev,
            });
            sqlx::query(
                r#"
                UPDATE people_employee
                SET manager_employee_id = $3, updated_at = now(), version = version + 1
                WHERE org_id = $1 AND id = $2
                "#,
            )
            .bind(org_id)
            .bind(rid)
            .bind(new_mgr)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }
    }
    if let Err(e) = check_fail_after(fail_after, "reassign_reports", &request_id) {
        compensate_offboarding(
            &mut tx,
            org_id,
            employee_id,
            &workflow_id,
            auth.ctx.actor.user_id,
            "reassign_reports",
            &previous_status,
            &reassigns,
            revoked_user,
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- create_return_tasks ----
    let assets: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT public_id, label FROM people_asset
        WHERE org_id = $1 AND employee_id = $2 AND deleted_at IS NULL
          AND status IN ('assigned', 'pending_return')
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for (apid, label) in &assets {
        sqlx::query(
            r#"
            UPDATE people_asset SET status = 'pending_return', updated_at = now()
            WHERE org_id = $1 AND public_id = $2
            "#,
        )
        .bind(org_id)
        .bind(apid)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        let _ = insert_offboard_task(
            &mut tx,
            org_id,
            employee_id,
            "asset",
            &format!("Return asset: {label}"),
            &workflow_id,
            Some(auth.ctx.actor.user_id),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    let docs: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT public_id, title FROM people_document
        WHERE org_id = $1 AND employee_id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for (_dpid, title) in &docs {
        let _ = insert_offboard_task(
            &mut tx,
            org_id,
            employee_id,
            "document",
            &format!("Return / archive document: {title}"),
            &workflow_id,
            Some(auth.ctx.actor.user_id),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    let _ = insert_offboard_task(
        &mut tx,
        org_id,
        employee_id,
        "offboarding",
        body.reason
            .as_deref()
            .unwrap_or("Complete offboarding checklist"),
        &workflow_id,
        Some(auth.ctx.actor.user_id),
    )
    .await
    .map_err(internal(&request_id))?;

    if let Err(e) = check_fail_after(fail_after, "create_return_tasks", &request_id) {
        compensate_offboarding(
            &mut tx,
            org_id,
            employee_id,
            &workflow_id,
            auth.ctx.actor.user_id,
            "create_return_tasks",
            &previous_status,
            &reassigns,
            revoked_user,
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- revoke_access ----
    if let Some(uid) = linked_user {
        checklist = revoke_all_access_paths(&mut tx, org_id, uid, &request_id).await?;
        revoked_user = Some(uid);
    } else {
        checklist.push(AccessChecklistItem {
            path: "membership".into(),
            cleared: true,
            detail: "no linked user_id — skip access revoke".into(),
        });
    }
    if let Err(e) = check_fail_after(fail_after, "revoke_access", &request_id) {
        compensate_offboarding(
            &mut tx,
            org_id,
            employee_id,
            &workflow_id,
            auth.ctx.actor.user_id,
            "revoke_access",
            &previous_status,
            &reassigns,
            revoked_user,
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- terminate ----
    sqlx::query(
        r#"
        UPDATE people_employee
        SET status = 'terminated', end_date = COALESCE($3, end_date, CURRENT_DATE),
            updated_at = now(), version = version + 1
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .bind(end_date)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Err(e) = check_fail_after(fail_after, "terminate", &request_id) {
        compensate_offboarding(
            &mut tx,
            org_id,
            employee_id,
            &workflow_id,
            auth.ctx.actor.user_id,
            "terminate",
            &previous_status,
            &reassigns,
            revoked_user,
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- notify ----
    insert_timeline(
        &mut tx,
        org_id,
        employee_id,
        "employee.offboarded",
        &format!("Employee {emp_public} offboarded"),
        serde_json::json!({
            "workflow_id": workflow_id,
            "reason": body.reason,
        }),
        Some(auth.ctx.actor.user_id),
    )
    .await
    .map_err(internal(&request_id))?;

    if let Err(e) = check_fail_after(fail_after, "notify", &request_id) {
        compensate_offboarding(
            &mut tx,
            org_id,
            employee_id,
            &workflow_id,
            auth.ctx.actor.user_id,
            "notify",
            &previous_status,
            &reassigns,
            revoked_user,
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "employee",
        "offboarded",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": emp_public,
            "workflow_id": workflow_id,
            "user_id": linked_user.map(user_public),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE people_workflow_run
        SET status = 'completed', updated_at = now()
        WHERE org_id = $1 AND workflow_id = $2
        "#,
    )
    .bind(org_id)
    .bind(&workflow_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.employee.offboard",
        "employee",
        &emp_public,
        serde_json::json!({ "workflow_id": workflow_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    let updated = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;

    let resp = OffboardResponse {
        employee: updated.into_directory_dto(),
        workflow_id: workflow_id.clone(),
        checklist,
        status: "terminated".into(),
    };

    idempotency::put(
        &mut *tx,
        org_id,
        "employee.offboard",
        &idem_key,
        200,
        serde_json::to_value(&resp).unwrap_or_default(),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::OK, Json(resp)).into_response())
}

/// GET /api/v1/people/employees/{id}/access-audit
#[utoipa::path(
    get,
    path = "/api/v1/people/employees/{id}/access-audit",
    tag = "people-offboarding",
    params(("id" = String, Path)),
    responses((status = 200, body = AccessAuditResponse), (status = 404))
)]
pub async fn access_audit(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<AccessAuditResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_employee_offboard(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_offboard(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let checklist = if let Some(uid) = emp.user_id {
        audit_access_cleared(&mut tx, org_id, uid)
            .await
            .map_err(internal(&request_id))?
    } else {
        vec![AccessChecklistItem {
            path: "membership".into(),
            cleared: true,
            detail: "no linked user_id".into(),
        }]
    };
    let all_cleared = checklist.iter().all(|c| c.cleared);

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(AccessAuditResponse {
        employee_id: emp.public_id,
        user_id: emp.user_id.map(user_public),
        checklist,
        all_cleared,
    }))
}
