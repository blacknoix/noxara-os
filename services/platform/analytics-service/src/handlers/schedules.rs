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
use crate::export::{to_csv, to_xlsx_tsv};
use crate::query::{execute_query, validate_query_for_tenant, ReportDefinition};
use crate::schedule::{temporal_workflow_id, DeliveryState, ScheduleFireInput, WORKFLOW_TYPE};
use crate::state::AppState;
use crate::types::{
    CreateScheduleRequest, ExportResponse, FireScheduleRequest, FireScheduleResponse, ScheduleDto,
    UpdateScheduleRequest,
};

use super::{authorize, ensure_human, internal, not_found, parse_id, set_org};

#[derive(Debug, FromRow)]
struct ScheduleRow {
    id: Uuid,
    public_id: String,
    report_id: Uuid,
    cron: String,
    timezone: String,
    channel: String,
    recipients: serde_json::Value,
    export_format: String,
    enabled: bool,
    last_run_at: Option<DateTime<Utc>>,
    next_run_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn map_schedule(row: ScheduleRow, request_id: &str) -> Result<ScheduleDto, AppError> {
    let recipients = serde_json::from_value(row.recipients)
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    Ok(ScheduleDto {
        id: row.public_id,
        report_id: PublicId::new(IdKind::AnalyticsReport, row.report_id).as_str(),
        cron: row.cron,
        timezone: row.timezone,
        channel: row.channel,
        recipients,
        export_format: row.export_format,
        enabled: row.enabled,
        last_run_at: row.last_run_at,
        next_run_at: row.next_run_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn validate_format(format: &str, request_id: &str) -> Result<(), AppError> {
    if matches!(format, "csv" | "xlsx") {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "export format must be csv or xlsx",
        ))
    }
}

async fn fetch_schedule(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    schedule_id: Uuid,
    request_id: &str,
) -> Result<ScheduleRow, AppError> {
    sqlx::query_as(
        "SELECT id, public_id, report_id, cron, timezone, channel, recipients, export_format, \
         enabled, last_run_at, next_run_at, created_at, updated_at FROM analytics_schedule \
         WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(schedule_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "schedule"))
}

async fn emit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    auth: &AuthCtx,
    aggregate: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let event = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Analytics,
        aggregate,
        event_type,
        1,
        auth.ctx.actor.clone(),
        payload,
    );
    companyos_outbox::insert_event(&mut **tx, &event)
        .await
        .map_err(|error| {
            AppError::new(ErrorCode::Internal, &auth.ctx.request_id, error.to_string())
        })?;
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/analytics/schedules", tag = "analytics-schedules",
    responses((status = 200, body = [ScheduleDto])))]
pub async fn list_schedules(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<Vec<ScheduleDto>>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::analytics_report_read()).await?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let rows: Vec<ScheduleRow> = sqlx::query_as(
        "SELECT id, public_id, report_id, cron, timezone, channel, recipients, export_format, \
         enabled, last_run_at, next_run_at, created_at, updated_at FROM analytics_schedule \
         WHERE org_id = $1 ORDER BY updated_at DESC LIMIT 200",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    let schedules = rows
        .into_iter()
        .map(|row| map_schedule(row, request_id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(schedules))
}

#[utoipa::path(post, path = "/api/v1/analytics/schedules", tag = "analytics-schedules",
    request_body = CreateScheduleRequest, responses((status = 201, body = ScheduleDto)))]
pub async fn create_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleDto>), AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_schedule_write()).await?;
    if body.cron.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "cron is required",
        ));
    }
    validate_format(&body.export_format, request_id)?;
    let report_id = parse_id(IdKind::AnalyticsReport, &body.report_id, request_id)?;
    let recipients = serde_json::to_value(&body.recipients)
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AnalyticsSchedule, id).as_str();
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM analytics_report WHERE org_id = $1 AND id = $2")
            .bind(auth.ctx.org_id.as_uuid())
            .bind(report_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(request_id))?;
    if exists.is_none() {
        return Err(not_found(request_id, "report"));
    }
    let row: ScheduleRow = sqlx::query_as(
        "INSERT INTO analytics_schedule \
         (id, public_id, org_id, report_id, cron, timezone, channel, recipients, export_format, \
          enabled, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
         RETURNING id, public_id, report_id, cron, timezone, channel, recipients, export_format, \
         enabled, last_run_at, next_run_at, created_at, updated_at",
    )
    .bind(id)
    .bind(&public_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(report_id)
    .bind(body.cron.trim())
    .bind(body.timezone)
    .bind(body.channel)
    .bind(recipients)
    .bind(body.export_format)
    .bind(body.enabled)
    .bind(auth.ctx.actor.on_behalf_of)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok((StatusCode::CREATED, Json(map_schedule(row, request_id)?)))
}

#[utoipa::path(get, path = "/api/v1/analytics/schedules/{id}", tag = "analytics-schedules",
    params(("id" = String, Path)), responses((status = 200, body = ScheduleDto)))]
pub async fn get_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ScheduleDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::analytics_report_read()).await?;
    let schedule_id = parse_id(IdKind::AnalyticsSchedule, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row = fetch_schedule(&mut tx, auth.ctx.org_id.as_uuid(), schedule_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(map_schedule(row, request_id)?))
}

#[utoipa::path(patch, path = "/api/v1/analytics/schedules/{id}", tag = "analytics-schedules",
    params(("id" = String, Path)), request_body = UpdateScheduleRequest,
    responses((status = 200, body = ScheduleDto)))]
pub async fn update_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateScheduleRequest>,
) -> Result<Json<ScheduleDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_schedule_write()).await?;
    if body
        .cron
        .as_deref()
        .is_some_and(|cron| cron.trim().is_empty())
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "cron cannot be empty",
        ));
    }
    if let Some(format) = body.export_format.as_deref() {
        validate_format(format, request_id)?;
    }
    let schedule_id = parse_id(IdKind::AnalyticsSchedule, &id, request_id)?;
    let recipients = body
        .recipients
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row: ScheduleRow = sqlx::query_as(
        "UPDATE analytics_schedule SET cron = COALESCE($3, cron), \
         timezone = COALESCE($4, timezone), channel = COALESCE($5, channel), \
         recipients = COALESCE($6, recipients), export_format = COALESCE($7, export_format), \
         enabled = COALESCE($8, enabled), updated_at = now() \
         WHERE org_id = $1 AND id = $2 \
         RETURNING id, public_id, report_id, cron, timezone, channel, recipients, export_format, \
         enabled, last_run_at, next_run_at, created_at, updated_at",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(schedule_id)
    .bind(body.cron.as_deref().map(str::trim))
    .bind(body.timezone)
    .bind(body.channel)
    .bind(recipients)
    .bind(body.export_format)
    .bind(body.enabled)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "schedule"))?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(map_schedule(row, request_id)?))
}

#[utoipa::path(delete, path = "/api/v1/analytics/schedules/{id}", tag = "analytics-schedules",
    params(("id" = String, Path)), responses((status = 204)))]
pub async fn delete_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_schedule_write()).await?;
    let schedule_id = parse_id(IdKind::AnalyticsSchedule, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let result = sqlx::query("DELETE FROM analytics_schedule WHERE org_id = $1 AND id = $2")
        .bind(auth.ctx.org_id.as_uuid())
        .bind(schedule_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(request_id))?;
    if result.rows_affected() == 0 {
        return Err(not_found(request_id, "schedule"));
    }
    tx.commit().await.map_err(internal(request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/analytics/schedules/{id}/fire",
    tag = "analytics-schedules", params(("id" = String, Path)),
    request_body = FireScheduleRequest, responses((status = 200, body = FireScheduleResponse)))]
pub async fn fire_schedule(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<FireScheduleRequest>,
) -> Result<Json<FireScheduleResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    let principal = authorize(&state, &auth, perms::analytics_schedule_write()).await?;
    let schedule_id = parse_id(IdKind::AnalyticsSchedule, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let schedule =
        fetch_schedule(&mut tx, auth.ctx.org_id.as_uuid(), schedule_id, request_id).await?;
    if !schedule.enabled {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "schedule is disabled",
        ));
    }
    let format = body
        .export_format
        .unwrap_or_else(|| schedule.export_format.clone())
        .to_ascii_lowercase();
    validate_format(&format, request_id)?;
    let channel = body.channel.unwrap_or_else(|| schedule.channel.clone());
    let report: Option<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT public_id, definition FROM analytics_report WHERE org_id = $1 AND id = $2",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(schedule.report_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    let (report_public, definition_json) = report.ok_or_else(|| not_found(request_id, "report"))?;
    let mut definition: ReportDefinition = serde_json::from_value(definition_json)
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    let home = auth.ctx.region.unwrap_or(companyos_tenancy::RegionCode::Us);
    if definition.region.is_none() {
        definition.region = Some(home.as_str().into());
    }
    let validated = validate_query_for_tenant(&definition, home, request_id)?;
    if validated.org != auth.ctx.org_id {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "report org_id does not match authenticated tenant",
        ));
    }

    let run_id = new_uuid_v7();
    let run_public = PublicId::new(IdKind::AnalyticsRun, run_id).as_str();
    let workflow_id = temporal_workflow_id(auth.ctx.org_id, &schedule.public_id, &run_public);
    let _input = ScheduleFireInput {
        org_id: auth.ctx.org_id.to_public().as_str(),
        schedule_id: schedule.public_id.clone(),
        report_id: report_public.clone(),
        run_id: run_public.clone(),
        export_format: format.clone(),
        channel: channel.clone(),
    };
    let mut delivery = DeliveryState::Pending;
    delivery = delivery.advance("generate");
    let result = execute_query(&mut tx, &validated, principal.as_ref(), false, request_id).await?;
    let (content, content_type, file_id) = if format == "xlsx" {
        (
            to_xlsx_tsv(&result),
            "text/tab-separated-values".to_string(),
            "inline:xlsx-tsv".to_string(),
        )
    } else {
        (
            to_csv(&result),
            "text/csv".to_string(),
            "inline:csv".to_string(),
        )
    };
    delivery = delivery.advance("export_ready");
    sqlx::query(
        "INSERT INTO analytics_run \
         (id, public_id, org_id, report_id, schedule_id, kind, status, started_by, \
          finished_at, row_count, file_id) \
         VALUES ($1,$2,$3,$4,$5,'schedule','completed',$6,now(),$7,$8)",
    )
    .bind(run_id)
    .bind(&run_public)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(schedule.report_id)
    .bind(schedule.id)
    .bind(auth.ctx.actor.on_behalf_of)
    .bind(i32::try_from(result.rows.len()).unwrap_or(i32::MAX))
    .bind(&file_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    sqlx::query(
        "UPDATE analytics_schedule SET last_run_at = now(), updated_at = now() \
         WHERE org_id = $1 AND id = $2",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(schedule.id)
    .execute(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    emit(
        &mut tx,
        &auth,
        "schedule",
        "fired",
        json!({
            "id": schedule.public_id,
            "report_id": report_public,
            "run_id": run_public,
            "workflow_id": workflow_id,
            "channel": channel,
        }),
    )
    .await?;
    emit(
        &mut tx,
        &auth,
        "export",
        "ready",
        json!({
            "report_id": report_public,
            "schedule_id": schedule.public_id,
            "run_id": run_public,
            "format": format,
            "file_id": file_id,
        }),
    )
    .await?;
    delivery = delivery.advance("notify");
    delivery = delivery.advance("done");
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(FireScheduleResponse {
        schedule_id: id,
        run_id: run_public.clone(),
        workflow_id,
        workflow_type: WORKFLOW_TYPE.into(),
        state: format!("{delivery:?}").to_ascii_lowercase(),
        export: ExportResponse {
            run_id: run_public,
            report_id: report_public,
            format,
            content_type,
            file_id,
            content,
            row_count: result.rows.len(),
        },
    }))
}
