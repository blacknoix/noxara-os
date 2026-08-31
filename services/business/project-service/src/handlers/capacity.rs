//! `/api/v1/operations/capacity` — allocations and overload view.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{
    internal, normalize_paging, not_found, parse_public_id, parse_user_ref, user_public, validation,
};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CapacityAllocationDto, CapacityAllocationListResponse, CapacityListQuery,
    CapacityOverloadResponse, CapacityOverloadRow, CreateCapacityAllocationRequest, OverloadQuery,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/operations/capacity/allocations",
            get(list_allocations).post(create_allocation),
        )
        .route(
            "/api/v1/operations/capacity/overload",
            get(overload_view),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AllocationRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    membership_user_id: Uuid,
    #[allow(dead_code)]
    project_id: Option<Uuid>,
    project_public_id: Option<String>,
    period_start: NaiveDate,
    period_end: NaiveDate,
    capacity_minutes: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const ALLOC_COLS: &str = r#"
    a.id, a.public_id, a.membership_user_id, a.project_id, p.public_id AS project_public_id,
    a.period_start, a.period_end, a.capacity_minutes, a.created_at, a.updated_at
"#;

fn parse_date(raw: &str, field: &str, request_id: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| validation(request_id, format!("{field} must be YYYY-MM-DD")))
}

fn row_to_dto(row: AllocationRow) -> CapacityAllocationDto {
    CapacityAllocationDto {
        id: row.public_id,
        membership_user_id: user_public(row.membership_user_id),
        project_id: row.project_public_id,
        period_start: row.period_start.to_string(),
        period_end: row.period_end.to_string(),
        capacity_minutes: row.capacity_minutes,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }
}

/// GET /api/v1/operations/capacity/allocations
#[utoipa::path(get, path = "/api/v1/operations/capacity/allocations", tag = "operations-capacity",
    params(CapacityListQuery),
    responses((status = 200, body = CapacityAllocationListResponse)))]
pub async fn list_allocations(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<CapacityListQuery>,
) -> Result<Json<CapacityAllocationListResponse>, AppError> {
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
        perms::operations_capacity_read(),
        &request_id,
    )?;

    let filter_user = q
        .membership_user_id
        .as_deref()
        .map(|s| parse_user_ref(s, &request_id))
        .transpose()?;
    let from = q
        .from
        .as_deref()
        .map(|s| parse_date(s, "from", &request_id))
        .transpose()?;
    let to = q
        .to
        .as_deref()
        .map(|s| parse_date(s, "to", &request_id))
        .transpose()?;
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint FROM operations_capacity_allocation
        WHERE org_id = $1
          AND ($2::uuid IS NULL OR membership_user_id = $2)
          AND ($3::date IS NULL OR period_end >= $3)
          AND ($4::date IS NULL OR period_start <= $4)
        "#,
    )
    .bind(org_id)
    .bind(filter_user)
    .bind(from)
    .bind(to)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let rows: Vec<AllocationRow> = sqlx::query_as(&format!(
        r#"
        SELECT {ALLOC_COLS}
        FROM operations_capacity_allocation a
        LEFT JOIN operations_project p ON p.id = a.project_id AND p.org_id = a.org_id
        WHERE a.org_id = $1
          AND ($2::uuid IS NULL OR a.membership_user_id = $2)
          AND ($3::date IS NULL OR a.period_end >= $3)
          AND ($4::date IS NULL OR a.period_start <= $4)
        ORDER BY a.period_start DESC, a.created_at DESC
        LIMIT $5 OFFSET $6
        "#
    ))
    .bind(org_id)
    .bind(filter_user)
    .bind(from)
    .bind(to)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(CapacityAllocationListResponse {
        items: rows.into_iter().map(row_to_dto).collect(),
        total,
    }))
}

/// POST /api/v1/operations/capacity/allocations
#[utoipa::path(post, path = "/api/v1/operations/capacity/allocations", tag = "operations-capacity",
    request_body = CreateCapacityAllocationRequest,
    responses((status = 201, body = CapacityAllocationDto)))]
pub async fn create_allocation(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateCapacityAllocationRequest>,
) -> Result<Response, AppError> {
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
        perms::operations_capacity_manage(),
        &request_id,
    )?;

    let member_id = parse_user_ref(&body.membership_user_id, &request_id)?;
    let period_start = parse_date(&body.period_start, "period_start", &request_id)?;
    let period_end = parse_date(&body.period_end, "period_end", &request_id)?;
    if period_end < period_start {
        return Err(validation(
            &request_id,
            "period_end must be on or after period_start",
        ));
    }
    if body.capacity_minutes <= 0 {
        return Err(validation(&request_id, "capacity_minutes must be > 0"));
    }

    let public_id = PublicId::generate(IdKind::CapacityAllocation);
    let id = public_id.uuid();
    let idem_key = idempotency::header_key(&headers);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "capacity.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let project_id = match body.project_id.as_deref() {
        None => None,
        Some(s) if s.trim().is_empty() => None,
        Some(s) => {
            let pid = parse_public_id(IdKind::Project, s, &request_id)?;
            let exists: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM operations_project WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
            )
            .bind(org_id)
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            Some(
                exists
                    .map(|r| r.0)
                    .ok_or_else(|| not_found(&request_id, "project"))?,
            )
        }
    };

    sqlx::query(
        r#"
        INSERT INTO operations_capacity_allocation (
            id, org_id, public_id, membership_user_id, project_id,
            period_start, period_end, capacity_minutes
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(member_id)
    .bind(project_id)
    .bind(period_start)
    .bind(period_end)
    .bind(body.capacity_minutes)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let row: AllocationRow = sqlx::query_as(&format!(
        "SELECT {ALLOC_COLS}
         FROM operations_capacity_allocation a
         LEFT JOIN operations_project p ON p.id = a.project_id AND p.org_id = a.org_id
         WHERE a.org_id = $1 AND a.id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let dto = row_to_dto(row);

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.capacity.create",
        "capacity_allocation",
        &dto.id,
        serde_json::json!({
            "membership_user_id": dto.membership_user_id,
            "capacity_minutes": dto.capacity_minutes,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let body_val = serde_json::to_value(&dto).unwrap_or_default();
    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "capacity.create",
            key,
            201,
            body_val.clone(),
        )
        .await
        .map_err(internal(&request_id))?;
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(body_val)).into_response())
}

/// GET /api/v1/operations/capacity/overload?from=&to=
#[utoipa::path(get, path = "/api/v1/operations/capacity/overload", tag = "operations-capacity",
    params(OverloadQuery),
    responses((status = 200, body = CapacityOverloadResponse)))]
pub async fn overload_view(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<OverloadQuery>,
) -> Result<Json<CapacityOverloadResponse>, AppError> {
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
        perms::operations_capacity_read(),
        &request_id,
    )?;

    let from = parse_date(&q.from, "from", &request_id)?;
    let to = parse_date(&q.to, "to", &request_id)?;
    if to < from {
        return Err(validation(&request_id, "to must be on or after from"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // Capacity overlapping [from, to]; booked = submitted|approved time entries in range.
    let rows: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        r#"
        WITH members AS (
            SELECT DISTINCT membership_user_id AS user_id
            FROM operations_capacity_allocation
            WHERE org_id = $1 AND period_end >= $2 AND period_start <= $3
            UNION
            SELECT DISTINCT membership_user_id
            FROM operations_time_entry
            WHERE org_id = $1
              AND entry_date >= $2 AND entry_date <= $3
              AND status IN ('submitted', 'approved')
        ),
        cap AS (
            SELECT membership_user_id AS user_id,
                   COALESCE(SUM(capacity_minutes), 0)::bigint AS capacity_minutes
            FROM operations_capacity_allocation
            WHERE org_id = $1 AND period_end >= $2 AND period_start <= $3
            GROUP BY membership_user_id
        ),
        booked AS (
            SELECT membership_user_id AS user_id,
                   COALESCE(SUM(minutes), 0)::bigint AS booked_minutes
            FROM operations_time_entry
            WHERE org_id = $1
              AND entry_date >= $2 AND entry_date <= $3
              AND status IN ('submitted', 'approved')
            GROUP BY membership_user_id
        )
        SELECT m.user_id,
               COALESCE(c.capacity_minutes, 0)::bigint,
               COALESCE(b.booked_minutes, 0)::bigint
        FROM members m
        LEFT JOIN cap c ON c.user_id = m.user_id
        LEFT JOIN booked b ON b.user_id = m.user_id
        ORDER BY m.user_id
        "#,
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    let items = rows
        .into_iter()
        .map(|(user_id, capacity, booked)| {
            let capacity_minutes = capacity as i32;
            let booked_minutes = booked as i32;
            CapacityOverloadRow {
                member_id: user_public(user_id),
                capacity_minutes,
                booked_minutes,
                overload_minutes: (booked_minutes - capacity_minutes).max(0),
            }
        })
        .collect();

    Ok(Json(CapacityOverloadResponse { items }))
}
