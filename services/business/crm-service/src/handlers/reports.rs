//! `/api/v1/sales/reports/summary` — pipeline, win-rate, and forecast
//! aggregations computed directly from CRM tables (no separate warehouse).

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{IdKind, PublicId};
use companyos_money::{Currency, Money};
use companyos_tenancy::set_session_org_id;
use sqlx::Postgres;
use uuid::Uuid;

use super::internal;
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    ActivityVolumeItem, ReportSummaryResponse, StageSummary, WeightedForecast, WinRateSummary,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/sales/reports/summary", get(report_summary))
}

/// GET /api/v1/sales/reports/summary
#[utoipa::path(get, path = "/api/v1/sales/reports/summary", tag = "sales-reports",
    responses((status = 200, body = ReportSummaryResponse)))]
pub async fn report_summary(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ReportSummaryResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(&membership.principal, perms::sales_report_read(), &request_id)?;
    let deal_scope = scope_for_permission(&membership.principal, &perms::sales_deal_read());
    let activity_scope = scope_for_permission(&membership.principal, &perms::sales_activity_read());

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // --- pipeline_by_stage: open deals grouped by stage, scoped to the caller.
    #[derive(sqlx::FromRow)]
    struct StageAgg {
        stage_id: Uuid,
        stage_name: String,
        open_deal_count: i64,
        open_amount_minor: Option<i64>,
        currency: Option<String>,
    }
    let mut qb: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(
        r#"
        SELECT s.id AS stage_id, s.name AS stage_name,
               COUNT(d.id) AS open_deal_count,
               COALESCE(SUM(d.amount_minor), 0) AS open_amount_minor,
               MIN(d.currency) AS currency
        FROM sales_pipeline_stage s
        LEFT JOIN sales_deal d ON d.stage_id = s.id AND d.status = 'open' AND d.deleted_at IS NULL AND d.org_id = s.org_id
        "#,
    );
    // Owner-predicate needs to apply to the deal side; simplest correct way in a
    // LEFT JOIN aggregate is to fold it into the join condition so non-matching
    // deals don't suppress stages with zero visible deals.
    if !matches!(deal_scope, companyos_authz::Scope::Organization) {
        qb.push(" AND (");
        push_owner_predicate_join(&mut qb, deal_scope, org_id, actor, membership.team_id, membership.department_id);
        qb.push(")");
    }
    qb.push(" WHERE s.org_id = ");
    qb.push_bind(org_id);
    qb.push(" AND s.deleted_at IS NULL GROUP BY s.id, s.name, s.position ORDER BY s.position ASC");

    let stage_rows: Vec<StageAgg> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let pipeline_by_stage: Vec<StageSummary> = stage_rows
        .into_iter()
        .map(|r| StageSummary {
            stage_id: PublicId::new(IdKind::Stage, r.stage_id).as_str(),
            stage_name: r.stage_name,
            open_deal_count: r.open_deal_count,
            open_amount_minor: r.open_amount_minor.unwrap_or(0),
            currency: r.currency.unwrap_or_else(|| "USD".to_string()),
        })
        .collect();

    // --- win_rate: all-time won vs lost counts, scoped to the caller.
    let mut won_qb: sqlx::QueryBuilder<Postgres> =
        sqlx::QueryBuilder::new("SELECT COUNT(*) FROM sales_deal WHERE org_id = ");
    won_qb.push_bind(org_id);
    won_qb.push(" AND status = 'won' AND deleted_at IS NULL");
    push_owner_predicate(&mut won_qb, deal_scope, org_id, actor, membership.team_id, membership.department_id);
    let won_count: i64 = won_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut lost_qb: sqlx::QueryBuilder<Postgres> =
        sqlx::QueryBuilder::new("SELECT COUNT(*) FROM sales_deal WHERE org_id = ");
    lost_qb.push_bind(org_id);
    lost_qb.push(" AND status = 'lost' AND deleted_at IS NULL");
    push_owner_predicate(&mut lost_qb, deal_scope, org_id, actor, membership.team_id, membership.department_id);
    let lost_count: i64 = lost_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let decided = won_count + lost_count;
    let win_rate_pct = if decided == 0 {
        0.0
    } else {
        (won_count as f64 / decided as f64) * 100.0
    };

    // --- activity_volume: counts grouped by kind, scoped to the caller.
    #[derive(sqlx::FromRow)]
    struct ActivityAgg {
        kind: String,
        count: i64,
    }
    let mut act_qb: sqlx::QueryBuilder<Postgres> =
        sqlx::QueryBuilder::new("SELECT kind, COUNT(*) AS count FROM sales_activity WHERE org_id = ");
    act_qb.push_bind(org_id);
    act_qb.push(" AND deleted_at IS NULL");
    push_owner_predicate(&mut act_qb, activity_scope, org_id, actor, membership.team_id, membership.department_id);
    act_qb.push(" GROUP BY kind ORDER BY kind ASC");
    let activity_rows: Vec<ActivityAgg> = act_qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    let activity_volume: Vec<ActivityVolumeItem> = activity_rows
        .into_iter()
        .map(|r| ActivityVolumeItem { kind: r.kind, count: r.count })
        .collect();

    // --- weighted_forecast: sum(amount_minor * probability / 100) for open
    // USD deals visible to the caller. Multi-currency portfolios need a
    // per-currency breakdown; USD-only is a documented v1 limitation.
    let mut fc_qb: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(
        "SELECT amount_minor, COALESCE(probability, 0) AS probability FROM sales_deal WHERE org_id = ",
    );
    fc_qb.push_bind(org_id);
    fc_qb.push(" AND status = 'open' AND deleted_at IS NULL AND currency = 'USD'");
    push_owner_predicate(&mut fc_qb, deal_scope, org_id, actor, membership.team_id, membership.department_id);
    let forecast_rows: Vec<(i64, i32)> = fc_qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let usd = Currency::USD;
    let mut weighted_total: i64 = 0;
    for (amount_minor, probability) in forecast_rows {
        let weighted = Money::new(amount_minor, usd)
            .scale_half_up(probability as i64, 100)
            .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        weighted_total = weighted_total
            .checked_add(weighted.amount_minor)
            .ok_or_else(|| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), "forecast overflow"))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ReportSummaryResponse {
        pipeline_by_stage,
        win_rate: WinRateSummary {
            won_count,
            lost_count,
            win_rate_pct,
        },
        activity_volume,
        weighted_forecast: WeightedForecast {
            amount_minor: weighted_total,
            currency: "USD".to_string(),
        },
    }))
}

/// Same shape as [`push_owner_predicate`] but written against the `d.`
/// (deal) table alias and with no leading ` AND ` — used inside a
/// `LEFT JOIN ... ON (...)` clause instead of a `WHERE`. Callers only invoke
/// this for non-`Organization` scopes.
fn push_owner_predicate_join(
    qb: &mut sqlx::QueryBuilder<'_, Postgres>,
    scope: companyos_authz::Scope,
    org_id: Uuid,
    actor_user_id: Uuid,
    team_id: Option<Uuid>,
    department_id: Option<Uuid>,
) {
    match scope {
        companyos_authz::Scope::Own => {
            qb.push("d.owner_user_id = ");
            qb.push_bind(actor_user_id);
        }
        companyos_authz::Scope::Team => {
            qb.push("(d.owner_user_id = ");
            qb.push_bind(actor_user_id);
            if let Some(team) = team_id {
                qb.push(" OR d.owner_user_id IN (SELECT user_id FROM membership WHERE org_id = ");
                qb.push_bind(org_id);
                qb.push(" AND team_id = ");
                qb.push_bind(team);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
        companyos_authz::Scope::Department => {
            qb.push("(d.owner_user_id = ");
            qb.push_bind(actor_user_id);
            if let Some(dept) = department_id {
                qb.push(" OR d.owner_user_id IN (SELECT user_id FROM membership WHERE org_id = ");
                qb.push_bind(org_id);
                qb.push(" AND department_id = ");
                qb.push_bind(dept);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
        companyos_authz::Scope::Organization => {}
    }
}
