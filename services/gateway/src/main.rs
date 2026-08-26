//! CompanyOS gateway / BFF — Phase 1.4.
//!
//! Authenticates access JWTs (org-scoped), resolves tenant, runs a coarse authz
//! pre-check, attaches request context headers, and proxies to core (auth,
//! workspace, dashboard) and CRM (sales).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use companyos_auth_token::{decode_jwk_k, verify_access_token, AccessClaims, KeyRing, SigningKey};
use companyos_authz::{self as authz, perms, Principal, Role};
use companyos_errors::{AppError, ErrorCode};
use companyos_telemetry::init_tracing;
use tokio::sync::RwLock;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Clone)]
struct GatewayState {
    core_url: String,
    crm_url: String,
    client: reqwest::Client,
    keyring: KeyRing,
    jwks_cache: Arc<RwLock<JwksCache>>,
    local_auth: bool,
}

struct JwksCache {
    fetched_at: Option<Instant>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-gateway");

    let core_url =
        std::env::var("CORE_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8081".into());
    let crm_url =
        std::env::var("CRM_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8082".into());
    let secret = std::env::var("AUTH_JWT_SECRET").unwrap_or_else(|_| "dev-gateway-shared".into());
    let keyring = KeyRing::from_secret(secret);
    let local_auth = matches!(
        std::env::var("COMPANYOS_LOCAL_AUTH").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    );

    let state = GatewayState {
        core_url,
        crm_url,
        client: reqwest::Client::new(),
        keyring,
        jwks_cache: Arc::new(RwLock::new(JwksCache { fetched_at: None })),
        local_auth,
    };

    let x_request_id = http::HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route(
            "/livez",
            get(|| async { Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route(
            "/readyz",
            get(|| async { Json(serde_json::json!({ "status": "ready" })) }),
        )
        .route(
            "/healthz",
            get(|| async {
                Json(serde_json::json!({ "status": "ok", "service": "companyos-gateway" }))
            }),
        )
        .route(
            "/api/v1/gateway/info",
            get(|State(state): State<GatewayState>| async move {
                Json(serde_json::json!({
                    "service": "companyos-gateway",
                    "auth": if state.local_auth {
                        "JWT primary + LOCAL-ONLY bypass enabled"
                    } else {
                        "JWT primary (LOCAL-ONLY bypass off)"
                    },
                    "phase": "1.4"
                }))
            }),
        )
        // Auth endpoints: proxy without requiring access token (login/register/refresh…).
        .route("/api/v1/auth/{*rest}", any(proxy_auth))
        .route("/api/v1/openapi.json", any(proxy_openapi))
        .route("/api/v1/hello", any(proxy_hello))
        .route("/api/v1/dashboard", any(proxy_dashboard))
        .route("/api/v1/workspace/{*rest}", any(proxy_workspace))
        .route("/api/v1/sales/{*rest}", any(proxy_sales))
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state);

    let addr: SocketAddr = std::env::var("GATEWAY_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    info!(%addr, "companyos-gateway listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn refresh_jwks(state: &GatewayState) {
    let mut cache = state.jwks_cache.write().await;
    if cache
        .fetched_at
        .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
    {
        return;
    }
    let url = format!(
        "{}/api/v1/auth/jwks.json",
        state.core_url.trim_end_matches('/')
    );
    if let Ok(resp) = state.client.get(&url).send().await {
        if let Ok(doc) = resp.json::<serde_json::Value>().await {
            if let Some(keys) = doc.get("keys").and_then(|k| k.as_array()) {
                for k in keys {
                    let kid = k.get("kid").and_then(|v| v.as_str()).unwrap_or("");
                    let material = k.get("k").and_then(|v| v.as_str()).unwrap_or("");
                    if kid.is_empty() || material.is_empty() {
                        continue;
                    }
                    if let Ok(bytes) = decode_jwk_k(material) {
                        if let Ok(secret) = String::from_utf8(bytes) {
                            state.keyring.upsert(SigningKey {
                                kid: kid.to_string(),
                                secret,
                                active: false, // verification-only; core owns active mint key
                            });
                        }
                    }
                }
            }
            cache.fetched_at = Some(Instant::now());
        }
    }
}

fn coarse_authz(claims: &AccessClaims, path: &str) -> Result<(), &'static str> {
    let roles: Vec<Role> = claims.roles.iter().filter_map(|r| Role::parse(r)).collect();
    let principal = Principal::with_roles(roles);
    // Coarse pre-check: hello + dashboard require workspace.dashboard.read.
    if (path.starts_with("/api/v1/hello") || path.starts_with("/api/v1/dashboard"))
        && !authz::is_allowed(&principal, &perms::workspace_dashboard_read())
    {
        return Err("missing workspace.dashboard.read");
    }
    Ok(())
}

fn local_bypass_ok(headers: &HeaderMap) -> bool {
    headers.contains_key("x-companyos-dev-org-id")
        && headers.contains_key("x-companyos-dev-user-id")
}

async fn authenticate(
    state: &GatewayState,
    headers: &HeaderMap,
    path: &str,
    request_id: &str,
) -> Result<Option<AccessClaims>, AppError> {
    // Auth routes and openapi are public at the gateway.
    if path.starts_with("/api/v1/auth/") || path.starts_with("/api/v1/openapi") {
        return Ok(None);
    }

    if let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        refresh_jwks(state).await;
        match verify_access_token(&state.keyring, auth) {
            Ok(claims) => {
                coarse_authz(&claims, path)
                    .map_err(|d| AppError::new(ErrorCode::Forbidden, request_id, d))?;
                return Ok(Some(claims));
            }
            Err(e) => {
                if state.local_auth && auth.matches('.').count() != 2 {
                    tracing::warn!(request_id, "gateway accepting LOCAL-ONLY unsigned bearer");
                    return Ok(None);
                }
                return Err(AppError::new(
                    ErrorCode::Unauthorized,
                    request_id,
                    format!("invalid access token: {e}"),
                ));
            }
        }
    }

    if state.local_auth && local_bypass_ok(headers) {
        tracing::warn!(request_id, "gateway LOCAL-ONLY header bypass");
        return Ok(None);
    }

    Err(AppError::new(
        ErrorCode::Unauthorized,
        request_id,
        "Bearer access token required",
    ))
}

fn with_query(req: &Request, path: &str) -> String {
    match req.uri().query() {
        Some(q) if !path.contains('?') => format!("{path}?{q}"),
        _ => path.to_string(),
    }
}

async fn proxy_to(
    state: &GatewayState,
    req: Request,
    upstream_path: &str,
    base_url: &str,
    require_auth: bool,
    upstream_label: &str,
) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Authz path check uses path without query.
    let auth_path = upstream_path
        .split_once('?')
        .map(|(p, _)| p)
        .unwrap_or(upstream_path);

    let claims = if require_auth {
        match authenticate(state, req.headers(), auth_path, &request_id).await {
            Ok(c) => c,
            Err(e) => return e.into_response(),
        }
    } else {
        None
    };

    let method = req.method().clone();
    let mut headers = req.headers().clone();
    let body_bytes = match axum::body::to_bytes(req.into_body(), 2_097_152).await {
        Ok(b) => b,
        Err(e) => {
            return AppError::new(ErrorCode::Internal, request_id, e.to_string()).into_response();
        }
    };

    if let Some(c) = claims {
        // Propagate resolved tenant + actor context for core (in addition to Bearer).
        if let Ok(v) = HeaderValue::from_str(&c.org_id) {
            headers.insert("x-companyos-org-id", v);
        }
        if let Ok(v) = HeaderValue::from_str(&c.sub) {
            headers.insert("x-companyos-user-id", v);
        }
        if let Ok(v) = HeaderValue::from_str(&c.sid.to_string()) {
            headers.insert("x-companyos-session-id", v);
        }
    }

    let url = format!("{}{}", base_url.trim_end_matches('/'), upstream_path);
    let mut outbound = state.client.request(method, &url);
    for (k, v) in headers.iter() {
        if k == axum::http::header::HOST || k == axum::http::header::CONTENT_LENGTH {
            continue;
        }
        outbound = outbound.header(k, v);
    }
    outbound = outbound.header("x-request-id", &request_id);

    let resp = match outbound.body(body_bytes).send().await {
        Ok(r) => r,
        Err(e) => {
            return AppError::new(
                ErrorCode::ServiceUnavailable,
                request_id,
                format!("{upstream_label} unreachable: {e}"),
            )
            .into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for name in [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::SET_COOKIE,
        axum::http::header::CACHE_CONTROL,
    ] {
        for val in resp.headers().get_all(&name) {
            if let Ok(v) = HeaderValue::from_bytes(val.as_bytes()) {
                builder = builder.header(name.clone(), v);
            }
        }
    }
    builder = builder.header("x-request-id", &request_id);
    let bytes = resp.bytes().await.unwrap_or_default();
    builder.body(Body::from(bytes)).unwrap_or_else(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "gateway response build failed",
        )
            .into_response()
    })
}

async fn proxy_hello(State(state): State<GatewayState>, req: Request) -> Response {
    let upstream = with_query(&req, "/api/v1/hello");
    proxy_to(&state, req, &upstream, &state.core_url, true, "core").await
}

async fn proxy_dashboard(State(state): State<GatewayState>, req: Request) -> Response {
    let upstream = with_query(&req, "/api/v1/dashboard");
    proxy_to(&state, req, &upstream, &state.core_url, true, "core").await
}

async fn proxy_workspace(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(&state, req, &upstream, &state.core_url, true, "core").await
}

async fn proxy_sales(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    // Coarse auth: authenticate only; CRM enforces sales.* permissions.
    proxy_to(&state, req, &upstream, &state.crm_url, true, "crm").await
}

async fn proxy_openapi(State(state): State<GatewayState>, req: Request) -> Response {
    proxy_to(
        &state,
        req,
        "/api/v1/openapi.json",
        &state.core_url,
        false,
        "core",
    )
    .await
}

async fn proxy_auth(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    // Some auth admin routes require auth — core enforces; gateway lets Bearer through.
    proxy_to(&state, req, &upstream, &state.core_url, false, "core").await
}
