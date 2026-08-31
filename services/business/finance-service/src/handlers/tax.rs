//! `/api/v1/finance/tax/...` — tax groups + append-only versioned rates.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{internal, not_found, parse_optional_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateTaxGroupRequest, CreateTaxRateRequest, TaxGroupDto, TaxGroupListResponse, TaxRateDto,
    TaxRateListResponse, TaxResolveQuery, TaxResolveResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/tax/groups",
            get(list_tax_groups).post(create_tax_group),
        )
        .route(
            "/api/v1/finance/tax/rates",
            get(list_tax_rates).post(create_tax_rate),
        )
        .route("/api/v1/finance/tax/resolve", get(resolve_tax))
}

#[derive(Debug, sqlx::FromRow)]
struct GroupRow {
    public_id: String,
    name: String,
    description: Option<String>,
    created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct RateRow {
    public_id: String,
    name: String,
    rate_bps: i64,
    valid_from: NaiveDate,
    valid_to: Option<NaiveDate>,
    tax_group_public_id: Option<String>,
    supersedes_public_id: Option<String>,
    component_name: Option<String>,
    is_component: bool,
    created_at: chrono::DateTime<Utc>,
}

fn parse_date(raw: &str, field: &str, request_id: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| validation(request_id, format!("{field} must be YYYY-MM-DD")))
}

/// Resolve rate_bps for a tax group (or concrete rate) as of a date.
/// Used by the resolve endpoint and invoice issue path.
pub async fn resolve_rate_bps(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    tax_group_id: Option<Uuid>,
    tax_rate_id: Option<Uuid>,
    as_of: NaiveDate,
) -> Result<Option<(Uuid, i64, Option<Uuid>)>, sqlx::Error> {
    if let Some(rate_id) = tax_rate_id {
        let row: Option<(Uuid, i64, Option<Uuid>, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
            r#"
            SELECT id, rate_bps, tax_group_id, valid_from, valid_to
            FROM finance_tax_rate
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(rate_id)
        .fetch_optional(&mut **tx)
        .await?;
        return Ok(row.and_then(|(id, bps, gid, from, to)| {
            if from <= as_of && to.map(|t| t >= as_of).unwrap_or(true) {
                Some((id, bps, gid))
            } else {
                // Still return the rate if explicitly referenced (snapshot intent),
                // but prefer validity window when present.
                Some((id, bps, gid))
            }
        }));
    }
    let Some(group_id) = tax_group_id else {
        return Ok(None);
    };
    let row: Option<(Uuid, i64, Option<Uuid>)> = sqlx::query_as(
        r#"
        SELECT id, rate_bps, tax_group_id
        FROM finance_tax_rate
        WHERE org_id = $1
          AND tax_group_id = $2
          AND is_component = false
          AND valid_from <= $3
          AND (valid_to IS NULL OR valid_to >= $3)
        ORDER BY valid_from DESC
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(group_id)
    .bind(as_of)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row)
}

/// GET /api/v1/finance/tax/groups
#[utoipa::path(get, path = "/api/v1/finance/tax/groups", tag = "finance-tax",
    responses((status = 200, body = TaxGroupListResponse)))]
pub async fn list_tax_groups(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<TaxGroupListResponse>, AppError> {
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
        perms::finance_tax_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<GroupRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, description, created_at
        FROM finance_tax_group WHERE org_id = $1 ORDER BY name ASC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let total = rows.len() as i64;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(TaxGroupListResponse {
        items: rows
            .into_iter()
            .map(|r| TaxGroupDto {
                id: r.public_id,
                name: r.name,
                description: r.description,
                created_at: r.created_at.to_rfc3339(),
            })
            .collect(),
        total,
    }))
}

/// POST /api/v1/finance/tax/groups
#[utoipa::path(post, path = "/api/v1/finance/tax/groups", tag = "finance-tax",
    request_body = CreateTaxGroupRequest,
    responses((status = 201, body = TaxGroupDto)))]
pub async fn create_tax_group(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateTaxGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
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
        perms::finance_tax_manage(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name is required"));
    }

    let public_id = PublicId::generate(IdKind::TaxGroup);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO finance_tax_group (id, org_id, public_id, name, description)
        VALUES ($1,$2,$3,$4,$5)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(body.description.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.tax_group.create",
        "tax_group",
        &public_id.as_str(),
        serde_json::json!({ "name": body.name.trim() }),
    )
    .await
    .map_err(internal(&request_id))?;

    let row: GroupRow = sqlx::query_as(
        "SELECT public_id, name, description, created_at FROM finance_tax_group WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok((
        StatusCode::CREATED,
        Json(TaxGroupDto {
            id: row.public_id,
            name: row.name,
            description: row.description,
            created_at: row.created_at.to_rfc3339(),
        }),
    ))
}

/// POST /api/v1/finance/tax/rates — create a new version (never PATCH rate_bps).
#[utoipa::path(post, path = "/api/v1/finance/tax/rates", tag = "finance-tax",
    request_body = CreateTaxRateRequest,
    responses((status = 201, body = TaxRateDto)))]
pub async fn create_tax_rate(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateTaxRateRequest>,
) -> Result<impl IntoResponse, AppError> {
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
        perms::finance_tax_manage(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name is required"));
    }
    if body.rate_bps < 0 {
        return Err(validation(&request_id, "rate_bps must be >= 0"));
    }
    let valid_from = parse_date(&body.valid_from, "valid_from", &request_id)?;
    let valid_to = body
        .valid_to
        .as_deref()
        .map(|s| parse_date(s, "valid_to", &request_id))
        .transpose()?;
    if let Some(to) = valid_to {
        if to < valid_from {
            return Err(validation(&request_id, "valid_to must be >= valid_from"));
        }
    }

    let group_uuid =
        parse_optional_public_id(IdKind::TaxGroup, body.tax_group_id.as_deref(), &request_id)?;
    let supersedes_uuid =
        parse_optional_public_id(IdKind::TaxRate, body.supersedes_id.as_deref(), &request_id)?;

    let public_id = PublicId::generate(IdKind::TaxRate);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut resolved_group = group_uuid;
    if let Some(prev) = supersedes_uuid {
        let prev_row: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT tax_group_id FROM finance_tax_rate WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(prev)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal(&request_id))?;
        let Some((prev_group,)) = prev_row else {
            return Err(not_found(&request_id, "superseded tax rate"));
        };
        if resolved_group.is_none() {
            resolved_group = prev_group;
        }
        // Close the previous version the day before the new valid_from.
        let close_to = valid_from
            .pred_opt()
            .unwrap_or(valid_from);
        sqlx::query(
            r#"
            UPDATE finance_tax_rate
            SET valid_to = COALESCE(valid_to, $3)
            WHERE org_id = $1 AND id = $2 AND (valid_to IS NULL OR valid_to >= $4)
            "#,
        )
        .bind(org_id)
        .bind(prev)
        .bind(close_to)
        .bind(valid_from)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    if let Some(gid) = resolved_group {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM finance_tax_group WHERE org_id = $1 AND id = $2)",
        )
        .bind(org_id)
        .bind(gid)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        if !exists {
            return Err(not_found(&request_id, "tax group"));
        }
    }

    sqlx::query(
        r#"
        INSERT INTO finance_tax_rate (
            id, org_id, public_id, name, rate_bps, valid_from, valid_to,
            tax_group_id, supersedes_id, component_name, is_component
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(body.rate_bps)
    .bind(valid_from)
    .bind(valid_to)
    .bind(resolved_group)
    .bind(supersedes_uuid)
    .bind(body.component_name.as_deref())
    .bind(body.is_component)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.tax_rate.create",
        "tax_rate",
        &public_id.as_str(),
        serde_json::json!({
            "rate_bps": body.rate_bps,
            "valid_from": body.valid_from,
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_rate_dto(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "tax rate"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn fetch_rate_dto(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    id: Uuid,
) -> Result<Option<TaxRateDto>, sqlx::Error> {
    let row: Option<RateRow> = sqlx::query_as(
        r#"
        SELECT r.public_id, r.name, r.rate_bps, r.valid_from, r.valid_to,
               g.public_id AS tax_group_public_id,
               s.public_id AS supersedes_public_id,
               r.component_name, r.is_component, r.created_at
        FROM finance_tax_rate r
        LEFT JOIN finance_tax_group g ON g.id = r.tax_group_id
        LEFT JOIN finance_tax_rate s ON s.id = r.supersedes_id
        WHERE r.org_id = $1 AND r.id = $2
        "#,
    )
    .bind(org_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| TaxRateDto {
        id: r.public_id,
        name: r.name,
        rate_bps: r.rate_bps,
        valid_from: r.valid_from.to_string(),
        valid_to: r.valid_to.map(|d| d.to_string()),
        tax_group_id: r.tax_group_public_id,
        supersedes_id: r.supersedes_public_id,
        component_name: r.component_name,
        is_component: r.is_component,
        created_at: r.created_at.to_rfc3339(),
    }))
}

/// GET /api/v1/finance/tax/rates?group_id=&as_of=YYYY-MM-DD
#[utoipa::path(get, path = "/api/v1/finance/tax/rates", tag = "finance-tax",
    params(
        ("group_id" = Option<String>, Query),
        ("as_of" = Option<String>, Query),
    ),
    responses((status = 200, body = TaxRateListResponse)))]
pub async fn list_tax_rates(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<TaxResolveQuery>,
) -> Result<Json<TaxRateListResponse>, AppError> {
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
        perms::finance_tax_read(),
        &request_id,
    )?;

    let group_uuid =
        parse_optional_public_id(IdKind::TaxGroup, q.group_id.as_deref(), &request_id)?;
    let as_of = q
        .as_of
        .as_deref()
        .map(|s| parse_date(s, "as_of", &request_id))
        .transpose()?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<RateRow> = if let Some(as_of) = as_of {
        if let Some(gid) = group_uuid {
            sqlx::query_as(
                r#"
                SELECT r.public_id, r.name, r.rate_bps, r.valid_from, r.valid_to,
                       g.public_id AS tax_group_public_id,
                       s.public_id AS supersedes_public_id,
                       r.component_name, r.is_component, r.created_at
                FROM finance_tax_rate r
                LEFT JOIN finance_tax_group g ON g.id = r.tax_group_id
                LEFT JOIN finance_tax_rate s ON s.id = r.supersedes_id
                WHERE r.org_id = $1 AND r.tax_group_id = $2
                  AND r.valid_from <= $3
                  AND (r.valid_to IS NULL OR r.valid_to >= $3)
                ORDER BY r.valid_from DESC
                "#,
            )
            .bind(org_id)
            .bind(gid)
            .bind(as_of)
            .fetch_all(&mut *tx)
            .await
        } else {
            sqlx::query_as(
                r#"
                SELECT r.public_id, r.name, r.rate_bps, r.valid_from, r.valid_to,
                       g.public_id AS tax_group_public_id,
                       s.public_id AS supersedes_public_id,
                       r.component_name, r.is_component, r.created_at
                FROM finance_tax_rate r
                LEFT JOIN finance_tax_group g ON g.id = r.tax_group_id
                LEFT JOIN finance_tax_rate s ON s.id = r.supersedes_id
                WHERE r.org_id = $1
                  AND r.valid_from <= $2
                  AND (r.valid_to IS NULL OR r.valid_to >= $2)
                ORDER BY r.valid_from DESC
                "#,
            )
            .bind(org_id)
            .bind(as_of)
            .fetch_all(&mut *tx)
            .await
        }
    } else if let Some(gid) = group_uuid {
        sqlx::query_as(
            r#"
            SELECT r.public_id, r.name, r.rate_bps, r.valid_from, r.valid_to,
                   g.public_id AS tax_group_public_id,
                   s.public_id AS supersedes_public_id,
                   r.component_name, r.is_component, r.created_at
            FROM finance_tax_rate r
            LEFT JOIN finance_tax_group g ON g.id = r.tax_group_id
            LEFT JOIN finance_tax_rate s ON s.id = r.supersedes_id
            WHERE r.org_id = $1 AND r.tax_group_id = $2
            ORDER BY r.valid_from DESC
            "#,
        )
        .bind(org_id)
        .bind(gid)
        .fetch_all(&mut *tx)
        .await
    } else {
        sqlx::query_as(
            r#"
            SELECT r.public_id, r.name, r.rate_bps, r.valid_from, r.valid_to,
                   g.public_id AS tax_group_public_id,
                   s.public_id AS supersedes_public_id,
                   r.component_name, r.is_component, r.created_at
            FROM finance_tax_rate r
            LEFT JOIN finance_tax_group g ON g.id = r.tax_group_id
            LEFT JOIN finance_tax_rate s ON s.id = r.supersedes_id
            WHERE r.org_id = $1
            ORDER BY r.valid_from DESC
            "#,
        )
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await
    }
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(TaxRateListResponse {
        items: rows
            .into_iter()
            .map(|r| TaxRateDto {
                id: r.public_id,
                name: r.name,
                rate_bps: r.rate_bps,
                valid_from: r.valid_from.to_string(),
                valid_to: r.valid_to.map(|d| d.to_string()),
                tax_group_id: r.tax_group_public_id,
                supersedes_id: r.supersedes_public_id,
                component_name: r.component_name,
                is_component: r.is_component,
                created_at: r.created_at.to_rfc3339(),
            })
            .collect(),
    }))
}

/// GET /api/v1/finance/tax/resolve?group_id=&as_of=
#[utoipa::path(get, path = "/api/v1/finance/tax/resolve", tag = "finance-tax",
    params(
        ("group_id" = Option<String>, Query),
        ("rate_id" = Option<String>, Query),
        ("as_of" = Option<String>, Query),
    ),
    responses((status = 200, body = TaxResolveResponse)))]
pub async fn resolve_tax(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<TaxResolveQuery>,
) -> Result<Json<TaxResolveResponse>, AppError> {
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
        perms::finance_tax_read(),
        &request_id,
    )?;

    let as_of = q
        .as_of
        .as_deref()
        .map(|s| parse_date(s, "as_of", &request_id))
        .transpose()?
        .unwrap_or_else(|| Utc::now().date_naive());
    let group_uuid =
        parse_optional_public_id(IdKind::TaxGroup, q.group_id.as_deref(), &request_id)?;
    let rate_uuid = parse_optional_public_id(IdKind::TaxRate, q.rate_id.as_deref(), &request_id)?;
    if group_uuid.is_none() && rate_uuid.is_none() {
        return Err(validation(
            &request_id,
            "group_id or rate_id is required",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let resolved = resolve_rate_bps(&mut tx, org_id, group_uuid, rate_uuid, as_of)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "tax rate for as_of date"))?;

    let rate_public: String =
        sqlx::query_scalar("SELECT public_id FROM finance_tax_rate WHERE id = $1")
            .bind(resolved.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    let group_public: Option<String> = if let Some(gid) = resolved.2 {
        sqlx::query_scalar("SELECT public_id FROM finance_tax_group WHERE id = $1")
            .bind(gid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?
    } else {
        None
    };
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(TaxResolveResponse {
        rate_bps: resolved.1,
        tax_rate_id: Some(rate_public),
        tax_group_id: group_public,
        as_of: as_of.to_string(),
    }))
}
