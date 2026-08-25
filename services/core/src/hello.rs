//! Hello resource — tenant-scoped vertical slice.

use axum::extract::{Json, State};
use axum::routing::get;
use axum::Router;
use companyos_authz::{self as authz, perms};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::LocalAuth;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct Hello {
    /// Prefixed public id (`hel_…`).
    pub id: String,
    /// Prefixed org id (`org_…`).
    pub org_id: String,
    pub message: String,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateHelloRequest {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HelloListResponse {
    pub items: Vec<Hello>,
}

#[derive(Debug, sqlx::FromRow)]
struct HelloRow {
    public_id: String,
    org_id: Uuid,
    message: String,
    created_by: Uuid,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/hello", get(list_hello).post(create_hello))
}

/// GET /api/v1/hello — list hello messages for the caller's org only.
#[utoipa::path(
    get,
    path = "/api/v1/hello",
    responses(
        (status = 200, description = "Hello messages for the tenant", body = HelloListResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    ),
    tag = "hello"
)]
pub async fn list_hello(
    State(state): State<AppState>,
    LocalAuth(ctx): LocalAuth,
) -> Result<Json<HelloListResponse>, AppError> {
    enforce_dashboard_read(&ctx)?;

    let mut tx = state.pool.begin().await.map_err(internal(&ctx))?;
    set_session_org_id(&mut tx, ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, ctx.request_id.clone(), e.to_string()))?;

    let rows: Vec<HelloRow> = sqlx::query_as(
        r#"
        SELECT public_id, org_id, message, created_by
        FROM hello_message
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&ctx))?;
    tx.commit().await.map_err(internal(&ctx))?;

    let items = rows
        .into_iter()
        .map(|r| Hello {
            id: r.public_id,
            org_id: companyos_tenancy::OrgId::new(r.org_id).to_public().as_str(),
            message: r.message,
            created_by: PublicId::new(IdKind::User, r.created_by).as_str(),
        })
        .collect::<Vec<_>>();

    tracing::info!(
        request_id = %ctx.request_id,
        org_id = %ctx.org_id,
        count = items.len(),
        "list hello"
    );

    Ok(Json(HelloListResponse { items }))
}

/// POST /api/v1/hello — write hello row + outbox + audit in one transaction.
#[utoipa::path(
    post,
    path = "/api/v1/hello",
    request_body = CreateHelloRequest,
    responses(
        (status = 200, description = "Created hello", body = Hello),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    ),
    tag = "hello"
)]
pub async fn create_hello(
    State(state): State<AppState>,
    LocalAuth(ctx): LocalAuth,
    Json(body): Json<CreateHelloRequest>,
) -> Result<Json<Hello>, AppError> {
    enforce_dashboard_read(&ctx)?;

    if body.message.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            ctx.request_id.clone(),
            "message must not be empty",
        ));
    }

    let id = companyos_ids::new_uuid_v7();
    let public_id = PublicId::new(IdKind::Hello, id);

    let mut tx = state.pool.begin().await.map_err(internal(&ctx))?;
    set_session_org_id(&mut tx, ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, ctx.request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO hello_message (id, org_id, public_id, message, created_by)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(&body.message)
    .bind(ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&ctx))?;

    let envelope = EventEnvelope::new(
        ctx.org_id,
        Context::Core,
        "hello",
        "created",
        1,
        ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "message": body.message,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, ctx.request_id.clone(), e.to_string()))?;

    let audit_id = companyos_ids::new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO audit_entry (
            id, org_id, actor_user_id, actor_on_behalf_of, actor_is_ai,
            action, resource_type, resource_id, metadata
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(audit_id)
    .bind(ctx.org_id.as_uuid())
    .bind(ctx.actor.user_id)
    .bind(ctx.actor.on_behalf_of)
    .bind(ctx.actor.is_ai)
    .bind("hello.create")
    .bind("hello")
    .bind(public_id.as_str())
    .bind(serde_json::json!({ "message": body.message }))
    .execute(&mut *tx)
    .await
    .map_err(internal(&ctx))?;

    tx.commit().await.map_err(internal(&ctx))?;

    tracing::info!(
        request_id = %ctx.request_id,
        org_id = %ctx.org_id,
        hello_id = %public_id,
        event_subject = %envelope.subject,
        "created hello + outbox + audit"
    );

    Ok(Json(Hello {
        id: public_id.as_str(),
        org_id: ctx.org_id.to_public().as_str(),
        message: body.message,
        created_by: PublicId::new(IdKind::User, ctx.actor.user_id).as_str(),
    }))
}

fn enforce_dashboard_read(ctx: &companyos_tenancy::RequestContext) -> Result<(), AppError> {
    let principal = authz::Principal::with_roles(vec![authz::Role::Member]);
    if !authz::is_allowed(&principal, &perms::workspace_dashboard_read()) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            ctx.request_id.clone(),
            "missing workspace.dashboard.read",
        ));
    }
    Ok(())
}

fn internal(ctx: &companyos_tenancy::RequestContext) -> impl Fn(sqlx::Error) -> AppError + '_ {
    move |e| AppError::new(ErrorCode::Internal, ctx.request_id.clone(), e.to_string())
}
