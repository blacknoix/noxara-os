//! POST `/api/v1/people/employees/onboard` — EmployeeOnboarding saga (in-process).
//!
//! Steps: create_employee → assign_role → allocate_assets → create_documents →
//! create_tasks → notify. `fail_after` injects a failure after the named step
//! for compensation tests.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::employees::{
    fetch_employee_row, insert_timeline, parse_department_link, parse_optional_date,
    resolve_manager_id, EmployeeRow,
};
use super::{conflict, internal, parse_user_ref, user_public, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{HrTaskDto, OnboardRequest, OnboardResponse};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/people/employees/onboard", post(onboard))
}

const STEPS: &[&str] = &[
    "create_employee",
    "assign_role",
    "allocate_assets",
    "create_documents",
    "create_tasks",
    "notify",
];

fn check_fail_after(
    fail_after: Option<&str>,
    step: &str,
    request_id: &str,
) -> Result<(), AppError> {
    if fail_after == Some(step) {
        Err(conflict(
            request_id,
            format!("injected failure after step '{step}'"),
        ))
    } else {
        Ok(())
    }
}

async fn insert_task(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    kind: &str,
    title: &str,
    workflow_id: &str,
    assignee: Option<Uuid>,
) -> Result<HrTaskDto, sqlx::Error> {
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
    Ok(HrTaskDto {
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

async fn compensate_onboarding(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    workflow_id: &str,
    actor: Uuid,
    fail_step: &str,
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

    // Soft-cancel path: draft status (cancelled not in CHECK; draft signals aborted onboard).
    sqlx::query(
        r#"
        UPDATE people_employee
        SET status = 'draft', updated_at = now(), version = version + 1
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
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
        "employee.onboarding.compensated",
        &format!("Onboarding compensated after step '{fail_step}'"),
        serde_json::json!({ "fail_after": fail_step, "workflow_id": workflow_id }),
        Some(actor),
    )
    .await?;
    Ok(())
}

/// POST /api/v1/people/employees/onboard
#[utoipa::path(
    post,
    path = "/api/v1/people/employees/onboard",
    tag = "people-onboarding",
    request_body = OnboardRequest,
    responses((status = 201, body = OnboardResponse), (status = 409))
)]
pub async fn onboard(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<OnboardRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
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
        perms::hr_employee_onboard(),
        &request_id,
    )?;

    if body.display_name.trim().is_empty() {
        return Err(validation(&request_id, "display_name must not be empty"));
    }

    let user_id = match body.user_id.as_deref() {
        Some(s) => Some(parse_user_ref(s, &request_id)?),
        None => None,
    };
    let owner_user_id = user_id.unwrap_or(auth.ctx.actor.user_id);
    let dept = parse_department_link(body.department_id.as_deref(), &request_id)?;
    let start_date = parse_optional_date(body.start_date.as_deref(), "start_date", &request_id)?;

    let public_id = PublicId::generate(IdKind::Employee);
    let emp_uuid = public_id.uuid();
    let org_public = auth.ctx.org_id.to_public().as_str();
    let workflow_id = format!("{org_public}:EmployeeOnboarding:{}", public_id.as_str());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some((status_code, stored)) =
        idempotency::get(&mut *tx, org_id, "employee.onboard", &idem_key)
            .await
            .map_err(internal(&request_id))?
    {
        tx.commit().await.map_err(internal(&request_id))?;
        let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
        return Ok((code, Json(stored)).into_response());
    }

    let fail_after = body.fail_after.as_deref();
    let mut tasks: Vec<HrTaskDto> = Vec::new();

    // ---- create_employee ----
    let manager_id = resolve_manager_id(
        &mut tx,
        org_id,
        body.manager_employee_id.as_deref(),
        &request_id,
    )
    .await?;
    let (department_id, department_public_id) = match &dept {
        Some((u, p)) => (Some(*u), Some(p.clone())),
        None => (None, None),
    };

    sqlx::query(
        r#"
        INSERT INTO people_employee (
            id, org_id, public_id, user_id, display_name, work_email, title, status,
            start_date, department_id, department_public_id, manager_employee_id, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,'onboarding',$8,$9,$10,$11,$12)
        "#,
    )
    .bind(emp_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(user_id)
    .bind(body.display_name.trim())
    .bind(&body.work_email)
    .bind(&body.title)
    .bind(start_date)
    .bind(department_id)
    .bind(&department_public_id)
    .bind(manager_id)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let wf_run_id = new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO people_workflow_run (
            id, org_id, employee_id, workflow_type, workflow_id, status
        ) VALUES ($1,$2,$3,'EmployeeOnboarding',$4,'running')
        "#,
    )
    .bind(wf_run_id)
    .bind(org_id)
    .bind(emp_uuid)
    .bind(&workflow_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Err(e) = check_fail_after(fail_after, "create_employee", &request_id) {
        compensate_onboarding(
            &mut tx,
            org_id,
            emp_uuid,
            &workflow_id,
            auth.ctx.actor.user_id,
            "create_employee",
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- assign_role ----
    if let (Some(uid), Some(role)) = (user_id, body.role.as_deref()) {
        if !role.trim().is_empty() {
            sqlx::query(
                r#"
                UPDATE membership
                SET role = $3, updated_at = now(), policy_version = policy_version + 1
                WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
                "#,
            )
            .bind(org_id)
            .bind(uid)
            .bind(role.trim())
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }
    }
    if let Err(e) = check_fail_after(fail_after, "assign_role", &request_id) {
        compensate_onboarding(
            &mut tx,
            org_id,
            emp_uuid,
            &workflow_id,
            auth.ctx.actor.user_id,
            "assign_role",
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- allocate_assets ----
    let asset_labels = body.asset_labels.clone().unwrap_or_default();
    for label in &asset_labels {
        if label.trim().is_empty() {
            continue;
        }
        let apid = PublicId::generate(IdKind::HrAsset);
        sqlx::query(
            r#"
            INSERT INTO people_asset (
                id, org_id, public_id, employee_id, label, status
            ) VALUES ($1,$2,$3,$4,$5,'assigned')
            "#,
        )
        .bind(apid.uuid())
        .bind(org_id)
        .bind(apid.as_str())
        .bind(emp_uuid)
        .bind(label.trim())
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        tasks.push(
            insert_task(
                &mut tx,
                org_id,
                emp_uuid,
                "asset",
                &format!("Confirm asset: {}", label.trim()),
                &workflow_id,
                Some(auth.ctx.actor.user_id),
            )
            .await
            .map_err(internal(&request_id))?,
        );
    }
    if let Err(e) = check_fail_after(fail_after, "allocate_assets", &request_id) {
        compensate_onboarding(
            &mut tx,
            org_id,
            emp_uuid,
            &workflow_id,
            auth.ctx.actor.user_id,
            "allocate_assets",
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- create_documents ----
    let doc_titles = body.document_titles.clone().unwrap_or_default();
    for title in &doc_titles {
        if title.trim().is_empty() {
            continue;
        }
        let dpid = PublicId::generate(IdKind::EmployeeDocument);
        sqlx::query(
            r#"
            INSERT INTO people_document (
                id, org_id, public_id, employee_id, title, doc_type, collected
            ) VALUES ($1,$2,$3,$4,$5,'onboarding',false)
            "#,
        )
        .bind(dpid.uuid())
        .bind(org_id)
        .bind(dpid.as_str())
        .bind(emp_uuid)
        .bind(title.trim())
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        tasks.push(
            insert_task(
                &mut tx,
                org_id,
                emp_uuid,
                "document",
                &format!("Collect document: {}", title.trim()),
                &workflow_id,
                Some(auth.ctx.actor.user_id),
            )
            .await
            .map_err(internal(&request_id))?,
        );
    }
    if let Err(e) = check_fail_after(fail_after, "create_documents", &request_id) {
        compensate_onboarding(
            &mut tx,
            org_id,
            emp_uuid,
            &workflow_id,
            auth.ctx.actor.user_id,
            "create_documents",
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // ---- create_tasks ----
    let task_titles = body.task_titles.clone().unwrap_or_else(|| {
        vec![
            "Complete onboarding checklist".into(),
            "Meet with manager".into(),
        ]
    });
    for title in &task_titles {
        if title.trim().is_empty() {
            continue;
        }
        tasks.push(
            insert_task(
                &mut tx,
                org_id,
                emp_uuid,
                "onboarding",
                title.trim(),
                &workflow_id,
                Some(auth.ctx.actor.user_id),
            )
            .await
            .map_err(internal(&request_id))?,
        );
    }
    if let Err(e) = check_fail_after(fail_after, "create_tasks", &request_id) {
        compensate_onboarding(
            &mut tx,
            org_id,
            emp_uuid,
            &workflow_id,
            auth.ctx.actor.user_id,
            "create_tasks",
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
        emp_uuid,
        "employee.onboarding.started",
        &format!("Onboarding started for {}", body.display_name.trim()),
        serde_json::json!({ "workflow_id": workflow_id }),
        Some(auth.ctx.actor.user_id),
    )
    .await
    .map_err(internal(&request_id))?;

    if let Err(e) = check_fail_after(fail_after, "notify", &request_id) {
        compensate_onboarding(
            &mut tx,
            org_id,
            emp_uuid,
            &workflow_id,
            auth.ctx.actor.user_id,
            "notify",
        )
        .await
        .map_err(internal(&request_id))?;
        tx.commit().await.map_err(internal(&request_id))?;
        return Err(e);
    }

    // Success path: emit created + onboarded events, mark workflow completed.
    let created = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "employee",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "display_name": body.display_name.trim(),
            "status": "onboarding",
            "user_id": user_id.map(user_public),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &created)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let onboarded = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "employee",
        "onboarded",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "workflow_id": workflow_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &onboarded)
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
        "hr.employee.onboard",
        "employee",
        &public_id.as_str(),
        serde_json::json!({ "workflow_id": workflow_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    let emp: EmployeeRow = fetch_employee_row(&mut tx, org_id, emp_uuid)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "employee missing after onboard",
            )
        })?;

    let resp = OnboardResponse {
        employee: emp.into_directory_dto(),
        workflow_id: workflow_id.clone(),
        tasks,
        status: "onboarding".into(),
    };

    idempotency::put(
        &mut *tx,
        org_id,
        "employee.onboard",
        &idem_key,
        201,
        serde_json::to_value(&resp).unwrap_or_default(),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(resp)).into_response())
}
