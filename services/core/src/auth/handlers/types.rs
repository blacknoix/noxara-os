//! Auth API request/response types (OpenAPI schemas).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub org_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterResponse {
    pub user_id: String,
    pub org_id: String,
    pub email: String,
    pub verification_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub org_id: Option<String>,
    pub device_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MfaChallengeResponse {
    pub mfa_required: bool,
    pub challenge_token: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VerifyEmailRequest {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResendVerificationRequest {
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MagicLinkRequest {
    pub email: String,
    pub org_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MagicLinkConsumeRequest {
    pub token: String,
    pub device_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PasswordResetRequest {
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PasswordResetConfirm {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MfaSetupRequest {
    /// When Owner/Admin must enroll before an access token exists.
    pub challenge_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MfaSetupResponse {
    pub secret: String,
    pub otpauth_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MfaConfirmRequest {
    pub code: String,
    pub challenge_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MfaConfirmResponse {
    pub recovery_codes: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MfaVerifyRequest {
    pub challenge_token: String,
    pub code: Option<String>,
    pub recovery_code: Option<String>,
    pub device_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SwitchOrgRequest {
    pub org_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MeResponse {
    pub user_id: String,
    pub org_id: String,
    pub roles: Vec<String>,
    pub policy_version: i64,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MembershipView {
    pub org_id: String,
    pub org_name: String,
    pub role: String,
    pub policy_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MembershipListResponse {
    pub items: Vec<MembershipView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionListResponse {
    pub items: Vec<crate::auth::sessions::SessionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SsoListResponse {
    pub items: Vec<crate::auth::sso::SsoConfigView>,
}
