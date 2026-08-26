//! `/api/v1/operations/board` — five-column kanban (TASK_STATUSES order).

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::IdKind;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};

use super::tasks::{load_task_dto, TaskRow, TASK_COLUMNS};
use super::{internal, parse_public_id};
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{BoardColumnDto, ListQuery, TaskBoardResponse, TASK_STATUSES};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/operations/board", get(get_board))
}

/// GET /api/v1/operations/board
#[utoipa::path(get, path = "/api/v1/operations/board", tag = "operations-board",
    params(("project_id" = Option<String>, Query)),
    responses((status = 200, body = TaskBoardResponse)))]
pub async fn get_board(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<TaskBoardResponse>, AppError> {
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
    let scope = scope_for_permission(&membership.principal, &perms::operations_task_read());

    let project_id = match q.project_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::Project, s, &request_id)?),
        None => None,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {TASK_COLUMNS} FROM operations_task WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    push_owner_predicate(
        &mut qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    if let Some(pid) = project_id {
        qb.push(" AND project_id = ");
        qb.push_bind(pid);
    }
    qb.push(" ORDER BY position ASC, created_at ASC");

    let rows: Vec<TaskRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut by_status: std::collections::HashMap<String, Vec<crate::types::TaskDto>> =
        std::collections::HashMap::new();
    for status in TASK_STATUSES {
        by_status.insert((*status).to_string(), Vec::new());
    }
    for row in rows {
        let status = row.status.clone();
        let dto = load_task_dto(&mut tx, org_id, row)
            .await
            .map_err(internal(&request_id))?;
        by_status.entry(status).or_default().push(dto);
    }

    let columns: Vec<BoardColumnDto> = TASK_STATUSES
        .iter()
        .map(|status| BoardColumnDto {
            status: (*status).to_string(),
            tasks: by_status.remove(*status).unwrap_or_default(),
        })
        .collect();

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(TaskBoardResponse {
        project_id: q.project_id.clone(),
        columns,
    }))
}
