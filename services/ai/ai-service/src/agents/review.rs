//! Quarterly agent action review pack.
//!
//! Ships the report + fixture that computes error rate vs published threshold.
//! Live quarterly ops is out of PR scope.

use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

/// Published default threshold (5%).
pub const DEFAULT_MAX_ERROR_RATE: f64 = 0.05;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentReviewReport {
    pub period_start: chrono::DateTime<chrono::Utc>,
    pub period_end: chrono::DateTime<chrono::Utc>,
    pub total_actions: u64,
    pub failures: u64,
    pub reversals: u64,
    pub error_rate: f64,
    pub max_error_rate: f64,
    pub within_threshold: bool,
    pub by_agent_type: Vec<AgentTypeStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentTypeStats {
    pub agent_type: String,
    pub total: u64,
    pub failures: u64,
    pub reversals: u64,
}

pub async fn compute_review(
    state: &AppState,
    org_id: OrgId,
    period_start: chrono::DateTime<chrono::Utc>,
    period_end: chrono::DateTime<chrono::Utc>,
    request_id: &str,
) -> Result<AgentReviewReport, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let max_error_rate: f64 = sqlx::query_scalar(
        "SELECT max_error_rate FROM ai_agent_review_threshold WHERE org_id = $1",
    )
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?
    .unwrap_or(DEFAULT_MAX_ERROR_RATE);

    let totals: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*)::bigint,
            COUNT(*) FILTER (WHERE error = true OR status = 'failed')::bigint,
            COUNT(*) FILTER (WHERE status = 'reversed')::bigint
        FROM ai_action
        WHERE org_id = $1 AND created_at >= $2 AND created_at < $3
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(period_start)
    .bind(period_end)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let by_type: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        r#"
        SELECT
            agent_type,
            COUNT(*)::bigint,
            COUNT(*) FILTER (WHERE error = true OR status = 'failed')::bigint,
            COUNT(*) FILTER (WHERE status = 'reversed')::bigint
        FROM ai_action
        WHERE org_id = $1 AND created_at >= $2 AND created_at < $3
        GROUP BY agent_type
        ORDER BY agent_type
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(period_start)
    .bind(period_end)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let total = totals.0.max(0) as u64;
    let failures = totals.1.max(0) as u64;
    let reversals = totals.2.max(0) as u64;
    let error_rate = if total == 0 {
        0.0
    } else {
        failures as f64 / total as f64
    };

    Ok(AgentReviewReport {
        period_start,
        period_end,
        total_actions: total,
        failures,
        reversals,
        error_rate,
        max_error_rate,
        within_threshold: error_rate <= max_error_rate,
        by_agent_type: by_type
            .into_iter()
            .map(|(agent_type, t, f, r)| AgentTypeStats {
                agent_type,
                total: t.max(0) as u64,
                failures: f.max(0) as u64,
                reversals: r.max(0) as u64,
            })
            .collect(),
    })
}

/// Seed enough ai_action rows for a deterministic review fixture.
pub async fn seed_review_fixture(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<(), AppError> {
    use companyos_ids::{new_uuid_v7, IdKind, PublicId};
    use serde_json::json;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO ai_agent_review_threshold (org_id, max_error_rate)
        VALUES ($1, $2)
        ON CONFLICT (org_id) DO UPDATE SET max_error_rate = EXCLUDED.max_error_rate
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(DEFAULT_MAX_ERROR_RATE)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    // 20 actions, 1 failure → 5% error rate (at threshold).
    for i in 0..20 {
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::AiAction, id).as_str();
        let is_error = i == 0;
        sqlx::query(
            r#"
            INSERT INTO ai_action
                (id, org_id, public_id, agent_type, tool_name, permission, model,
                 prompt_template_version, tool_trace, command, effect, status, error)
            VALUES ($1,$2,$3,'receivables_chase','send_invoice_reminder','finance.invoice.send',
                    'mock','ai.agent.v1','[]'::jsonb,$4,$5,$6,$7)
            "#,
        )
        .bind(id)
        .bind(org_id.as_uuid())
        .bind(&public_id)
        .bind(json!({"fixture": i}))
        .bind(json!({"reminder": true}))
        .bind(if is_error { "failed" } else { "committed" })
        .bind(is_error)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        let _ = Uuid::nil();
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}
