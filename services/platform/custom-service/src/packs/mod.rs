//! Industry pack catalogue (Phase 4.5).
//!
//! Packs are **configuration**: a `companyos.custom.package` plus seed defaults
//! and a marketplace listing reference. Domain services (CRM/finance/HR/inventory)
//! must not branch on pack id — differences are data only.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::CustomPackage;

/// Marketplace listing metadata shipped with an industry pack.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PackMarketplace {
    pub connector_key: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub requested_scopes: Vec<String>,
}

/// Seed overlays applied on install (pipeline stages + expense categories).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PackSeed {
    pub pipeline_stages: Vec<String>,
    pub expense_categories: Vec<String>,
}

/// Full industry pack manifest.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IndustryPack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub marketplace: PackMarketplace,
    pub seed: PackSeed,
    pub package: CustomPackage,
}

/// Catalogue row returned to API clients (package body omitted for list).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IndustryPackSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub marketplace_connector_key: String,
    pub entity_slugs: Vec<String>,
    pub installed: bool,
}

const PACK_FILES: &[(&str, &str)] = &[
    (
        "professional-services",
        include_str!("../../packs/professional-services.json"),
    ),
    ("retail", include_str!("../../packs/retail.json")),
    (
        "light-manufacturing",
        include_str!("../../packs/light-manufacturing.json"),
    ),
    (
        "healthcare-admin",
        include_str!("../../packs/healthcare-admin.json"),
    ),
];

/// All shipped industry packs (validated at load).
pub fn catalogue() -> Vec<IndustryPack> {
    PACK_FILES
        .iter()
        .map(|(id, raw)| {
            let pack: IndustryPack = serde_json::from_str(raw)
                .unwrap_or_else(|e| panic!("invalid industry pack {id}: {e}"));
            assert_eq!(pack.id, *id, "pack file id mismatch");
            assert_eq!(
                pack.package.format, "companyos.custom.package",
                "pack {id} must use companyos.custom.package"
            );
            assert_eq!(
                pack.package.format_version, 1,
                "pack {id} format_version must be 1"
            );
            pack
        })
        .collect()
}

pub fn get(pack_id: &str) -> Option<IndustryPack> {
    catalogue().into_iter().find(|p| p.id == pack_id)
}

pub fn summary(pack: &IndustryPack, installed: bool) -> IndustryPackSummary {
    IndustryPackSummary {
        id: pack.id.clone(),
        name: pack.name.clone(),
        description: pack.description.clone(),
        version: pack.version.clone(),
        marketplace_connector_key: pack.marketplace.connector_key.clone(),
        entity_slugs: pack
            .package
            .entities
            .iter()
            .map(|e| e.slug.clone())
            .collect(),
        installed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_packs_load_as_custom_packages() {
        let packs = catalogue();
        assert_eq!(packs.len(), 4);
        let ids: Vec<_> = packs.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"professional-services"));
        assert!(ids.contains(&"retail"));
        assert!(ids.contains(&"light-manufacturing"));
        assert!(ids.contains(&"healthcare-admin"));
        for p in &packs {
            assert!(!p.package.entities.is_empty());
            assert_eq!(p.package.name, format!("industry.{}", p.id));
        }
    }

    #[test]
    fn healthcare_pack_does_not_claim_ehr_or_phi_bypass() {
        let pack = get("healthcare-admin").expect("healthcare pack");
        let blob = serde_json::to_string(&pack).unwrap().to_lowercase();
        assert!(
            blob.contains("not an ehr") || pack.description.to_lowercase().contains("not an ehr")
        );
        assert!(!blob.contains("bypass authz"));
        assert!(!blob.contains("bypass rls"));
    }
}
