//! `/api/v1/sales/activities` — calls, meetings, emails, and notes.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::Postgres;
use uuid::Uuid;

use super::{internal, validation};
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{ActivityDto, ActivityListQuery, ActivityListResponse, CreateActivityRequest};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/sales/activities", get(list_activities).post(create_activity))
}

#[derive(Debug, sqlx::FromRow)]
struct ActivityRow {
    public_id: String,
    kind: String,
    subject: Option<String>,
    body: Option<String>,
    occurred_at: DateTime<Utc>,
    customer_id: Option<Uuid>,
    deal_id: Option<Uuid>,
    lead_id: Option<Uuid>,
    owner_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

const ACTIVITY_COLUMNS: &str = "public_id, kind, subject, body, occurred_at, customer_id, deal_id, lead_id, owner_user_id, created_at";

impl ActivityRow {
    fn into_dto(self) -> ActivityDto {
        ActivityDto {
            id: self.public_id,
            kind: self.kind,
            subject: self.subject,
            body: self.body,
            occurred_at: self.occurred_at.to_rfc3339(),
            customer_id: self.customer_id.map(|u| PublicId::new(IdKind::Customer, u).as_str()),
            deal_id: self.deal_id.map(|u| PublicId::new(IdKind::Deal, u).as_str()),
            lead_id: self.lead_id.map(|u| PublicId::new(IdKind::Lead, u).as_str()),
            owner_user_id: self.owner_user_id.map(|u| PublicId::new(IdKind::User, u).as_str()),
            created_at: self.created_at.to_rfc3339(),
        }
    }
}

/// GET /api/v1/sales/activities?customer_id=&deal_id=&lead_id=
#[utoipa::path(get, path = "/api/v1/sales/activities", tag = "sales-activities",
    responses((status = 200, body = ActivityListResponse)))]
pub async fn list_activities(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ActivityListQuery>,
) -> Result<Json<ActivityListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(&membership.principal, perms::sales_activity_read(), &request_id)?;
    let scope = scope_for_permission(&membership.principal, &perms::sales_activity_read());
    let (limit, offset) = super::normalize_paging(q.limit, q.offset);

    let customer_id = match q.customer_id.as_deref() {
        Some(s) => Some(super::parse_public_id(IdKind::Customer, s, &request_id)?),
        None => None,
    };
    let deal_id = match q.deal_id.as_deref() {
        Some(s) => Some(super::parse_public_id(IdKind::Deal, s, &request_id)?),
        None => None,
    };
    let lead_id = match q.lead_id.as_deref() {
        Some(s) => Some(super::parse_public_id(IdKind::Lead, s, &request_id)?),
        None => None,
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut qb: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(format!(
        "SELECT {ACTIVITY_COLUMNS} FROM sales_activity WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    push_owner_predicate(&mut qb, scope, org_id, actor, membership.team_id, membership.department_id);
    if let Some(c) = customer_id {
        qb.push(" AND customer_id = ");
        qb.push_bind(c);
    }
    if let Some(d) = deal_id {
        qb.push(" AND deal_id = ");
        qb.push_bind(d);
    }
    if let Some(l) = lead_id {
        qb.push(" AND lead_id = ");
        qb.push_bind(l);
    }
    qb.push(" ORDER BY occurred_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<ActivityRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ActivityListResponse {
        items: rows.into_iter().map(ActivityRow::into_dto).collect(),
    }))
}

/// POST /api/v1/sales/activities
#[utoipa::path(post, path = "/api/v1/sales/activities", tag = "sales-activities",
    request_body = CreateActivityRequest,
    responses((status = 201, body = ActivityDto)))]
pub async fn create_activity(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateActivityRequest>,
) -> Result<(StatusCode, Json<ActivityDto>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce_any_scope(&membership.principal, perms::sales_activity_create(), &request_id)?;

    if !matches!(body.kind.as_str(), "note" | "call" | "meeting" | "email") {
        return Err(validation(
            &request_id,
            "kind must be one of note|call|meeting|email",
        ));
    }

    let customer_id = super::parse_optional_public_id(IdKind::Customer, body.customer_id.as_deref(), &request_id)?;
    let deal_id = super::parse_optional_public_id(IdKind::Deal, body.deal_id.as_deref(), &request_id)?;
    let lead_id = super::parse_optional_public_id(IdKind::Lead, body.lead_id.as_deref(), &request_id)?;
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => super::parse_public_id(IdKind::User, s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };
    let occurred_at = match body.occurred_at.as_deref() {
        Some(s) => Some(
            DateTime::parse_from_rfc3339(s)
                .map_err(|_| validation(&request_id, "occurred_at must be RFC3339"))?
                .with_timezone(&Utc),
        ),
        None => None,
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Activity, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: ActivityRow = sqlx::query_as(&format!(
        r#"
        INSERT INTO sales_activity (
            id, org_id, public_id, kind, subject, body, occurred_at, customer_id, deal_id, lead_id, owner_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,COALESCE($7, now()),$8,$9,$10,$11)
        RETURNING {ACTIVITY_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&body.kind)
    .bind(&body.subject)
    .bind(&body.body)
    .bind(occurred_at)
    .bind(customer_id)
    .bind(deal_id)
    .bind(lead_id)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Sales,
        "activity",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": public_id.as_str(), "kind": body.kind }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(row.into_dto())))
}
