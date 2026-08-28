//! CompanyOS gateway / BFF — Phase 1.9.
//!
//! Authenticates access JWTs (org-scoped), resolves tenant, runs a coarse authz
//! pre-check, attaches request context headers, and proxies to core, CRM,
//! Finance, Operations, platform, and AI.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use companyos_auth_token::{decode_jwk_k, verify_access_token, AccessClaims, KeyRing, SigningKey};
use companyos_authz::{self as authz, perms, Principal, Role};
use companyos_errors::{AppError, ErrorCode};
use companyos_telemetry::init_tracing;
use futures::stream::Stream;
use tokio::sync::RwLock;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

#[derive(Clone)]
struct GatewayState {
    core_url: String,
    crm_url: String,
    finance_url: String,
    project_url: String,
    notification_url: String,
    search_url: String,
    analytics_url: String,
    file_url: String,
    ai_url: String,
    hr_url: String,
    inventory_url: String,
    redis_url: Option<String>,
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
    let finance_url =
        std::env::var("FINANCE_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8083".into());
    let project_url =
        std::env::var("PROJECT_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8084".into());
    let notification_url = std::env::var("NOTIFICATION_SERVICE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8085".into());
    let search_url =
        std::env::var("SEARCH_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8086".into());
    let analytics_url =
        std::env::var("ANALYTICS_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8087".into());
    let file_url =
        std::env::var("FILE_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8089".into());
    let ai_url = std::env::var("AI_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8092".into());
    let hr_url = std::env::var("HR_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8088".into());
    let inventory_url = std::env::var("INVENTORY_SERVICE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8093".into());
    let redis_url = std::env::var("REDIS_URL").ok().filter(|s| !s.is_empty());
    let secret = std::env::var("AUTH_JWT_SECRET").unwrap_or_else(|_| "dev-gateway-shared".into());
    let keyring = KeyRing::from_secret(secret);
    let local_auth = matches!(
        std::env::var("COMPANYOS_LOCAL_AUTH").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    );

    let state = GatewayState {
        core_url,
        crm_url,
        finance_url,
        project_url,
        notification_url,
        search_url,
        analytics_url,
        file_url,
        ai_url,
        hr_url,
        inventory_url,
        redis_url,
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
                    "phase": "2.1"
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
        .route("/api/v1/finance/{*rest}", any(proxy_finance))
        .route("/api/v1/operations/{*rest}", any(proxy_operations))
        .route("/api/v1/people/{*rest}", any(proxy_people))
        .route("/api/v1/inventory/{*rest}", any(proxy_inventory))
        // Platform (Phase 1.8) — SSE stream registered before the catch-all.
        .route("/api/v1/notifications/stream", get(notifications_stream))
        .route("/api/v1/notifications/{*rest}", any(proxy_notifications))
        .route("/api/v1/search/{*rest}", any(proxy_search))
        .route("/api/v1/analytics/{*rest}", any(proxy_analytics))
        .route("/api/v1/files/{*rest}", any(proxy_files))
        // AI (Phase 1.9)
        .route("/api/v1/ai/{*rest}", any(proxy_ai))
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
    proxy_to(&state, req, &upstream, &state.crm_url, true, "crm").await
}

async fn proxy_finance(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(&state, req, &upstream, &state.finance_url, true, "finance").await
}

async fn proxy_operations(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(&state, req, &upstream, &state.project_url, true, "project").await
}

async fn proxy_people(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(&state, req, &upstream, &state.hr_url, true, "hr").await
}

async fn proxy_inventory(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(
        &state,
        req,
        &upstream,
        &state.inventory_url,
        true,
        "inventory",
    )
    .await
}

async fn proxy_notifications(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(
        &state,
        req,
        &upstream,
        &state.notification_url,
        true,
        "notification",
    )
    .await
}

async fn proxy_search(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(&state, req, &upstream, &state.search_url, true, "search").await
}

async fn proxy_analytics(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(
        &state,
        req,
        &upstream,
        &state.analytics_url,
        true,
        "analytics",
    )
    .await
}

async fn proxy_files(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(&state, req, &upstream, &state.file_url, true, "file").await
}

async fn proxy_ai(State(state): State<GatewayState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let upstream = with_query(&req, &path);
    proxy_to(&state, req, &upstream, &state.ai_url, true, "ai").await
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

/// GET /api/v1/notifications/stream — authenticated SSE.
///
/// Subscribes to Redis `companyos:notifications:{org_id}:{user_id}` when
/// `REDIS_URL` is set; otherwise polls the notification feed / sends keepalives.
#[allow(clippy::result_large_err)]
async fn notifications_stream(
    State(state): State<GatewayState>,
    req: Request,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let claims = match authenticate(
        &state,
        req.headers(),
        "/api/v1/notifications/stream",
        &request_id,
    )
    .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                request_id,
                "Bearer access token required for SSE",
            )
            .into_response());
        }
        Err(e) => return Err(e.into_response()),
    };

    let org_id = claims.org_id.clone();
    let user_id = claims.sub.clone();
    let channel = format!("companyos:notifications:{org_id}:{user_id}");
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let notification_url = state.notification_url.clone();
    let client = state.client.clone();
    let redis_url = state.redis_url.clone();

    let stream = async_stream::stream! {
        yield Ok(Event::default().comment("connected"));

        if let Some(url) = redis_url {
            match redis::Client::open(url.as_str()) {
                Ok(redis_client) => {
                    match redis_client.get_async_pubsub().await {
                        Ok(mut pubsub) => {
                            if let Err(e) = pubsub.subscribe(&channel).await {
                                warn!(error = %e, "redis subscribe failed; falling back to poll");
                            } else {
                                info!(%channel, "sse subscribed to redis");
                                let mut msg_stream = pubsub.on_message();
                                use futures::StreamExt;
                                while let Some(msg) = msg_stream.next().await {
                                    let payload: String = msg.get_payload().unwrap_or_default();
                                    yield Ok(Event::default().event("notification").data(payload));
                                }
                                // If the pubsub stream ends, fall through to poll below.
                            }
                        }
                        Err(e) => warn!(error = %e, "redis pubsub unavailable"),
                    }
                }
                Err(e) => warn!(error = %e, "invalid REDIS_URL"),
            }
        }

        // Fallback / keep-alive: poll notification feed periodically.
        let mut last_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        loop {
            tick.tick().await;
            let feed_url = format!(
                "{}/api/v1/notifications/feed",
                notification_url.trim_end_matches('/')
            );
            let mut req = client.get(&feed_url);
            if let Some(ref auth) = auth_header {
                req = req.header(axum::http::header::AUTHORIZATION, auth);
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if let Some(items) = body.get("items").and_then(|i| i.as_array()) {
                            for item in items {
                                let id = item
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if id.is_empty() || last_ids.contains(&id) {
                                    continue;
                                }
                                last_ids.insert(id);
                                let data = item.to_string();
                                yield Ok(Event::default().event("notification").data(data));
                            }
                        }
                    }
                }
                _ => {
                    yield Ok(Event::default().comment("keepalive"));
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20))))
}
