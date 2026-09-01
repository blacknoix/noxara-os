//! Network allowlist + infrastructure tier (fail-closed in gateway via internal gate).

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};

use crate::auth::extract::AuthUser;
use crate::governance::{authorize, internal};
use crate::state::AppState;
use crate::workspace;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/governance/network",
            get(get_network).put(put_network),
        )
        .route("/api/v1/internal/network-gate", post(network_gate))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyDto {
    pub infra_tier: String,
    pub allowlist_enabled: bool,
    pub cidr_allowlist: Vec<String>,
    pub mtls_client_ids: Vec<String>,
    pub version: i32,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNetworkRequest {
    pub infra_tier: Option<String>,
    pub allowlist_enabled: Option<bool>,
    pub cidr_allowlist: Option<Vec<String>>,
    pub mtls_client_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct NetworkGateRequest {
    pub org_id: String,
    pub source_ip: String,
    pub mtls_client_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NetworkGateResponse {
    pub allowed: bool,
    pub reason: String,
    pub infra_tier: String,
}

async fn get_network(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<NetworkPolicyDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_network_manage()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row: Option<(String, bool, Vec<String>, Vec<String>, i32)> = sqlx::query_as(
        r#"
        SELECT infra_tier, allowlist_enabled, cidr_allowlist, mtls_client_ids, version
        FROM org_network_policy WHERE org_id = $1
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(row.map_or_else(
        || NetworkPolicyDto {
            infra_tier: "shared".into(),
            allowlist_enabled: false,
            cidr_allowlist: vec![],
            mtls_client_ids: vec![],
            version: 0,
        },
        |(infra_tier, allowlist_enabled, cidr_allowlist, mtls_client_ids, version)| {
            NetworkPolicyDto {
                infra_tier,
                allowlist_enabled,
                cidr_allowlist,
                mtls_client_ids,
                version,
            }
        },
    )))
}

async fn put_network(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateNetworkRequest>,
) -> Result<Json<NetworkPolicyDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_network_manage()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    let infra_tier = body.infra_tier.unwrap_or_else(|| "shared".into());
    if infra_tier != "shared" && infra_tier != "dedicated" {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "infra_tier must be shared|dedicated",
        ));
    }
    let allowlist_enabled = body.allowlist_enabled.unwrap_or(false);
    let cidr_allowlist = body.cidr_allowlist.unwrap_or_default();
    let mtls_client_ids = body.mtls_client_ids.unwrap_or_default();
    for cidr in &cidr_allowlist {
        if parse_ipv4_cidr(cidr).is_none() {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                format!("invalid CIDR: {cidr}"),
            ));
        }
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let version: i32 = sqlx::query_scalar(
        r#"
        INSERT INTO org_network_policy (
            org_id, infra_tier, allowlist_enabled, cidr_allowlist, mtls_client_ids, updated_by, version
        ) VALUES ($1,$2,$3,$4,$5,$6,1)
        ON CONFLICT (org_id) DO UPDATE SET
            infra_tier = EXCLUDED.infra_tier,
            allowlist_enabled = EXCLUDED.allowlist_enabled,
            cidr_allowlist = EXCLUDED.cidr_allowlist,
            mtls_client_ids = EXCLUDED.mtls_client_ids,
            updated_by = EXCLUDED.updated_by,
            updated_at = now(),
            version = org_network_policy.version + 1
        RETURNING version
        "#,
    )
    .bind(org_id)
    .bind(&infra_tier)
    .bind(allowlist_enabled)
    .bind(&cidr_allowlist)
    .bind(&mtls_client_ids)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    // Mirror for gateway (no RLS).
    sqlx::query(
        r#"
        INSERT INTO org_network_policy_lookup (
            org_id, infra_tier, allowlist_enabled, cidr_allowlist, mtls_client_ids, version
        ) VALUES ($1,$2,$3,$4,$5,$6)
        ON CONFLICT (org_id) DO UPDATE SET
            infra_tier = EXCLUDED.infra_tier,
            allowlist_enabled = EXCLUDED.allowlist_enabled,
            cidr_allowlist = EXCLUDED.cidr_allowlist,
            mtls_client_ids = EXCLUDED.mtls_client_ids,
            version = EXCLUDED.version
        "#,
    )
    .bind(org_id)
    .bind(&infra_tier)
    .bind(allowlist_enabled)
    .bind(&cidr_allowlist)
    .bind(&mtls_client_ids)
    .bind(version)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "network.policy.update",
        "org_network_policy",
        &org_id.to_string(),
        serde_json::json!({
            "infra_tier": infra_tier,
            "allowlist_enabled": allowlist_enabled,
            "version": version,
        }),
    )
    .await;

    Ok(Json(NetworkPolicyDto {
        infra_tier,
        allowlist_enabled,
        cidr_allowlist,
        mtls_client_ids,
        version,
    }))
}

/// Internal gateway gate — fail closed when allowlist enabled.
async fn network_gate(
    State(state): State<AppState>,
    Json(body): Json<NetworkGateRequest>,
) -> Result<Json<NetworkGateResponse>, AppError> {
    let request_id = "network-gate";
    let org_pid: PublicId = body
        .org_id
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, request_id, "invalid org_id"))?;
    if org_pid.kind() != IdKind::Org {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "org_id must be org_…",
        ));
    }
    let org_uuid = org_pid.uuid();

    let row: Option<(String, bool, Vec<String>, Vec<String>)> = sqlx::query_as(
        r#"
        SELECT infra_tier, allowlist_enabled, cidr_allowlist, mtls_client_ids
        FROM org_network_policy_lookup WHERE org_id = $1
        "#,
    )
    .bind(org_uuid)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal(request_id))?;

    let Some((infra_tier, allowlist_enabled, cidrs, mtls_ids)) = row else {
        return Ok(Json(NetworkGateResponse {
            allowed: true,
            reason: "no policy".into(),
            infra_tier: "shared".into(),
        }));
    };

    if !allowlist_enabled {
        return Ok(Json(NetworkGateResponse {
            allowed: true,
            reason: "allowlist disabled".into(),
            infra_tier,
        }));
    }

    if let Some(ref client_id) = body.mtls_client_id {
        if !client_id.is_empty() && mtls_ids.iter().any(|id| id == client_id) {
            return Ok(Json(NetworkGateResponse {
                allowed: true,
                reason: "mtls client allowlisted".into(),
                infra_tier,
            }));
        }
    }

    if ip_in_any_cidr(&body.source_ip, &cidrs) {
        return Ok(Json(NetworkGateResponse {
            allowed: true,
            reason: "source ip allowlisted".into(),
            infra_tier,
        }));
    }

    Ok(Json(NetworkGateResponse {
        allowed: false,
        reason: "source not on allowlist".into(),
        infra_tier,
    }))
}

/// Parse `a.b.c.d/prefix` into (addr_u32, mask_bits).
pub fn parse_ipv4_cidr(cidr: &str) -> Option<(u32, u8)> {
    let (addr, prefix) = cidr.split_once('/')?;
    let prefix: u8 = prefix.parse().ok()?;
    if prefix > 32 {
        return None;
    }
    let parts: Vec<_> = addr.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut ip = 0u32;
    for p in parts {
        let octet: u8 = p.parse().ok()?;
        ip = (ip << 8) | u32::from(octet);
    }
    Some((ip, prefix))
}

pub fn ip_in_cidr(ip: &str, cidr: &str) -> bool {
    let Some((net, prefix)) = parse_ipv4_cidr(cidr) else {
        return false;
    };
    let Some((addr, _)) = parse_ipv4_cidr(&format!("{ip}/32")) else {
        return false;
    };
    if prefix == 0 {
        return true;
    }
    let mask = !((1u32 << (32 - prefix)) - 1);
    (addr & mask) == (net & mask)
}

pub fn ip_in_any_cidr(ip: &str, cidrs: &[String]) -> bool {
    cidrs.iter().any(|c| ip_in_cidr(ip, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_match() {
        assert!(ip_in_cidr("10.0.1.5", "10.0.0.0/16"));
        assert!(!ip_in_cidr("11.0.1.5", "10.0.0.0/16"));
        assert!(ip_in_cidr("192.168.1.1", "192.168.1.1/32"));
    }
}
