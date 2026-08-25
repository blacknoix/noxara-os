//! OpenAPI 3.1 document for Hello + Auth (contract chain source).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::auth::handlers::types::{
    LoginRequest, MagicLinkConsumeRequest, MagicLinkRequest, MeResponse, MembershipListResponse,
    MembershipView, MessageResponse, MfaChallengeResponse, MfaConfirmRequest, MfaConfirmResponse,
    MfaSetupRequest, MfaSetupResponse, MfaVerifyRequest, PasswordResetConfirm,
    PasswordResetRequest, RegisterRequest, RegisterResponse, ResendVerificationRequest,
    SessionListResponse, SsoListResponse, SwitchOrgRequest, TokenResponse, VerifyEmailRequest,
};
use crate::auth::sessions::SessionView;
use crate::auth::sso::{SsoConfigView, UpsertSsoRequest};
use crate::hello::{CreateHelloRequest, Hello, HelloListResponse};
use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::hello::list_hello,
        crate::hello::create_hello,
        crate::auth::handlers::register,
        crate::auth::handlers::login,
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
        MessageResponse,
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
    )),
    tags(
        (name = "hello", description = "Phase 0 hello vertical slice"),
        (name = "auth", description = "Phase 1.1 identity & authentication"),
    ),
    info(
        title = "CompanyOS Core API",
        version = "0.2.0",
        description = "Phase 1.1 — org-scoped JWT auth + hello resource. LOCAL-ONLY auth is opt-in via COMPANYOS_LOCAL_AUTH=1."
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
