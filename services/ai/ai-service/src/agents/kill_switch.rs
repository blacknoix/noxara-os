//! Org-wide (and optional per-agent) kill switch.
//!
//! Consulted on every agent tool call. Engaging the switch marks in-flight
//! runs as killed and refuses new runs. CI bound: halt within ≤ 2s.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

/// In-process cache so same-pod agents observe kill within milliseconds.
#[derive(Clone, Default)]
pub struct KillSwitchCache {
    inner: Arc<Mutex<HashMap<(Uuid, String), bool>>>,
}

impl KillSwitchCache {
    pub fn set(&self, org_id: Uuid, agent_type: &str, engaged: bool) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert((org_id, agent_type.to_string()), engaged);
            if agent_type == "*" {
                // Org-wide also stamps a wildcard entry.
                g.insert((org_id, "*".into()), engaged);
            }
        }
    }

    pub fn get(&self, org_id: Uuid, agent_type: &str) -> Option<bool> {
        let g = self.inner.lock().ok()?;
        if let Some(v) = g.get(&(org_id, "*".into())) {
            if *v {
                return Some(true);
            }
        }
        g.get(&(org_id, agent_type.to_string())).copied()
    }

    pub fn clear_org(&self, org_id: Uuid) {
        if let Ok(mut g) = self.inner.lock() {
            g.retain(|(o, _), _| *o != org_id);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KillSwitchView {
    pub org_wide: bool,
    pub agent_type: String,
    pub engaged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engaged_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetKillSwitchRequest {
    pub engaged: bool,
    #[serde(default = "default_star")]
    pub agent_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn default_star() -> String {
    "*".into()
}

pub async fn is_killed(
    state: &AppState,
    org_id: OrgId,
    agent_type: &str,
    request_id: &str,
) -> Result<bool, AppError> {
    if let Some(cached) = state.kill_switch_cache.get(org_id.as_uuid(), agent_type) {
        if cached {
            return Ok(true);
        }
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let org_wide: Option<bool> = sqlx::query_scalar(
        "SELECT engaged FROM ai_agent_kill_switch WHERE org_id = $1 AND agent_type = '*'",
    )
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let per_agent: Option<bool> = if agent_type != "*" {
        sqlx::query_scalar(
            "SELECT engaged FROM ai_agent_kill_switch WHERE org_id = $1 AND agent_type = $2",
        )
        .bind(org_id.as_uuid())
        .bind(agent_type)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?
    } else {
        None
    };

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let killed = org_wide.unwrap_or(false) || per_agent.unwrap_or(false);
    state
        .kill_switch_cache
        .set(org_id.as_uuid(), "*", org_wide.unwrap_or(false));
    if agent_type != "*" {
        state
            .kill_switch_cache
            .set(org_id.as_uuid(), agent_type, per_agent.unwrap_or(false));
    }
    Ok(killed)
}

pub async fn set_kill_switch(
    state: &AppState,
    org_id: OrgId,
    engaged_by: Uuid,
    req: &SetKillSwitchRequest,
    request_id: &str,
) -> Result<KillSwitchView, AppError> {
    let agent_type = if req.agent_type.is_empty() {
        "*"
    } else {
        req.agent_type.as_str()
    };

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
        INSERT INTO ai_agent_kill_switch (org_id, agent_type, engaged, engaged_by, engaged_at, reason, updated_at)
        VALUES ($1, $2, $3, $4, CASE WHEN $3 THEN now() ELSE NULL END, $5, now())
        ON CONFLICT (org_id, agent_type) DO UPDATE SET
            engaged = EXCLUDED.engaged,
            engaged_by = EXCLUDED.engaged_by,
            engaged_at = CASE WHEN EXCLUDED.engaged THEN now() ELSE NULL END,
            reason = EXCLUDED.reason,
            updated_at = now()
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(agent_type)
    .bind(req.engaged)
    .bind(engaged_by)
    .bind(&req.reason)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    if req.engaged {
        // Pause / kill in-flight agent runs for this org (and Temporal workflow ids).
        if agent_type == "*" {
            sqlx::query(
                r#"
                UPDATE ai_agent_run
                SET status = 'killed', finished_at = now(),
                    error_message = COALESCE(error_message, 'kill switch engaged')
                WHERE org_id = $1 AND status IN ('running', 'waiting')
                "#,
            )
            .bind(org_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        } else {
            sqlx::query(
                r#"
                UPDATE ai_agent_run
                SET status = 'killed', finished_at = now(),
                    error_message = COALESCE(error_message, 'kill switch engaged')
                WHERE org_id = $1 AND agent_type = $2 AND status IN ('running', 'waiting')
                "#,
            )
            .bind(org_id.as_uuid())
            .bind(agent_type)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        }
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    state
        .kill_switch_cache
        .set(org_id.as_uuid(), agent_type, req.engaged);

    Ok(KillSwitchView {
        org_wide: agent_type == "*",
        agent_type: agent_type.to_string(),
        engaged: req.engaged,
        reason: req.reason.clone(),
        engaged_at: if req.engaged {
            Some(chrono::Utc::now())
        } else {
            None
        },
    })
}

pub async fn get_kill_switch(
    state: &AppState,
    org_id: OrgId,
    request_id: &str,
) -> Result<KillSwitchView, AppError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let row: Option<(bool, Option<String>, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as(
            r#"
            SELECT engaged, reason, engaged_at
            FROM ai_agent_kill_switch
            WHERE org_id = $1 AND agent_type = '*'
            "#,
        )
        .bind(org_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let (engaged, reason, engaged_at) = row.unwrap_or((false, None, None));
    state.kill_switch_cache.set(org_id.as_uuid(), "*", engaged);
    Ok(KillSwitchView {
        org_wide: true,
        agent_type: "*".into(),
        engaged,
        reason,
        engaged_at,
    })
}

/// Poll until a run is no longer running/waiting, or deadline elapses.
pub async fn await_run_halted(
    state: &AppState,
    org_id: OrgId,
    run_id: Uuid,
    request_id: &str,
    bound: Duration,
) -> Result<bool, AppError> {
    let start = Instant::now();
    while start.elapsed() < bound {
        let mut tx = state
            .pool
            .begin()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        set_session_org_id(&mut tx, org_id)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM ai_agent_run WHERE id = $1 AND org_id = $2")
                .bind(run_id)
                .bind(org_id.as_uuid())
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        if let Some(s) = status {
            if s != "running" && s != "waiting" {
                return Ok(true);
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(false)
}
