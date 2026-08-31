//! Governance HTTP handlers — `/api/v1/governance/...`.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::new_uuid_v7;
use companyos_outbox::insert_event;
use companyos_tenancy::set_session_org_id;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::types::*;
use super::{access_review, api_keys, audit_verify, retention};
use super::{authorize, internal, outbox_internal, validation};
use crate::auth::extract::AuthUser;
use crate::state::AppState;
use crate::workspace::types::MessageResponse;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/governance/access-review/who-could-see",
            get(who_could_see),
        )
        .route("/api/v1/governance/access-review/who-did", get(who_did))
        .route("/api/v1/governance/access-review/runs", post(kickoff_run))
        .route("/api/v1/governance/access-review/runs/{id}", get(get_run))
        .route(
            "/api/v1/governance/access-review/runs/{id}/export",
            get(export_run),
        )
        .route("/api/v1/governance/audit/verify", post(verify_audit))
        .route(
            "/api/v1/governance/retention",
            get(get_retention).put(update_retention),
        )
        .route(
            "/api/v1/governance/retention/dry-run",
            post(retention_dry_run),
        )
        .route(
            "/api/v1/governance/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route(
            "/api/v1/governance/api-keys/{id}/rotate",
            post(rotate_api_key),
        )
        .route(
            "/api/v1/governance/api-keys/{id}/revoke",
            post(revoke_api_key),
        )
}

fn idem_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn idem_get<'e, E>(
    executor: E,
    org_id: Uuid,
    scope: &str,
    key: &str,
) -> Result<Option<(i32, Value)>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as(
        r#"
        SELECT response_status, response_body
        FROM governance_idempotency
        WHERE org_id = $1 AND scope = $2 AND key = $3
        "#,
    )
    .bind(org_id)
    .bind(scope)
    .bind(key)
    .fetch_optional(executor)
    .await
}

async fn idem_put<'e, E>(
    executor: E,
    org_id: Uuid,
    scope: &str,
    key: &str,
    status: i32,
    body: Value,
) -> Result<(), sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query(
        r#"
        INSERT INTO governance_idempotency (id, org_id, scope, key, response_status, response_body)
        VALUES ($1,$2,$3,$4,$5,$6)
        ON CONFLICT (org_id, scope, key) DO NOTHING
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org_id)
    .bind(scope)
    .bind(key)
    .bind(status)
    .bind(body)
    .execute(executor)
    .await?;
    Ok(())
}

fn parse_period(
    period_start: &str,
    period_end: &str,
    request_id: &str,
) -> Result<(DateTime<Utc>, DateTime<Utc>), AppError> {
    let start = DateTime::parse_from_rfc3339(period_start)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| validation(request_id, "period_start must be RFC3339"))?;
    let end = DateTime::parse_from_rfc3339(period_end)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| validation(request_id, "period_end must be RFC3339"))?;
    if end < start {
        return Err(validation(request_id, "period_end must be >= period_start"));
    }
    Ok((start, end))
}

fn parse_optional_rfc3339(
    s: Option<&str>,
    request_id: &str,
) -> Result<Option<DateTime<Utc>>, AppError> {
    match s {
        None => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|d| Some(d.with_timezone(&Utc)))
            .map_err(|_| validation(request_id, "expires_at must be RFC3339")),
    }
}

#[utoipa::path(get, path = "/api/v1/governance/access-review/who-could-see", tag = "governance",
    responses((status = 200, body = WhoCouldSeeResponse)))]
pub async fn who_could_see(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<AccessReviewQuery>,
) -> Result<Json<WhoCouldSeeResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_access_review_read()).await?;
    let (start, end) = parse_period(&q.period_start, &q.period_end, &request_id)?;
    let items = access_review::who_could(
        &state.pool,
        user.ctx.org_id,
        &q.permission_id,
        start,
        end,
        &request_id,
    )
    .await?;
    Ok(Json(WhoCouldSeeResponse { items }))
}

#[utoipa::path(get, path = "/api/v1/governance/access-review/who-did", tag = "governance",
    responses((status = 200, body = WhoDidSeeResponse)))]
pub async fn who_did(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<AccessReviewQuery>,
) -> Result<Json<WhoDidSeeResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_access_review_read()).await?;
    let (start, end) = parse_period(&q.period_start, &q.period_end, &request_id)?;
    let items = access_review::who_did(
        &state.pool,
        user.ctx.org_id,
        &q.permission_id,
        start,
        end,
        &request_id,
    )
    .await?;
    Ok(Json(WhoDidSeeResponse { items }))
}

#[utoipa::path(post, path = "/api/v1/governance/access-review/runs", tag = "governance",
    request_body = AccessReviewKickoffRequest, responses((status = 201, body = AccessReviewRunView)))]
pub async fn kickoff_run(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Json(body): Json<AccessReviewKickoffRequest>,
) -> Result<Response, AppError> {
    let request_id = user.ctx.request_id.clone();
    let org_id = user.ctx.org_id.as_uuid();
    authorize(&state, &user, &perms::admin_access_review_manage()).await?;
    let (start, end) = parse_period(&body.period_start, &body.period_end, &request_id)?;
    let idem_key = idem_header(&headers);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idem_get(&mut *tx, org_id, "access_review.kickoff", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let view = access_review::kickoff_run(
        &mut tx,
        user.ctx.org_id,
        user.ctx.actor.clone(),
        &body.permission_id,
        start,
        end,
        &request_id,
    )
    .await?;

    if let Some(key) = idem_key.as_deref() {
        idem_put(
            &mut *tx,
            org_id,
            "access_review.kickoff",
            key,
            201,
            serde_json::to_value(&view).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

#[utoipa::path(get, path = "/api/v1/governance/access-review/runs/{id}", tag = "governance",
    params(("id" = String, Path)), responses((status = 200, body = AccessReviewRunView)))]
pub async fn get_run(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<AccessReviewRunView>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_access_review_read()).await?;
    let view = access_review::get_run(&state.pool, user.ctx.org_id, &id, &request_id).await?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    format: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/governance/access-review/runs/{id}/export", tag = "governance",
    params(("id" = String, Path)),
    responses((status = 200, description = "JSON (default) or CSV export depending on ?format=")))]
pub async fn export_run(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_access_review_read()).await?;
    match q.format.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("json") => {
            let value =
                access_review::export_json(&state.pool, user.ctx.org_id, &id, &request_id).await?;
            Ok(Json(value).into_response())
        }
        Some("csv") => {
            let csv =
                access_review::export_csv(&state.pool, user.ctx.org_id, &id, &request_id).await?;
            // Explicit Response::builder so the body cannot be dropped by
            // header-tuple IntoResponse ambiguity under axum upgrades.
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
                .body(Body::from(csv))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
        }
        Some(other) => Err(validation(
            &request_id,
            format!("unsupported format: {other}"),
        )),
    }
}

#[utoipa::path(post, path = "/api/v1/governance/audit/verify", tag = "governance",
    request_body = AuditVerifyRequest, responses((status = 200, body = AuditVerifyResponse)))]
pub async fn verify_audit(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<AuditVerifyRequest>,
) -> Result<Json<AuditVerifyResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_audit_verify()).await?;
    let result = match body.partition_key.as_deref() {
        Some(key) => {
            audit_verify::verify_partition(&state.pool, user.ctx.org_id, key, &request_id).await?
        }
        None => audit_verify::verify_all(&state.pool, user.ctx.org_id, &request_id).await?,
    };
    Ok(Json(result))
}

#[utoipa::path(get, path = "/api/v1/governance/retention", tag = "governance",
    responses((status = 200, body = RetentionConfigView)))]
pub async fn get_retention(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<RetentionConfigView>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_retention_manage()).await?;
    let view = retention::get(&state.pool, user.ctx.org_id, &request_id).await?;
    Ok(Json(view))
}

#[utoipa::path(put, path = "/api/v1/governance/retention", tag = "governance",
    request_body = UpdateRetentionRequest, responses((status = 200, body = RetentionConfigView)))]
pub async fn update_retention(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Json(body): Json<UpdateRetentionRequest>,
) -> Result<Response, AppError> {
    let request_id = user.ctx.request_id.clone();
    let org_id = user.ctx.org_id.as_uuid();
    authorize(&state, &user, &perms::admin_retention_manage()).await?;
    if let Some(days) = body.default_retention_days {
        if !(30..=3650).contains(&days) {
            return Err(validation(
                &request_id,
                "default_retention_days must be between 30 and 3650",
            ));
        }
    }
    let idem_key = idem_header(&headers);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idem_get(&mut *tx, org_id, "retention.update", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let view = retention::upsert(
        &mut tx,
        user.ctx.org_id,
        user.ctx.actor.user_id,
        body.default_retention_days,
        body.overrides.clone(),
        &request_id,
    )
    .await?;

    let envelope = EventEnvelope::new(
        user.ctx.org_id,
        Context::Admin,
        "retention",
        "changed",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({
            "default_retention_days": view.default_retention_days,
            "version": view.version,
        }),
    );
    insert_event(&mut *tx, &envelope)
        .await
        .map_err(outbox_internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        idem_put(
            &mut *tx,
            org_id,
            "retention.update",
            key,
            200,
            serde_json::to_value(&view).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(view).into_response())
}

#[utoipa::path(post, path = "/api/v1/governance/retention/dry-run", tag = "governance",
    responses((status = 200, body = RetentionDryRunResponse)))]
pub async fn retention_dry_run(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<RetentionDryRunResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_retention_manage()).await?;
    let result = retention::dry_run(&state.pool, user.ctx.org_id, &request_id).await?;
    Ok(Json(result))
}

#[utoipa::path(get, path = "/api/v1/governance/api-keys", tag = "governance",
    responses((status = 200, body = ApiKeyListResponse)))]
pub async fn list_api_keys(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<ApiKeyListResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_api_key_manage()).await?;
    let items = api_keys::list(&state.pool, user.ctx.org_id, &request_id).await?;
    Ok(Json(ApiKeyListResponse { items }))
}

#[utoipa::path(post, path = "/api/v1/governance/api-keys", tag = "governance",
    request_body = CreateApiKeyRequest, responses((status = 201, body = CreateApiKeyResponse)))]
pub async fn create_api_key(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_api_key_manage()).await?;
    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name required"));
    }
    let expires_at = parse_optional_rfc3339(body.expires_at.as_deref(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let (key, secret) = api_keys::create(
        &mut tx,
        user.ctx.org_id,
        user.ctx.actor.user_id,
        body.name.trim(),
        &body.scopes,
        expires_at,
        &request_id,
    )
    .await?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse { key, secret }),
    ))
}

#[utoipa::path(post, path = "/api/v1/governance/api-keys/{id}/rotate", tag = "governance",
    params(("id" = String, Path)), responses((status = 200, body = RotateApiKeyResponse)))]
pub async fn rotate_api_key(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RotateApiKeyResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_api_key_manage()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let (key, secret) = api_keys::rotate(&mut tx, user.ctx.org_id, &id, &request_id).await?;

    let envelope = EventEnvelope::new(
        user.ctx.org_id,
        Context::Admin,
        "api_key",
        "rotated",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({ "api_key_id": key.id }),
    );
    insert_event(&mut *tx, &envelope)
        .await
        .map_err(outbox_internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(RotateApiKeyResponse { key, secret }))
}

#[utoipa::path(post, path = "/api/v1/governance/api-keys/{id}/revoke", tag = "governance",
    params(("id" = String, Path)), responses((status = 200, body = MessageResponse)))]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_api_key_manage()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    api_keys::revoke(&mut tx, user.ctx.org_id, &id, &request_id).await?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(MessageResponse {
        message: "api key revoked".into(),
    }))
}
