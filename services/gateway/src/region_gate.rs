//! Gateway multi-region cell gate (Phase 4.1).
//!
//! A gateway process is bound to one cell via `COMPANYOS_CELL_ID` (or
//! `COMPANYOS_CELL_REGION`). Data-plane requests whose tenant home region does
//! not match the resolved serving cell are rejected with HTTP 451.
//!
//! Control-plane paths (auth, openapi, control-plane) pass through.
//! `X-CompanyOS-Region` is honored as an edge routing hint and echoed; the
//! authoritative home region comes from the access token claim.

use axum::http::HeaderMap;
use companyos_auth_token::AccessClaims;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::{
    is_control_plane_path, is_data_plane_path, CellId, ControlPlane, RegionCode, RegionError,
    REGION_HINT_HEADER,
};

/// Cell identity for this gateway process.
#[derive(Debug, Clone)]
pub struct CellBinding {
    pub cell: CellId,
}

impl CellBinding {
    pub fn from_env() -> Self {
        let cell = std::env::var("COMPANYOS_CELL_ID")
            .ok()
            .and_then(|s| CellId::parse(&s).ok())
            .or_else(|| {
                std::env::var("COMPANYOS_CELL_REGION")
                    .ok()
                    .and_then(|s| RegionCode::parse(&s).ok())
                    .map(CellId::primary_for)
            })
            .unwrap_or(CellId::UsPrimary);
        Self { cell }
    }

    pub fn region(&self) -> RegionCode {
        self.cell.region()
    }
}

/// Decide whether this cell may proxy a data-plane request for the claims org.
pub fn gate_data_plane(
    binding: &CellBinding,
    plane: &ControlPlane,
    claims: &AccessClaims,
    path: &str,
    request_id: &str,
) -> Result<(), AppError> {
    if is_control_plane_path(path) || !is_data_plane_path(path) {
        return Ok(());
    }

    // Ensure directory knows this org (token region is authoritative for home).
    let home = RegionCode::parse(&claims.region).map_err(|e| {
        AppError::new(
            ErrorCode::ResidencyViolation,
            request_id,
            format!("invalid region claim: {e}"),
        )
    })?;

    // Prefer live control-plane resolution (includes failover); fall back to
    // direct home↔cell match when the org is not yet registered in the plane.
    let decision = match plane.org_region(&claims.org_id) {
        Some(_) => plane.enforce_data_plane_access(&claims.org_id, binding.cell),
        None => {
            // Transient: org not hydrated into in-memory plane — enforce home==cell region.
            companyos_tenancy::enforce_cell_serves_tenant(home, binding.region()).map(|_| {
                companyos_tenancy::RoutingDecision {
                    org_id: claims.org_id.clone(),
                    home_region: home,
                    serving_cell: binding.cell,
                    failover: false,
                    reason: "direct region match (directory miss)".into(),
                }
            })
        }
    };

    decision
        .map(|_| ())
        .map_err(|e| map_region_error(e, request_id))
}

fn map_region_error(e: RegionError, request_id: &str) -> AppError {
    let code = match e {
        RegionError::FailoverDenied | RegionError::CellUnavailable(_) => {
            ErrorCode::ServiceUnavailable
        }
        RegionError::RegionMismatch { .. }
        | RegionError::ObjectKeyRegionDenied
        | RegionError::ReplicaForbidden(_, _)
        | RegionError::MissingRegion => ErrorCode::ResidencyViolation,
        _ => ErrorCode::ResidencyViolation,
    };
    AppError::new(code, request_id, e.to_string())
}

/// Read optional edge region hint (`X-CompanyOS-Region`).
pub fn edge_region_hint(headers: &HeaderMap) -> Option<RegionCode> {
    headers
        .get(REGION_HINT_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| RegionCode::parse(s).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_tenancy::{CellHealth, OrgId};
    use uuid::Uuid;

    fn claims(org: &str, region: &str) -> AccessClaims {
        AccessClaims {
            sub: "usr_x".into(),
            user_id: Uuid::nil(),
            org_id: org.into(),
            org_uuid: Uuid::nil(),
            membership_id: Uuid::nil(),
            roles: vec!["owner".into()],
            policy_version: 1,
            sid: Uuid::nil(),
            family_id: Uuid::nil(),
            jti: Uuid::nil(),
            iss: "companyos".into(),
            iat: 0,
            exp: 0,
            api_key_id: None,
            scopes: None,
            region: region.into(),
        }
    }

    #[test]
    fn eu_org_denied_on_us_cell() {
        let binding = CellBinding {
            cell: CellId::UsPrimary,
        };
        let mut plane = ControlPlane::new();
        let org = OrgId::generate().to_public().as_str();
        plane.register_org(&org, RegionCode::Eu);
        let err = gate_data_plane(
            &binding,
            &plane,
            &claims(&org, "eu"),
            "/api/v1/files/x",
            "r1",
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ResidencyViolation);
    }

    #[test]
    fn eu_org_allowed_on_eu_cell() {
        let binding = CellBinding {
            cell: CellId::EuPrimary,
        };
        let mut plane = ControlPlane::new();
        let org = OrgId::generate().to_public().as_str();
        plane.register_org(&org, RegionCode::Eu);
        assert!(gate_data_plane(
            &binding,
            &plane,
            &claims(&org, "eu"),
            "/api/v1/files/x",
            "r1",
        )
        .is_ok());
    }

    #[test]
    fn auth_path_bypasses_gate() {
        let binding = CellBinding {
            cell: CellId::UsPrimary,
        };
        let plane = ControlPlane::new();
        assert!(gate_data_plane(
            &binding,
            &plane,
            &claims("org_x", "eu"),
            "/api/v1/auth/login",
            "r1",
        )
        .is_ok());
    }

    #[test]
    fn us_failover_allows_us_dr_cell() {
        let binding = CellBinding { cell: CellId::UsDr };
        let mut plane = ControlPlane::new();
        let org = OrgId::generate().to_public().as_str();
        plane.register_org(&org, RegionCode::Us);
        plane.set_cell_health(CellId::UsPrimary, CellHealth::Unhealthy);
        assert!(gate_data_plane(
            &binding,
            &plane,
            &claims(&org, "us"),
            "/api/v1/finance/invoices",
            "r1",
        )
        .is_ok());
    }

    #[test]
    fn edge_hint_parses() {
        let mut headers = HeaderMap::new();
        headers.insert(REGION_HINT_HEADER, "eu".parse().unwrap());
        assert_eq!(edge_region_hint(&headers), Some(RegionCode::Eu));
    }
}
