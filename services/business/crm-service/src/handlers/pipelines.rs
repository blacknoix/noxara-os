//! `/api/v1/sales/pipelines` — pipelines, stages, and the kanban board view.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::Postgres;
use uuid::Uuid;

use super::internal;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::seed;
use crate::state::AppState;
use crate::types::{BoardResponse, BoardStage, PipelineDto, PipelineListResponse, StageDto};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sales/pipelines", get(list_pipelines))
        .route("/api/v1/sales/pipelines/default/board", get(default_board))
}

#[derive(Debug, sqlx::FromRow)]
struct PipelineRow {
    public_id: String,
    name: String,
    is_default: bool,
}

impl PipelineRow {
    fn into_dto(self) -> PipelineDto {
        PipelineDto {
            id: self.public_id,
            name: self.name,
            is_default: self.is_default,
        }
    }
}

/// GET /api/v1/sales/pipelines
#[utoipa::path(get, path = "/api/v1/sales/pipelines", tag = "sales-pipelines",
    responses((status = 200, body = PipelineListResponse)))]
pub async fn list_pipelines(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PipelineListResponse>, AppError> {
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
        perms::sales_pipeline_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // First-use seeding: ensure at least the default pipeline exists.
    seed::ensure_default_pipeline(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let rows: Vec<PipelineRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, is_default FROM sales_pipeline
        WHERE org_id = $1 AND deleted_at IS NULL
        ORDER BY is_default DESC, created_at ASC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(PipelineListResponse {
        items: rows.into_iter().map(PipelineRow::into_dto).collect(),
    }))
}

/// Fetch open deals for a stage, filtered by the caller's authz scope.
/// Shared by the pipeline board and the `/deals/board` alias.
async fn open_deals_for_stage(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    stage_id: Uuid,
    scope: companyos_authz::Scope,
    actor: Uuid,
    team_id: Option<Uuid>,
    department_id: Option<Uuid>,
) -> Result<Vec<crate::types::DealDto>, sqlx::Error> {
    let mut qb: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(&format!(
        "SELECT {} FROM sales_deal WHERE org_id = ",
        super::deals::DEAL_COLUMNS
    ));
    qb.push_bind(org_id);
    qb.push(" AND stage_id = ");
    qb.push_bind(stage_id);
    qb.push(" AND status = 'open' AND deleted_at IS NULL");
    push_owner_predicate(&mut qb, scope, org_id, actor, team_id, department_id);
    qb.push(" ORDER BY created_at DESC");

    let rows: Vec<super::deals::DealRow> = qb.build_query_as().fetch_all(&mut **tx).await?;
    Ok(rows
        .into_iter()
        .map(super::deals::DealRow::into_dto)
        .collect())
}

/// GET /api/v1/sales/pipelines/default/board — kanban view: stages + open deals.
///
/// Also mounted at `GET /api/v1/sales/deals/board` (same handler).
#[utoipa::path(get, path = "/api/v1/sales/pipelines/default/board", tag = "sales-pipelines",
    responses((status = 200, body = BoardResponse)))]
pub async fn default_board(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<BoardResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::sales_pipeline_read(),
        &request_id,
    )?;
    enforce_any_scope(&membership.principal, perms::sales_deal_read(), &request_id)?;
    let deal_scope = scope_for_permission(&membership.principal, &perms::sales_deal_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let pipeline_id = seed::ensure_default_pipeline(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let pipeline_row: PipelineRow = sqlx::query_as(
        "SELECT public_id, name, is_default FROM sales_pipeline WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(pipeline_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    #[derive(sqlx::FromRow)]
    struct StageRow {
        public_id: String,
        name: String,
        position: i32,
        probability: i32,
        is_won: bool,
        is_lost: bool,
        id: Uuid,
    }
    let stage_rows: Vec<StageRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, position, probability, is_won, is_lost, id
        FROM sales_pipeline_stage
        WHERE org_id = $1 AND pipeline_id = $2 AND deleted_at IS NULL
        ORDER BY position ASC
        "#,
    )
    .bind(org_id)
    .bind(pipeline_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut stages = Vec::with_capacity(stage_rows.len());
    for s in stage_rows {
        let deals = open_deals_for_stage(
            &mut tx,
            org_id,
            s.id,
            deal_scope,
            actor,
            membership.team_id,
            membership.department_id,
        )
        .await
        .map_err(internal(&request_id))?;
        stages.push(BoardStage {
            stage: StageDto {
                id: s.public_id,
                pipeline_id: PublicId::new(IdKind::Pipeline, pipeline_id).as_str(),
                name: s.name,
                position: s.position,
                probability: s.probability,
                is_won: s.is_won,
                is_lost: s.is_lost,
            },
            deals,
        });
    }

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(BoardResponse {
        pipeline: pipeline_row.into_dto(),
        stages,
    }))
}
