//! Domain types for the Phase 4.4 low-code builder.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Number,
    Money,
    Date,
    Bool,
    Select,
    /// Reference to a first-party or custom entity (public id string).
    Ref,
    Formula,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FieldDef {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub required: bool,
    /// Select options when `field_type == Select`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// Target entity kind/slug for `Ref` (e.g. `customer` or custom slug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_target: Option<String>,
    /// Formula expression when `field_type == Formula`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EntityDefinitionDto {
    pub id: String,
    pub slug: String,
    pub label: String,
    pub description: String,
    pub fields: Vec<FieldDef>,
    pub status: String,
    pub published_version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateEntityRequest {
    pub slug: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateEntityRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: Option<Vec<FieldDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomRecordDto {
    pub id: String,
    pub entity_slug: String,
    pub values: serde_json::Value,
    /// Optimistic concurrency version (If-Match on PATCH).
    #[serde(default = "default_version")]
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_version() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertRecordRequest {
    pub values: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomViewDto {
    pub id: String,
    pub entity_slug: String,
    pub name: String,
    pub columns: serde_json::Value,
    pub filters: serde_json::Value,
    pub sort: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertViewRequest {
    pub name: String,
    #[serde(default)]
    pub columns: serde_json::Value,
    #[serde(default)]
    pub filters: serde_json::Value,
    #[serde(default)]
    pub sort: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomLayoutDto {
    pub id: String,
    pub entity_slug: String,
    pub name: String,
    pub sections: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertLayoutRequest {
    pub name: String,
    #[serde(default)]
    pub sections: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomScriptDto {
    pub id: String,
    pub entity_slug: String,
    pub hook: String,
    pub source: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertScriptRequest {
    pub hook: String,
    pub source: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Versioned customisation package (entities, views, layouts, scripts, permission names).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomPackage {
    /// Always `companyos.custom.package`.
    pub format: String,
    /// Package format version (currently 1).
    pub format_version: u32,
    pub name: String,
    pub version: String,
    pub entities: Vec<PackageEntity>,
    #[serde(default)]
    pub views: Vec<PackageView>,
    #[serde(default)]
    pub layouts: Vec<PackageLayout>,
    #[serde(default)]
    pub scripts: Vec<PackageScript>,
    /// Permission names that must exist after import (e.g. `custom.widget.read`).
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PackageEntity {
    pub slug: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PackageView {
    pub entity_slug: String,
    pub name: String,
    pub columns: serde_json::Value,
    pub filters: serde_json::Value,
    pub sort: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PackageLayout {
    pub entity_slug: String,
    pub name: String,
    pub sections: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PackageScript {
    pub entity_slug: String,
    pub hook: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportPackageRequest {
    pub package: CustomPackage,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportPackageResponse {
    pub package_id: String,
    pub entities_imported: usize,
    pub permissions: Vec<String>,
}

pub const PACKAGE_FORMAT: &str = "companyos.custom.package";
pub const PACKAGE_FORMAT_VERSION: u32 = 1;
