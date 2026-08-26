//! `/api/v1/operations/summary` — dashboard counts for Operations.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};

use super::internal;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::SummaryResponse;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/operations/summary", get(get_summary))
}

/// GET /api/v1/operations/summary
#[utoipa::path(get, path = "/api/v1/operations/summary", tag = "operations-summary",
    responses((status = 200, body = SummaryResponse)))]
pub async fn get_summary(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<SummaryResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_read(),
        &request_id,
    )?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_project_read(),
        &request_id,
    )?;
    let task_scope = scope_for_permission(&membership.principal, &perms::operations_task_read());
    let project_scope =
        scope_for_permission(&membership.principal, &perms::operations_project_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut open_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM operations_task WHERE org_id = ");
    open_qb.push_bind(org_id);
    open_qb.push(" AND deleted_at IS NULL AND status <> 'done'");
    push_owner_predicate(
        &mut open_qb,
        task_scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    let open_tasks: i64 = open_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    // Assignee-leading path (my_work index): org_id + assignee_id.
    let my_open_tasks: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM operations_task
        WHERE org_id = $1 AND assignee_id = $2
          AND deleted_at IS NULL AND status <> 'done'
        "#,
    )
    .bind(org_id)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut proj_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM operations_project WHERE org_id = ");
    proj_qb.push_bind(org_id);
    proj_qb.push(" AND deleted_at IS NULL AND status = 'active'");
    push_owner_predicate(
        &mut proj_qb,
        project_scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    let projects_active: i64 = proj_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut overdue_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM operations_task WHERE org_id = ");
    overdue_qb.push_bind(org_id);
    overdue_qb.push(
        " AND deleted_at IS NULL AND status <> 'done' AND due_at IS NOT NULL AND due_at < now()",
    );
    push_owner_predicate(
        &mut overdue_qb,
        task_scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    let overdue: i64 = overdue_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(SummaryResponse {
        open_tasks,
        my_open_tasks,
        projects_active,
        overdue,
    }))
}
