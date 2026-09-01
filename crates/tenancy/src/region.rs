//! Multi-region cells, residency policy, and tenant→cell routing (Phase 4.1).
//!
//! Region is a **tenant attribute** (ADR-015): set at org creation, immutable by
//! default. Residency is a tenancy/routing invariant — not an authz permission.
//! The sole PDP remains `crates/authz`.
//!
//! Architecture:
//! - **Control plane** (identity, org directory, region map, cell health): may be
//!   global; tokens stay org-scoped.
//! - **Data plane** (CRM, finance, files, search, analytics, workflows): cell-local.
//!   Cross-region data-plane access is deny-by-default.
//!
//! Replication of data-plane payloads across regions is **deny by default**. Only
//! explicitly allowlisted metadata (routing table, login/identity) may be global.
//! Failover is permitted only to an in-region standby when the residency policy
//! lists `DisasterRecovery` among allowed replica kinds.

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{OrgId, TenancyError};

/// Canonical region codes for CompanyOS cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RegionCode {
    #[default]
    Us,
    Eu,
    Ap,
}

impl RegionCode {
    pub const ALL: [RegionCode; 3] = [RegionCode::Us, RegionCode::Eu, RegionCode::Ap];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Us => "us",
            Self::Eu => "eu",
            Self::Ap => "ap",
        }
    }

    pub fn parse(s: &str) -> Result<Self, RegionError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "us" => Ok(Self::Us),
            "eu" => Ok(Self::Eu),
            "ap" => Ok(Self::Ap),
            other => Err(RegionError::UnknownRegion(other.to_string())),
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Us => "United States",
            Self::Eu => "European Union",
            Self::Ap => "Asia Pacific",
        }
    }
}

impl fmt::Display for RegionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What may leave the home cell (deny-by-default for data-plane payloads).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaKind {
    /// Org→region routing rows and cell health (global control plane).
    RoutingMetadata,
    /// Login / identity / session minting (global control plane).
    Identity,
    /// In-region disaster-recovery standby cell only.
    DisasterRecovery,
}

/// Contractual residency policy for one region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegionPolicy {
    pub region: RegionCode,
    /// Human-readable jurisdiction / contractual note.
    pub jurisdiction: &'static str,
    /// Replica kinds explicitly permitted. Anything else is forbidden.
    #[serde(skip)]
    pub allowed_replicas: &'static [ReplicaKind],
    /// When true, customer data-plane payloads must remain in-region forever
    /// (no cross-region DR, no cross-region analytics replicas).
    pub home_cell_only_data_plane: bool,
}

/// Built-in catalogue — contractual source of truth mirrored in
/// `docs/compliance/data-residency.md`.
pub fn region_catalogue() -> &'static [RegionPolicy] {
    &CATALOGUE
}

const CATALOGUE: [RegionPolicy; 3] = [
    RegionPolicy {
        region: RegionCode::Us,
        jurisdiction: "United States (customer data may stay in US cells; US-DR standby allowed)",
        allowed_replicas: &[
            ReplicaKind::RoutingMetadata,
            ReplicaKind::Identity,
            ReplicaKind::DisasterRecovery,
        ],
        home_cell_only_data_plane: false,
    },
    RegionPolicy {
        region: RegionCode::Eu,
        jurisdiction: "EU/EEA — GDPR; data-plane must not leave EU cells (no US failover)",
        allowed_replicas: &[ReplicaKind::RoutingMetadata, ReplicaKind::Identity],
        home_cell_only_data_plane: true,
    },
    RegionPolicy {
        region: RegionCode::Ap,
        jurisdiction: "Asia Pacific — home-cell data-plane; routing/identity metadata global",
        allowed_replicas: &[ReplicaKind::RoutingMetadata, ReplicaKind::Identity],
        home_cell_only_data_plane: true,
    },
];

pub fn policy_for(region: RegionCode) -> &'static RegionPolicy {
    region_catalogue()
        .iter()
        .find(|p| p.region == region)
        .expect("catalogue covers all RegionCode variants")
}

pub fn allows_replica(region: RegionCode, kind: ReplicaKind) -> bool {
    policy_for(region).allowed_replicas.contains(&kind)
}

/// Logical cell within a region (primary or in-region standby).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellId {
    UsPrimary,
    UsDr,
    EuPrimary,
    ApPrimary,
}

impl CellId {
    pub const ALL: [CellId; 4] = [
        CellId::UsPrimary,
        CellId::UsDr,
        CellId::EuPrimary,
        CellId::ApPrimary,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UsPrimary => "us-primary",
            Self::UsDr => "us-dr",
            Self::EuPrimary => "eu-primary",
            Self::ApPrimary => "ap-primary",
        }
    }

    pub fn parse(s: &str) -> Result<Self, RegionError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "us-primary" | "us" => Ok(Self::UsPrimary),
            "us-dr" => Ok(Self::UsDr),
            "eu-primary" | "eu" => Ok(Self::EuPrimary),
            "ap-primary" | "ap" => Ok(Self::ApPrimary),
            other => Err(RegionError::UnknownCell(other.to_string())),
        }
    }

    pub fn region(self) -> RegionCode {
        match self {
            Self::UsPrimary | Self::UsDr => RegionCode::Us,
            Self::EuPrimary => RegionCode::Eu,
            Self::ApPrimary => RegionCode::Ap,
        }
    }

    pub fn is_standby(self) -> bool {
        matches!(self, Self::UsDr)
    }

    /// Home primary cell for a tenant region.
    pub fn primary_for(region: RegionCode) -> Self {
        match region {
            RegionCode::Us => Self::UsPrimary,
            RegionCode::Eu => Self::EuPrimary,
            RegionCode::Ap => Self::ApPrimary,
        }
    }

    /// Documented in-region standby, if the catalogue permits DR.
    pub fn standby_for(region: RegionCode) -> Option<Self> {
        match region {
            RegionCode::Us if allows_replica(region, ReplicaKind::DisasterRecovery) => {
                Some(Self::UsDr)
            }
            _ => None,
        }
    }
}

impl fmt::Display for CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellHealth {
    Healthy,
    Unhealthy,
    Unknown,
}

impl CellHealth {
    pub fn is_serving(self) -> bool {
        matches!(self, Self::Healthy)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RegionError {
    #[error("unknown region '{0}'")]
    UnknownRegion(String),
    #[error("unknown cell '{0}'")]
    UnknownCell(String),
    #[error("organization home region is immutable after creation")]
    RegionImmutable,
    #[error("missing region on tenant-owned request")]
    MissingRegion,
    #[error("region mismatch: tenant home={home}, cell={cell}")]
    RegionMismatch { home: String, cell: String },
    #[error("object key missing or mismatched region prefix")]
    ObjectKeyRegionDenied,
    #[error("no in-region standby available for failover (fail closed)")]
    FailoverDenied,
    #[error("residency policy forbids replica kind {0:?} in region {1}")]
    ReplicaForbidden(ReplicaKind, String),
    #[error("cell {0} is unhealthy and no permitted standby is healthy")]
    CellUnavailable(String),
    #[error("org not found in routing directory")]
    OrgNotInDirectory,
}

impl From<RegionError> for TenancyError {
    fn from(value: RegionError) -> Self {
        TenancyError::Residency(value.to_string())
    }
}

/// Object-storage key layout: `{region}/org/{org_uuid}/{file_id}/{filename}`.
///
/// Region is part of the key so a wrong-cell client cannot fetch another
/// region's objects even if it guesses the org/file UUID path.
pub fn object_key(
    region: RegionCode,
    org_id: OrgId,
    file_id: impl fmt::Display,
    filename: &str,
) -> String {
    format!(
        "{}/org/{}/{}/{}",
        region.as_str(),
        org_id.as_uuid(),
        file_id,
        filename
    )
}

/// Parse the leading region segment from an object key.
pub fn region_from_object_key(key: &str) -> Result<RegionCode, RegionError> {
    let mut parts = key.split('/');
    let region = parts.next().ok_or(RegionError::ObjectKeyRegionDenied)?;
    let org_marker = parts.next().ok_or(RegionError::ObjectKeyRegionDenied)?;
    if org_marker != "org" {
        return Err(RegionError::ObjectKeyRegionDenied);
    }
    RegionCode::parse(region)
}

/// Reject when the object key's region does not match the serving cell.
pub fn enforce_object_key_region(
    key: &str,
    cell_region: RegionCode,
) -> Result<RegionCode, RegionError> {
    let key_region = region_from_object_key(key)?;
    if key_region != cell_region {
        return Err(RegionError::RegionMismatch {
            home: key_region.as_str().into(),
            cell: cell_region.as_str().into(),
        });
    }
    Ok(key_region)
}

/// Reject missing region **or** mismatched tenant home vs serving cell.
pub fn enforce_cell_serves_tenant(
    tenant_home: RegionCode,
    cell_region: RegionCode,
) -> Result<(), RegionError> {
    if tenant_home != cell_region {
        return Err(RegionError::RegionMismatch {
            home: tenant_home.as_str().into(),
            cell: cell_region.as_str().into(),
        });
    }
    Ok(())
}

/// Analytics / search query guard: region required and must match tenant home.
pub fn enforce_query_region(
    query_region: Option<&str>,
    tenant_home: RegionCode,
) -> Result<RegionCode, RegionError> {
    let raw = query_region
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(RegionError::MissingRegion)?;
    let parsed = RegionCode::parse(raw)?;
    if parsed != tenant_home {
        return Err(RegionError::RegionMismatch {
            home: tenant_home.as_str().into(),
            cell: parsed.as_str().into(),
        });
    }
    Ok(parsed)
}

/// Org home region cannot change after creation (default Phase 4.1 posture).
pub fn enforce_region_immutable(
    existing: RegionCode,
    requested: Option<RegionCode>,
) -> Result<(), RegionError> {
    if let Some(r) = requested {
        if r != existing {
            return Err(RegionError::RegionImmutable);
        }
    }
    Ok(())
}

/// Header name honored for latency-based edge routing hints (Cloudflare/geo).
pub const REGION_HINT_HEADER: &str = "x-companyos-region";

/// Resolved serving cell for a tenant after health + residency checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub org_id: String,
    pub home_region: RegionCode,
    pub serving_cell: CellId,
    pub failover: bool,
    pub reason: String,
}

/// In-memory control-plane slice used by CI / compose cell simulation.
///
/// Production would back this with a global org directory + cell health service;
/// here we model the same contracts so gateway tests and the failover drill can
/// run without real multi-region infra.
#[derive(Debug, Clone, Default)]
pub struct ControlPlane {
    /// org public id → home region
    org_regions: HashMap<String, RegionCode>,
    cell_health: HashMap<CellId, CellHealth>,
}

impl ControlPlane {
    pub fn new() -> Self {
        let mut cell_health = HashMap::new();
        for cell in CellId::ALL {
            cell_health.insert(cell, CellHealth::Healthy);
        }
        Self {
            org_regions: HashMap::new(),
            cell_health,
        }
    }

    pub fn register_org(&mut self, org_public_id: impl Into<String>, region: RegionCode) {
        self.org_regions.insert(org_public_id.into(), region);
    }

    pub fn org_region(&self, org_public_id: &str) -> Option<RegionCode> {
        self.org_regions.get(org_public_id).copied()
    }

    pub fn set_cell_health(&mut self, cell: CellId, health: CellHealth) {
        self.cell_health.insert(cell, health);
    }

    pub fn cell_health(&self, cell: CellId) -> CellHealth {
        self.cell_health
            .get(&cell)
            .copied()
            .unwrap_or(CellHealth::Unknown)
    }

    /// Resolve which cell should serve this org's data-plane traffic.
    pub fn resolve_serving_cell(
        &self,
        org_public_id: &str,
    ) -> Result<RoutingDecision, RegionError> {
        let home = self
            .org_region(org_public_id)
            .ok_or(RegionError::OrgNotInDirectory)?;
        let primary = CellId::primary_for(home);
        if self.cell_health(primary).is_serving() {
            return Ok(RoutingDecision {
                org_id: org_public_id.to_string(),
                home_region: home,
                serving_cell: primary,
                failover: false,
                reason: "primary healthy".into(),
            });
        }

        // Failover only when residency policy allows in-region DR.
        if !allows_replica(home, ReplicaKind::DisasterRecovery) {
            return Err(RegionError::FailoverDenied);
        }
        let Some(standby) = CellId::standby_for(home) else {
            return Err(RegionError::FailoverDenied);
        };
        if !self.cell_health(standby).is_serving() {
            return Err(RegionError::CellUnavailable(primary.as_str().into()));
        }
        Ok(RoutingDecision {
            org_id: org_public_id.to_string(),
            home_region: home,
            serving_cell: standby,
            failover: true,
            reason: format!(
                "primary {} unhealthy; cut over to in-region standby {}",
                primary.as_str(),
                standby.as_str()
            ),
        })
    }

    /// Data-plane gate: this cell may serve the org only if it is the resolved
    /// serving cell (primary or permitted failover).
    pub fn enforce_data_plane_access(
        &self,
        org_public_id: &str,
        this_cell: CellId,
    ) -> Result<RoutingDecision, RegionError> {
        let decision = self.resolve_serving_cell(org_public_id)?;
        if decision.serving_cell != this_cell
            && decision.serving_cell.region() != this_cell.region()
        {
            return Err(RegionError::RegionMismatch {
                home: decision.home_region.as_str().into(),
                cell: this_cell.region().as_str().into(),
            });
        }
        if decision.serving_cell != this_cell {
            // Same region but wrong cell identity (e.g. primary while DR is active,
            // or DR while primary is healthy) — still deny to keep sticky cutover.
            return Err(RegionError::RegionMismatch {
                home: decision.serving_cell.as_str().into(),
                cell: this_cell.as_str().into(),
            });
        }
        // Belt-and-suspenders residency check.
        enforce_cell_serves_tenant(decision.home_region, this_cell.region())?;
        Ok(decision)
    }
}

/// Published production RTO for full-region restore (TRD).
pub const PRODUCTION_REGION_RTO: Duration = Duration::from_secs(60 * 60);

/// CI / compose failover drill budget (seconds are fine; runbook maps to 60m).
pub const CI_FAILOVER_DRILL_BUDGET: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailoverDrillReport {
    pub scenario: String,
    pub success: bool,
    pub elapsed: Duration,
    pub within_budget: bool,
    pub budget: Duration,
    pub production_rto: Duration,
    pub decision: Option<RoutingDecision>,
    pub error: Option<String>,
}

/// Simulated cell failure → routing cutover → health assertion.
///
/// Completes in-process (CI). The runbook documents how the same steps map to
/// the ≤60 minute production RTO.
pub fn run_failover_drill(
    plane: &mut ControlPlane,
    org_public_id: &str,
    fail_cell: CellId,
    budget: Duration,
) -> FailoverDrillReport {
    let started = Instant::now();
    let scenario = format!(
        "mark {} unhealthy → resolve serving cell for {org_public_id}",
        fail_cell.as_str()
    );
    plane.set_cell_health(fail_cell, CellHealth::Unhealthy);
    let result = plane.resolve_serving_cell(org_public_id);
    let elapsed = started.elapsed();
    match result {
        Ok(decision) => FailoverDrillReport {
            scenario,
            success: true,
            elapsed,
            within_budget: elapsed <= budget,
            budget,
            production_rto: PRODUCTION_REGION_RTO,
            decision: Some(decision),
            error: None,
        },
        Err(e) => FailoverDrillReport {
            scenario,
            success: false,
            elapsed,
            within_budget: elapsed <= budget,
            budget,
            production_rto: PRODUCTION_REGION_RTO,
            decision: None,
            error: Some(e.to_string()),
        },
    }
}

/// Paths that are control-plane (may be served globally / any healthy cell).
pub fn is_control_plane_path(path: &str) -> bool {
    path.starts_with("/api/v1/auth/")
        || path.starts_with("/api/v1/openapi")
        || path.starts_with("/api/v1/internal/")
        || path.starts_with("/api/v1/control-plane/")
        || path == "/livez"
        || path == "/readyz"
        || path == "/healthz"
        || path == "/api/v1/gateway/info"
}

/// Data-plane prefixes that must be cell-local.
pub fn is_data_plane_path(path: &str) -> bool {
    path.starts_with("/api/v1/sales/")
        || path.starts_with("/api/v1/finance/")
        || path.starts_with("/api/v1/operations/")
        || path.starts_with("/api/v1/people/")
        || path.starts_with("/api/v1/inventory/")
        || path.starts_with("/api/v1/files/")
        || path.starts_with("/api/v1/search/")
        || path.starts_with("/api/v1/analytics/")
        || path.starts_with("/api/v1/ai/")
        || path.starts_with("/api/v1/workflows/")
        || path.starts_with("/api/v1/notifications/")
        || path.starts_with("/api/v1/marketplace/")
        || path.starts_with("/api/v1/integrations")
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn catalogue_covers_all_regions() {
        for r in RegionCode::ALL {
            let p = policy_for(r);
            assert_eq!(p.region, r);
            assert!(p.allowed_replicas.contains(&ReplicaKind::RoutingMetadata));
            assert!(p.allowed_replicas.contains(&ReplicaKind::Identity));
        }
    }

    #[test]
    fn eu_forbids_dr_us_allows() {
        assert!(!allows_replica(
            RegionCode::Eu,
            ReplicaKind::DisasterRecovery
        ));
        assert!(allows_replica(
            RegionCode::Us,
            ReplicaKind::DisasterRecovery
        ));
        assert!(CellId::standby_for(RegionCode::Eu).is_none());
        assert_eq!(CellId::standby_for(RegionCode::Us), Some(CellId::UsDr));
    }

    #[test]
    fn object_key_includes_region_and_org() {
        let org = OrgId::new(Uuid::nil());
        let key = object_key(RegionCode::Eu, org, "file-1", "a.pdf");
        assert_eq!(
            key,
            "eu/org/00000000-0000-0000-0000-000000000000/file-1/a.pdf"
        );
        assert_eq!(region_from_object_key(&key).unwrap(), RegionCode::Eu);
        assert!(enforce_object_key_region(&key, RegionCode::Eu).is_ok());
        assert!(enforce_object_key_region(&key, RegionCode::Us).is_err());
    }

    #[test]
    fn query_guard_rejects_missing_and_mismatch() {
        assert_eq!(
            enforce_query_region(None, RegionCode::Eu),
            Err(RegionError::MissingRegion)
        );
        assert!(enforce_query_region(Some("eu"), RegionCode::Eu).is_ok());
        assert!(enforce_query_region(Some("us"), RegionCode::Eu).is_err());
    }

    #[test]
    fn region_immutable_after_create() {
        assert!(enforce_region_immutable(RegionCode::Eu, None).is_ok());
        assert!(enforce_region_immutable(RegionCode::Eu, Some(RegionCode::Eu)).is_ok());
        assert_eq!(
            enforce_region_immutable(RegionCode::Eu, Some(RegionCode::Us)),
            Err(RegionError::RegionImmutable)
        );
    }

    #[test]
    fn us_failover_to_us_dr_succeeds_within_budget() {
        let mut plane = ControlPlane::new();
        plane.register_org("org_us", RegionCode::Us);
        let report = run_failover_drill(
            &mut plane,
            "org_us",
            CellId::UsPrimary,
            CI_FAILOVER_DRILL_BUDGET,
        );
        assert!(report.success, "{report:?}");
        assert!(report.within_budget);
        let d = report.decision.unwrap();
        assert!(d.failover);
        assert_eq!(d.serving_cell, CellId::UsDr);
        assert!(report.elapsed < PRODUCTION_REGION_RTO);
    }

    #[test]
    fn eu_failover_to_us_rejected() {
        let mut plane = ControlPlane::new();
        plane.register_org("org_eu", RegionCode::Eu);
        // Even if an operator mistakenly marks a US cell as candidate, resolve
        // must fail closed — no standby for EU.
        let report = run_failover_drill(
            &mut plane,
            "org_eu",
            CellId::EuPrimary,
            CI_FAILOVER_DRILL_BUDGET,
        );
        assert!(!report.success);
        assert_eq!(
            report.error.as_deref(),
            Some("no in-region standby available for failover (fail closed)")
        );
    }

    #[test]
    fn cross_region_data_plane_denied() {
        let mut plane = ControlPlane::new();
        plane.register_org("org_eu", RegionCode::Eu);
        let err = plane
            .enforce_data_plane_access("org_eu", CellId::UsPrimary)
            .unwrap_err();
        assert!(matches!(err, RegionError::RegionMismatch { .. }));
    }

    #[test]
    fn same_region_primary_serves_when_healthy() {
        let mut plane = ControlPlane::new();
        plane.register_org("org_eu", RegionCode::Eu);
        let d = plane
            .enforce_data_plane_access("org_eu", CellId::EuPrimary)
            .unwrap();
        assert!(!d.failover);
        assert_eq!(d.serving_cell, CellId::EuPrimary);
    }

    #[test]
    fn control_vs_data_plane_paths() {
        assert!(is_control_plane_path("/api/v1/auth/login"));
        assert!(is_control_plane_path("/api/v1/control-plane/regions"));
        assert!(is_data_plane_path("/api/v1/files/presign-upload"));
        assert!(is_data_plane_path("/api/v1/analytics/query"));
        assert!(!is_data_plane_path("/api/v1/auth/login"));
    }
}
