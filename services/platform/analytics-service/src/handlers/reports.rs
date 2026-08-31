use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::query::{execute_query, validate_query, ReportDefinition};
use crate::state::AppState;
use crate::types::{
    CreateReportRequest, QueryResult, ReportDto, ReportListResponse, RunReportRequest,
    RunReportResponse, SimulateQueryRequest, UpdateReportRequest,
};

use super::{authorize, ensure_human, internal, not_found, parse_id, set_org, user_public};

#[derive(Debug, FromRow)]
struct ReportRow {
    id: Uuid,
    public_id: String,
    org_id: Uuid,
    name: String,
    description: String,
    definition: serde_json::Value,
    visualization: String,
    created_by: Uuid,
    updated_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn map_report(row: ReportRow, request_id: &str) -> Result<ReportDto, AppError> {
    let definition = serde_json::from_value(row.definition)
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    Ok(ReportDto {
        id: row.public_id,
        org_id: companyos_tenancy::OrgId::new(row.org_id)
            .to_public()
            .as_str(),
        name: row.name,
        description: row.description,
        definition,
        visualization: row.visualization,
        created_by: user_public(row.created_by),
        updated_by: user_public(row.updated_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn validate_definition(
    mut definition: ReportDefinition,
    auth: &AuthCtx,
) -> Result<ReportDefinition, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let expected = auth.ctx.org_id.to_public().as_str();
    if let Some(org_id) = definition.org_id.as_deref() {
        if org_id != expected {
            return Err(AppError::new(
                ErrorCode::Forbidden,
                request_id,
                "report org_id does not match authenticated tenant",
            ));
        }
    } else {
        definition.org_id = Some(expected);
    }
    validate_query(&definition, request_id)?;
    Ok(definition)
}

async fn fetch_report(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    report_id: Uuid,
    request_id: &str,
) -> Result<ReportRow, AppError> {
    sqlx::query_as(
        "SELECT id, public_id, org_id, name, description, definition, visualization, \
         created_by, updated_by, created_at, updated_at \
         FROM analytics_report WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(report_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "report"))
}

async fn emit_report_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    auth: &AuthCtx,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Analytics,
        "report",
        event_type,
        1,
        auth.ctx.actor.clone(),
        payload,
    );
    companyos_outbox::insert_event(&mut **tx, &envelope)
        .await
        .map_err(|error| {
            AppError::new(ErrorCode::Internal, &auth.ctx.request_id, error.to_string())
        })?;
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/analytics/reports", tag = "analytics-reports",
    responses((status = 200, body = ReportListResponse)))]
pub async fn list_reports(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ReportListResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::analytics_report_read()).await?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let rows: Vec<ReportRow> = sqlx::query_as(
        "SELECT id, public_id, org_id, name, description, definition, visualization, \
         created_by, updated_by, created_at, updated_at FROM analytics_report \
         WHERE org_id = $1 ORDER BY updated_at DESC LIMIT 200",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    let reports = rows
        .into_iter()
        .map(|row| map_report(row, request_id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ReportListResponse { reports }))
}

#[utoipa::path(post, path = "/api/v1/analytics/reports", tag = "analytics-reports",
    request_body = CreateReportRequest, responses((status = 201, body = ReportDto)))]
pub async fn create_report(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateReportRequest>,
) -> Result<(StatusCode, Json<ReportDto>), AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_report_write()).await?;
    if body.name.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "report name is required",
        ));
    }
    let definition = validate_definition(body.definition, &auth)?;
    let definition_json = serde_json::to_value(&definition)
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AnalyticsReport, id).as_str();
    let actor = auth.ctx.actor.on_behalf_of;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row: ReportRow = sqlx::query_as(
        "INSERT INTO analytics_report \
         (id, public_id, org_id, name, description, definition, visualization, created_by, updated_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$8) \
         RETURNING id, public_id, org_id, name, description, definition, visualization, \
         created_by, updated_by, created_at, updated_at",
    )
    .bind(id)
    .bind(&public_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(definition_json)
    .bind(&definition.visualization)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    emit_report_event(
        &mut tx,
        &auth,
        "saved",
        json!({"id": public_id, "metric": definition.metric}),
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok((StatusCode::CREATED, Json(map_report(row, request_id)?)))
}

#[utoipa::path(get, path = "/api/v1/analytics/reports/{id}", tag = "analytics-reports",
    params(("id" = String, Path)), responses((status = 200, body = ReportDto)))]
pub async fn get_report(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ReportDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::analytics_report_read()).await?;
    let report_id = parse_id(IdKind::AnalyticsReport, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row = fetch_report(&mut tx, auth.ctx.org_id.as_uuid(), report_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(map_report(row, request_id)?))
}

#[utoipa::path(patch, path = "/api/v1/analytics/reports/{id}", tag = "analytics-reports",
    params(("id" = String, Path)), request_body = UpdateReportRequest,
    responses((status = 200, body = ReportDto)))]
pub async fn update_report(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateReportRequest>,
) -> Result<Json<ReportDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_report_write()).await?;
    let report_id = parse_id(IdKind::AnalyticsReport, &id, request_id)?;
    if body
        .name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "report name cannot be empty",
        ));
    }
    let definition = body
        .definition
        .map(|definition| validate_definition(definition, &auth))
        .transpose()?;
    let definition_json = definition
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row: ReportRow = sqlx::query_as(
        "UPDATE analytics_report SET \
         name = COALESCE($3, name), description = COALESCE($4, description), \
         definition = COALESCE($5, definition), \
         visualization = COALESCE($6, visualization), updated_by = $7, updated_at = now() \
         WHERE org_id = $1 AND id = $2 \
         RETURNING id, public_id, org_id, name, description, definition, visualization, \
         created_by, updated_by, created_at, updated_at",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(report_id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.description)
    .bind(definition_json)
    .bind(
        definition
            .as_ref()
            .map(|value| value.visualization.as_str()),
    )
    .bind(auth.ctx.actor.on_behalf_of)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "report"))?;
    emit_report_event(&mut tx, &auth, "saved", json!({"id": id})).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(map_report(row, request_id)?))
}

#[utoipa::path(delete, path = "/api/v1/analytics/reports/{id}", tag = "analytics-reports",
    params(("id" = String, Path)), responses((status = 204)))]
pub async fn delete_report(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_report_write()).await?;
    let report_id = parse_id(IdKind::AnalyticsReport, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let result = sqlx::query("DELETE FROM analytics_report WHERE org_id = $1 AND id = $2")
        .bind(auth.ctx.org_id.as_uuid())
        .bind(report_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(request_id))?;
    if result.rows_affected() == 0 {
        return Err(not_found(request_id, "report"));
    }
    tx.commit().await.map_err(internal(request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn run_definition(
    state: &AppState,
    auth: &AuthCtx,
    definition: ReportDefinition,
    report: Option<(Uuid, String)>,
    principal: Option<&companyos_authz::Principal>,
) -> Result<RunReportResponse, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let definition = validate_definition(definition, auth)?;
    let validated = validate_query(&definition, request_id)?;
    let run_id = new_uuid_v7();
    let run_public = PublicId::new(IdKind::AnalyticsRun, run_id).as_str();
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let result = execute_query(&mut tx, &validated, principal, false, request_id).await?;
    sqlx::query(
        "INSERT INTO analytics_run \
         (id, public_id, org_id, report_id, kind, status, started_by, finished_at, row_count) \
         VALUES ($1,$2,$3,$4,'report','completed',$5,now(),$6)",
    )
    .bind(run_id)
    .bind(&run_public)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(report.as_ref().map(|(id, _)| *id))
    .bind(auth.ctx.actor.on_behalf_of)
    .bind(i32::try_from(result.rows.len()).unwrap_or(i32::MAX))
    .execute(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    emit_report_event(
        &mut tx,
        auth,
        "run",
        json!({
            "id": report.as_ref().map(|(_, public)| public),
            "run_id": run_public,
            "metric": result.metric,
            "row_count": result.rows.len(),
        }),
    )
    .await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(RunReportResponse {
        run_id: run_public,
        report_id: report.map(|(_, public)| public),
        result,
    })
}

#[utoipa::path(post, path = "/api/v1/analytics/reports/{id}/run", tag = "analytics-reports",
    params(("id" = String, Path)), request_body = RunReportRequest,
    responses((status = 200, body = RunReportResponse)))]
pub async fn run_report(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(_body): Json<RunReportRequest>,
) -> Result<Json<RunReportResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal = authorize(&state, &auth, perms::analytics_report_run()).await?;
    let report_id = parse_id(IdKind::AnalyticsReport, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row = fetch_report(&mut tx, auth.ctx.org_id.as_uuid(), report_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    let definition: ReportDefinition = serde_json::from_value(row.definition)
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    Ok(Json(
        run_definition(
            &state,
            &auth,
            definition,
            Some((report_id, row.public_id)),
            principal.as_ref(),
        )
        .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/analytics/reports/simulate", tag = "analytics-reports",
    request_body = SimulateQueryRequest, responses((status = 200, body = QueryResult)))]
pub async fn simulate_query(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<SimulateQueryRequest>,
) -> Result<Json<crate::query::QueryResult>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal = authorize(&state, &auth, perms::analytics_report_run()).await?;
    let definition = validate_definition(body.definition, &auth)?;
    let validated = validate_query(&definition, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let result = execute_query(&mut tx, &validated, principal.as_ref(), true, request_id).await?;
    tx.rollback().await.map_err(internal(request_id))?;
    Ok(Json(result))
}

#[utoipa::path(post, path = "/api/v1/analytics/query/run", tag = "analytics-reports",
    request_body = ReportDefinition, responses((status = 200, body = RunReportResponse)))]
pub async fn run_query(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(definition): Json<ReportDefinition>,
) -> Result<Json<RunReportResponse>, AppError> {
    let principal = authorize(&state, &auth, perms::analytics_report_run()).await?;
    Ok(Json(
        run_definition(&state, &auth, definition, None, principal.as_ref()).await?,
    ))
}
