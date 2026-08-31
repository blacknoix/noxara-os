//! OpenAPI 3.1 document for Hello + Auth + Workspace + Dashboard (contract chain source).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::auth::handlers::types::{
    LoginRequest, MagicLinkConsumeRequest, MagicLinkRequest, MeResponse, MembershipListResponse,
    MembershipView, MessageResponse as AuthMessageResponse, MfaChallengeResponse,
    MfaConfirmRequest, MfaConfirmResponse, MfaSetupRequest, MfaSetupResponse, MfaVerifyRequest,
    PasswordResetConfirm, PasswordResetRequest, RegisterRequest, RegisterResponse,
    ResendVerificationRequest, SessionListResponse, SsoListResponse, SwitchOrgRequest,
    TokenResponse, VerifyEmailRequest,
};
use crate::auth::sessions::SessionView;
use crate::auth::sso::{SsoConfigView, UpsertSsoRequest};
use crate::dashboard::{DashboardResponse, DashboardWidget};
use crate::governance::types::{
    AccessReviewKickoffRequest, AccessReviewQuery, AccessReviewRunView, ApiKeyExchangeRequest,
    ApiKeyExchangeResponse, ApiKeyListResponse, ApiKeyView, AuditReadRow, AuditVerifyRequest,
    AuditVerifyResponse, CreateApiKeyRequest, CreateApiKeyResponse, CreateWebhookEndpointRequest,
    CreateWebhookEndpointResponse, DisableWebhookRequest, EntitlementRow, ReplayWebhookResponse,
    RetentionConfigView, RetentionDryRunResponse, RotateApiKeyResponse,
    RotateWebhookSecretResponse, UpdateRetentionRequest, WebhookDeliveryListResponse,
    WebhookDeliveryView, WebhookEndpointListResponse, WebhookEndpointView, WhoCouldSeeResponse,
    WhoDidSeeResponse,
};
use crate::hello::{CreateHelloRequest, Hello, HelloListResponse};
use crate::state::AppState;
use crate::workspace::types::{
    AcceptInviteRequest, CapabilityPreviewResponse, ChangeRoleRequest, CreateDepartmentRequest,
    CreateOrgRequest, CreateTeamRequest, DepartmentListResponse, DepartmentView,
    InviteMemberRequest, InviteResponse, MemberListResponse, MemberView, MessageResponse,
    MyCapabilitiesResponse, OrgResponse, PermissionCatalogueItem, PermissionCatalogueResponse,
    RoleListResponse, RolePermissionInput, RolePermissionView, RoleView, TeamListResponse,
    TeamView, UpdateOrgSettingsRequest, UpsertRoleRequest,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::hello::list_hello,
        crate::hello::create_hello,
        crate::dashboard::get_dashboard,
        crate::auth::handlers::register,
        crate::auth::handlers::login,
        crate::auth::handlers::flows::list_sso,
        crate::auth::handlers::flows::create_sso,
        crate::auth::handlers::flows::sso_start,
        crate::auth::handlers::flows::sso_callback,
        crate::workspace::handlers::create_organization,
        crate::workspace::handlers::get_organization,
        crate::workspace::handlers::update_settings,
        crate::workspace::handlers::list_members,
        crate::workspace::handlers::invite_member,
        crate::workspace::handlers::accept_invitation,
        crate::workspace::handlers::change_member_role,
        crate::workspace::handlers::suspend_member,
        crate::workspace::handlers::revoke_member,
        crate::workspace::handlers::list_roles,
        crate::workspace::handlers::get_role,
        crate::workspace::handlers::create_role,
        crate::workspace::handlers::update_role,
        crate::workspace::handlers::preview_role_capabilities,
        crate::workspace::handlers::list_permissions,
        crate::workspace::handlers::my_capabilities,
        crate::workspace::handlers::list_teams,
        crate::workspace::handlers::create_team,
        crate::workspace::handlers::list_departments,
        crate::workspace::handlers::create_department,
        crate::governance::handlers::who_could_see,
        crate::governance::handlers::who_did,
        crate::governance::handlers::kickoff_run,
        crate::governance::handlers::get_run,
        crate::governance::handlers::export_run,
        crate::governance::handlers::verify_audit,
        crate::governance::handlers::get_retention,
        crate::governance::handlers::update_retention,
        crate::governance::handlers::retention_dry_run,
        crate::governance::handlers::list_api_keys,
        crate::governance::handlers::create_api_key,
        crate::governance::handlers::rotate_api_key,
        crate::governance::handlers::revoke_api_key,
        crate::governance::handlers::list_webhooks,
        crate::governance::handlers::create_webhook,
        crate::governance::handlers::rotate_webhook,
        crate::governance::handlers::disable_webhook,
        crate::governance::handlers::list_webhook_deliveries,
        crate::governance::handlers::replay_webhook_delivery,
        crate::governance::handlers::exchange_api_key,
    ),
    components(schemas(
        Hello,
        CreateHelloRequest,
        HelloListResponse,
        DashboardResponse,
        DashboardWidget,
        RegisterRequest,
        RegisterResponse,
        LoginRequest,
        TokenResponse,
        MfaChallengeResponse,
        VerifyEmailRequest,
        AuthMessageResponse,
        ResendVerificationRequest,
        MagicLinkRequest,
        MagicLinkConsumeRequest,
        PasswordResetRequest,
        PasswordResetConfirm,
        MfaSetupRequest,
        MfaSetupResponse,
        MfaConfirmRequest,
        MfaConfirmResponse,
        MfaVerifyRequest,
        SwitchOrgRequest,
        MeResponse,
        MembershipView,
        MembershipListResponse,
        SessionView,
        SessionListResponse,
        SsoConfigView,
        UpsertSsoRequest,
        SsoListResponse,
        CreateOrgRequest,
        OrgResponse,
        UpdateOrgSettingsRequest,
        MemberView,
        MemberListResponse,
        InviteMemberRequest,
        InviteResponse,
        AcceptInviteRequest,
        ChangeRoleRequest,
        RoleView,
        RolePermissionView,
        RoleListResponse,
        UpsertRoleRequest,
        RolePermissionInput,
        CapabilityPreviewResponse,
        PermissionCatalogueResponse,
        PermissionCatalogueItem,
        TeamView,
        TeamListResponse,
        CreateTeamRequest,
        DepartmentView,
        DepartmentListResponse,
        CreateDepartmentRequest,
        MessageResponse,
        MyCapabilitiesResponse,
        AccessReviewQuery,
        EntitlementRow,
        WhoCouldSeeResponse,
        AuditReadRow,
        WhoDidSeeResponse,
        AccessReviewKickoffRequest,
        AccessReviewRunView,
        AuditVerifyRequest,
        AuditVerifyResponse,
        RetentionConfigView,
        UpdateRetentionRequest,
        RetentionDryRunResponse,
        ApiKeyView,
        ApiKeyListResponse,
        CreateApiKeyRequest,
        CreateApiKeyResponse,
        RotateApiKeyResponse,
        WebhookEndpointView,
        WebhookEndpointListResponse,
        CreateWebhookEndpointRequest,
        CreateWebhookEndpointResponse,
        RotateWebhookSecretResponse,
        DisableWebhookRequest,
        WebhookDeliveryView,
        WebhookDeliveryListResponse,
        ReplayWebhookResponse,
        ApiKeyExchangeRequest,
        ApiKeyExchangeResponse,
    )),
    tags(
        (name = "hello", description = "Phase 0 hello vertical slice"),
        (name = "auth", description = "Phase 1.1 identity & authentication"),
        (name = "workspace", description = "Phase 1.2 organizations, members, roles, teams"),
        (name = "dashboard", description = "Phase 1.3 dashboard BFF widget descriptors"),
        (name = "governance", description = "Phase 2.6–3.3 access review, audit, retention, API keys, outbound webhooks"),
        (name = "internal", description = "Internal service-to-service endpoints (not public)"),
    ),
    info(
        title = "CompanyOS Core API",
        version = "0.6.0",
        description = "Phase 3.3 — public API keys, outbound webhooks, governance + dashboard BFF + workspace + auth + hello."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/openapi.json",
            get(|| async { Json(ApiDoc::openapi()) }),
        )
        .route(
            "/api/v1/openapi.public.json",
            get(|| async { Json(crate::public_openapi::public_openapi()) }),
        )
}

/// Write the OpenAPI document as pretty JSON (used by the codegen script).
#[allow(dead_code)]
pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
