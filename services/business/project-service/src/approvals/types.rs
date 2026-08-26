//! DTOs and policy definition shapes for the approval engine.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Approval modes for multi-step / multi-approver policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Sequential,
    Any,
    All,
}

impl ApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Any => "any",
            Self::All => "all",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sequential" => Some(Self::Sequential),
            "any" => Some(Self::Any),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// Match criteria evaluated at request time (snapshotted onto the approval).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct PolicyMatch {
    #[serde(default)]
    pub amount_minor_gte: Option<i64>,
    #[serde(default)]
    pub amount_minor_lt: Option<i64>,
    /// Discount basis points (quote_discount): discount/subtotal * 10000.
    #[serde(default)]
    pub discount_bps_gte: Option<i64>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub department_ids: Vec<String>,
    #[serde(default)]
    pub requester_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PolicyStepDef {
    pub order: i32,
    #[serde(default)]
    pub approver_role: Option<String>,
    #[serde(default)]
    pub approver_user_ids: Vec<String>,
    #[serde(default)]
    pub sla_seconds: Option<i32>,
    #[serde(default)]
    pub escalate_to_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PolicyDefinition {
    pub mode: ApprovalMode,
    #[serde(default)]
    pub match_criteria: PolicyMatch,
    pub steps: Vec<PolicyStepDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResolvedStepSnapshot {
    pub order: i32,
    pub approver_role: Option<String>,
    pub assignee_user_ids: Vec<Uuid>,
    pub sla_seconds: Option<i32>,
    pub escalate_to_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RoutingSnapshot {
    pub policy_public_id: String,
    pub policy_name: String,
    pub policy_version: i32,
    pub mode: ApprovalMode,
    pub match_criteria: PolicyMatch,
    pub steps: Vec<ResolvedStepSnapshot>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApprovalStepDto {
    pub order: i32,
    pub status: String,
    pub approver_role: Option<String>,
    pub assignee_user_ids: Vec<String>,
    pub sla_seconds: Option<i32>,
    pub escalate_to_role: Option<String>,
    pub escalated_at: Option<String>,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApprovalDto {
    pub id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub status: String,
    pub requester_user_id: String,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub category: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub mode: String,
    pub current_step: i32,
    pub policy_id: String,
    pub policy_version: i32,
    pub routing_snapshot: RoutingSnapshot,
    pub steps: Vec<ApprovalStepDto>,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
    pub decision_note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApprovalListResponse {
    pub items: Vec<ApprovalDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateApprovalRequest {
    pub subject_type: String,
    pub subject_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub amount_minor: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub department_id: Option<String>,
    #[serde(default)]
    pub requester_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DecideApprovalRequest {
    pub approve: bool,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BulkDecideRequest {
    pub ids: Vec<String>,
    pub approve: bool,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BulkDecideResponse {
    pub decided: Vec<ApprovalDto>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDelegationRequest {
    pub to_user_id: String,
    #[serde(default)]
    pub approval_id: Option<String>,
    #[serde(default)]
    pub ends_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DelegationDto {
    pub id: String,
    pub from_user_id: String,
    pub to_user_id: String,
    pub approval_id: Option<String>,
    pub starts_at: String,
    pub ends_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApprovalPolicyDto {
    pub id: String,
    pub name: String,
    pub subject_type: String,
    pub is_active: bool,
    pub current_version: i32,
    pub definition: PolicyDefinition,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub subject_type: String,
    pub definition: PolicyDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdatePolicyRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_active: Option<bool>,
    /// Publishing a new definition increments version (immutable history).
    #[serde(default)]
    pub definition: Option<PolicyDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PolicyListResponse {
    pub items: Vec<ApprovalPolicyDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InboxSummaryDto {
    pub pending_for_me: i64,
}

/// Inputs used by the Temporal ApprovalProcess workflow / activities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalProcessInput {
    pub org_id: String,
    pub approval_id: String,
    pub approval_public_id: String,
    pub sla_seconds: i32,
    pub current_step: i32,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecideSignal {
    pub approve: bool,
    pub actor_user_id: String,
    pub on_behalf_of: String,
    pub comment: Option<String>,
    pub idempotency_key: Option<String>,
}
