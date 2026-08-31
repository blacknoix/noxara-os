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
use crate::metrics::get_metric;
use crate::state::AppState;
use crate::types::{
    CreateDashboardRequest, DashboardDto, DashboardListResponse, UpdateDashboardRequest,
    UpsertWidgetRequest, WidgetDto,
};

use super::{authorize, ensure_human, internal, not_found, parse_id, set_org, user_public};

#[derive(Debug, FromRow)]
struct DashboardRow {
    id: Uuid,
    public_id: String,
    org_id: Uuid,
    name: String,
    description: String,
    layout: serde_json::Value,
    created_by: Uuid,
    updated_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct WidgetRow {
    id: Uuid,
    public_id: String,
    dashboard_id: Uuid,
    title: String,
    metric_name: String,
    visualization: String,
    config: serde_json::Value,
    position: i32,
    created_at: DateTime<Utc>,
}

fn map_widget(row: WidgetRow) -> WidgetDto {
    WidgetDto {
        id: row.public_id,
        dashboard_id: PublicId::new(IdKind::AnalyticsDashboard, row.dashboard_id).as_str(),
        title: row.title,
        metric_name: row.metric_name,
        visualization: row.visualization,
        config: row.config,
        position: row.position,
        created_at: row.created_at,
    }
}

fn map_dashboard(row: DashboardRow, widgets: Vec<WidgetDto>) -> DashboardDto {
    DashboardDto {
        id: row.public_id,
        org_id: companyos_tenancy::OrgId::new(row.org_id)
            .to_public()
            .as_str(),
        name: row.name,
        description: row.description,
        layout: row.layout,
        widgets,
        created_by: user_public(row.created_by),
        updated_by: user_public(row.updated_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

async fn widgets_for(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    dashboard_id: Uuid,
    request_id: &str,
) -> Result<Vec<WidgetDto>, AppError> {
    let rows: Vec<WidgetRow> = sqlx::query_as(
        "SELECT id, public_id, dashboard_id, title, metric_name, visualization, \
         config, position, created_at FROM analytics_dashboard_widget \
         WHERE org_id = $1 AND dashboard_id = $2 ORDER BY position, created_at",
    )
    .bind(org_id)
    .bind(dashboard_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(rows.into_iter().map(map_widget).collect())
}

async fn fetch_dashboard(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    dashboard_id: Uuid,
    request_id: &str,
) -> Result<DashboardRow, AppError> {
    sqlx::query_as(
        "SELECT id, public_id, org_id, name, description, layout, created_by, updated_by, \
         created_at, updated_at FROM analytics_dashboard WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(dashboard_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "dashboard"))
}

async fn emit_saved(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    auth: &AuthCtx,
    dashboard_id: &str,
) -> Result<(), AppError> {
    let event = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Analytics,
        "dashboard",
        "saved",
        1,
        auth.ctx.actor.clone(),
        json!({"id": dashboard_id}),
    );
    companyos_outbox::insert_event(&mut **tx, &event)
        .await
        .map_err(|error| {
            AppError::new(ErrorCode::Internal, &auth.ctx.request_id, error.to_string())
        })?;
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/analytics/dashboards", tag = "analytics-dashboards",
    responses((status = 200, body = DashboardListResponse)))]
pub async fn list_dashboards(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<DashboardListResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::analytics_dashboard_read()).await?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let rows: Vec<DashboardRow> = sqlx::query_as(
        "SELECT id, public_id, org_id, name, description, layout, created_by, updated_by, \
         created_at, updated_at FROM analytics_dashboard \
         WHERE org_id = $1 ORDER BY updated_at DESC LIMIT 200",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    let mut dashboards = Vec::with_capacity(rows.len());
    for row in rows {
        let widgets = widgets_for(&mut tx, auth.ctx.org_id.as_uuid(), row.id, request_id).await?;
        dashboards.push(map_dashboard(row, widgets));
    }
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(DashboardListResponse { dashboards }))
}

#[utoipa::path(post, path = "/api/v1/analytics/dashboards", tag = "analytics-dashboards",
    request_body = CreateDashboardRequest, responses((status = 201, body = DashboardDto)))]
pub async fn create_dashboard(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateDashboardRequest>,
) -> Result<(StatusCode, Json<DashboardDto>), AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_dashboard_write()).await?;
    if body.name.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "dashboard name is required",
        ));
    }
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AnalyticsDashboard, id).as_str();
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row: DashboardRow = sqlx::query_as(
        "INSERT INTO analytics_dashboard \
         (id, public_id, org_id, name, description, layout, created_by, updated_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$7) \
         RETURNING id, public_id, org_id, name, description, layout, created_by, updated_by, \
         created_at, updated_at",
    )
    .bind(id)
    .bind(&public_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(body.name.trim())
    .bind(body.description)
    .bind(body.layout)
    .bind(auth.ctx.actor.on_behalf_of)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    emit_saved(&mut tx, &auth, &public_id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok((StatusCode::CREATED, Json(map_dashboard(row, Vec::new()))))
}

#[utoipa::path(get, path = "/api/v1/analytics/dashboards/{id}", tag = "analytics-dashboards",
    params(("id" = String, Path)), responses((status = 200, body = DashboardDto)))]
pub async fn get_dashboard(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<DashboardDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    authorize(&state, &auth, perms::analytics_dashboard_read()).await?;
    let dashboard_id = parse_id(IdKind::AnalyticsDashboard, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row = fetch_dashboard(&mut tx, auth.ctx.org_id.as_uuid(), dashboard_id, request_id).await?;
    let widgets = widgets_for(&mut tx, auth.ctx.org_id.as_uuid(), dashboard_id, request_id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(map_dashboard(row, widgets)))
}

#[utoipa::path(patch, path = "/api/v1/analytics/dashboards/{id}", tag = "analytics-dashboards",
    params(("id" = String, Path)), request_body = UpdateDashboardRequest,
    responses((status = 200, body = DashboardDto)))]
pub async fn update_dashboard(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateDashboardRequest>,
) -> Result<Json<DashboardDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_dashboard_write()).await?;
    if body
        .name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "dashboard name cannot be empty",
        ));
    }
    let dashboard_id = parse_id(IdKind::AnalyticsDashboard, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row: DashboardRow = sqlx::query_as(
        "UPDATE analytics_dashboard SET name = COALESCE($3, name), \
         description = COALESCE($4, description), layout = COALESCE($5, layout), \
         updated_by = $6, updated_at = now() WHERE org_id = $1 AND id = $2 \
         RETURNING id, public_id, org_id, name, description, layout, created_by, updated_by, \
         created_at, updated_at",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(dashboard_id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.description)
    .bind(body.layout)
    .bind(auth.ctx.actor.on_behalf_of)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "dashboard"))?;
    let widgets = widgets_for(&mut tx, auth.ctx.org_id.as_uuid(), dashboard_id, request_id).await?;
    emit_saved(&mut tx, &auth, &id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(map_dashboard(row, widgets)))
}

#[utoipa::path(delete, path = "/api/v1/analytics/dashboards/{id}", tag = "analytics-dashboards",
    params(("id" = String, Path)), responses((status = 204)))]
pub async fn delete_dashboard(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_dashboard_write()).await?;
    let dashboard_id = parse_id(IdKind::AnalyticsDashboard, &id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let result = sqlx::query("DELETE FROM analytics_dashboard WHERE org_id = $1 AND id = $2")
        .bind(auth.ctx.org_id.as_uuid())
        .bind(dashboard_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(request_id))?;
    if result.rows_affected() == 0 {
        return Err(not_found(request_id, "dashboard"));
    }
    tx.commit().await.map_err(internal(request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/analytics/dashboards/{id}/widgets",
    tag = "analytics-dashboards", params(("id" = String, Path)),
    request_body = UpsertWidgetRequest, responses((status = 200, body = WidgetDto)))]
pub async fn upsert_widget(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpsertWidgetRequest>,
) -> Result<Json<WidgetDto>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_dashboard_write()).await?;
    if get_metric(&body.metric_name).is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("unknown governed metric '{}'", body.metric_name),
        ));
    }
    if body.title.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "widget title is required",
        ));
    }
    let dashboard_id = parse_id(IdKind::AnalyticsDashboard, &id, request_id)?;
    let (widget_id, public_id) = if let Some(widget_public) = body.id.as_deref() {
        (
            parse_id(IdKind::AnalyticsWidget, widget_public, request_id)?,
            widget_public.to_string(),
        )
    } else {
        let widget_id = new_uuid_v7();
        (
            widget_id,
            PublicId::new(IdKind::AnalyticsWidget, widget_id).as_str(),
        )
    };
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    fetch_dashboard(&mut tx, auth.ctx.org_id.as_uuid(), dashboard_id, request_id).await?;
    let row: WidgetRow = sqlx::query_as(
        "INSERT INTO analytics_dashboard_widget \
         (id, public_id, org_id, dashboard_id, title, metric_name, visualization, config, position) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) \
         ON CONFLICT (org_id, public_id) DO UPDATE SET \
         title = EXCLUDED.title, metric_name = EXCLUDED.metric_name, \
         visualization = EXCLUDED.visualization, config = EXCLUDED.config, \
         position = EXCLUDED.position \
         RETURNING id, public_id, dashboard_id, title, metric_name, visualization, \
         config, position, created_at",
    )
    .bind(widget_id)
    .bind(public_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(dashboard_id)
    .bind(body.title.trim())
    .bind(body.metric_name)
    .bind(body.visualization)
    .bind(body.config)
    .bind(body.position)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    emit_saved(&mut tx, &auth, &id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(map_widget(row)))
}

#[utoipa::path(delete, path = "/api/v1/analytics/dashboards/{id}/widgets/{widget_id}",
    tag = "analytics-dashboards", params(("id" = String, Path), ("widget_id" = String, Path)),
    responses((status = 204)))]
pub async fn delete_widget(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path((id, widget_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    ensure_human(&auth)?;
    authorize(&state, &auth, perms::analytics_dashboard_write()).await?;
    let dashboard_id = parse_id(IdKind::AnalyticsDashboard, &id, request_id)?;
    let widget_id = parse_id(IdKind::AnalyticsWidget, &widget_id, request_id)?;
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let result = sqlx::query(
        "DELETE FROM analytics_dashboard_widget \
         WHERE org_id = $1 AND dashboard_id = $2 AND id = $3",
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(dashboard_id)
    .bind(widget_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    if result.rows_affected() == 0 {
        return Err(not_found(request_id, "widget"));
    }
    emit_saved(&mut tx, &auth, &id).await?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(StatusCode::NO_CONTENT)
}
