//! Workspace API types (OpenAPI schemas).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateOrgRequest {
    pub name: String,
    #[serde(default = "default_business_type")]
    pub business_type: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    /// Home region (`us` | `eu` | `ap`). Immutable after creation (ADR-015).
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_business_type() -> String {
    "general".into()
}
fn default_currency() -> String {
    "USD".into()
}
fn default_timezone() -> String {
    "UTC".into()
}
fn default_region() -> String {
    "us".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OrgResponse {
    pub org_id: String,
    pub name: String,
    pub currency: String,
    pub timezone: String,
    pub fiscal_year_start_month: i32,
    pub business_type: String,
    pub plan: String,
    pub numbering_series: serde_json::Value,
    pub branding: serde_json::Value,
    pub feature_flags: serde_json::Value,
    /// Home region (`us` | `eu` | `ap`) — ADR-015 / Phase 4.1.
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateOrgSettingsRequest {
    pub name: Option<String>,
    pub currency: Option<String>,
    pub timezone: Option<String>,
    pub fiscal_year_start_month: Option<i32>,
    pub numbering_series: Option<serde_json::Value>,
    pub branding: Option<serde_json::Value>,
    pub business_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemberView {
    pub membership_id: String,
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub role_id: Option<String>,
    pub role_name: Option<String>,
    pub status: String,
    pub policy_version: i64,
    pub team_id: Option<String>,
    pub department_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemberListResponse {
    pub items: Vec<MemberView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InviteMemberRequest {
    pub email: String,
    /// Public role id (`rol_…`) or system key (`owner`, `admin`, …).
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InviteResponse {
    pub invitation_id: String,
    pub email: String,
    pub status: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AcceptInviteRequest {
    pub token: String,
    pub display_name: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChangeRoleRequest {
    /// Public role id or system key.
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RoleView {
    pub role_id: String,
    pub name: String,
    pub description: String,
    pub system_key: Option<String>,
    pub is_system: bool,
    pub approval_limit_amount_minor: Option<i64>,
    pub approval_limit_currency: Option<String>,
    pub permissions: Vec<RolePermissionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RolePermissionView {
    pub permission_id: String,
    pub effect: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RoleListResponse {
    pub items: Vec<RoleView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertRoleRequest {
    pub name: String,
    pub description: Option<String>,
    pub approval_limit_amount_minor: Option<i64>,
    pub approval_limit_currency: Option<String>,
    pub permissions: Vec<RolePermissionInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RolePermissionInput {
    pub permission_id: String,
    pub effect: String,
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_scope() -> String {
    "organization".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CapabilityPreviewResponse {
    pub role_id: String,
    pub allowed: Vec<String>,
    pub denied_sensitive: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PermissionCatalogueResponse {
    pub items: Vec<PermissionCatalogueItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PermissionCatalogueItem {
    pub id: String,
    pub context: String,
    pub resource: String,
    pub action: String,
    pub description: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeamView {
    pub team_id: String,
    pub name: String,
    pub department_id: Option<String>,
    pub parent_team_id: Option<String>,
    pub lead_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeamListResponse {
    pub items: Vec<TeamView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTeamRequest {
    pub name: String,
    pub department_id: Option<String>,
    pub parent_team_id: Option<String>,
    pub lead_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DepartmentView {
    pub department_id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DepartmentListResponse {
    pub items: Vec<DepartmentView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDepartmentRequest {
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MyCapabilitiesResponse {
    pub org_id: String,
    pub role: String,
    pub policy_version: i64,
    pub allowed: Vec<String>,
}
