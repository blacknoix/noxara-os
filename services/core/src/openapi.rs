//! OpenAPI 3.1 document for Hello + Auth + Workspace (contract chain source).

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
        crate::auth::handlers::register,
        crate::auth::handlers::login,
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
    ),
    components(schemas(
        Hello,
        CreateHelloRequest,
        HelloListResponse,
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
    )),
    tags(
        (name = "hello", description = "Phase 0 hello vertical slice"),
        (name = "auth", description = "Phase 1.1 identity & authentication"),
        (name = "workspace", description = "Phase 1.2 organizations, members, roles, teams"),
    ),
    info(
        title = "CompanyOS Core API",
        version = "0.3.0",
        description = "Phase 1.2 — workspace (orgs, memberships, roles, permissions) + auth + hello."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

/// Write the OpenAPI document as pretty JSON (used by the codegen script).
#[allow(dead_code)]
pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
