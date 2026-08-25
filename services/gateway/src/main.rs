//! CompanyOS gateway / BFF — Phase 0.
//!
//! Thin layer: request-id, coarse LOCAL-ONLY authN stub, proxy hello to core.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use companyos_errors::{AppError, ErrorCode};
use companyos_telemetry::init_tracing;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Clone)]
struct GatewayState {
    core_url: String,
    client: reqwest::Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing("companyos-gateway");

    let core_url =
        std::env::var("CORE_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8081".into());
    let state = GatewayState {
        core_url,
        client: reqwest::Client::new(),
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
        .route("/api/v1/hello", any(proxy_hello))
        .route(
            "/api/v1/gateway/info",
            get(|| async {
                Json(serde_json::json!({
                    "service": "companyos-gateway",
                    "auth": "LOCAL-ONLY stub — not for production",
                    "phase": 0
                }))
            }),
        )
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

/// Coarse authN stub: require LOCAL-ONLY identity headers or Bearer before proxying.
fn coarse_authn(headers: &HeaderMap, request_id: &str) -> Result<(), AppError> {
    let has_dev = headers.contains_key("x-companyos-dev-org-id")
        && headers.contains_key("x-companyos-dev-user-id");
    let has_bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer "));
    if has_dev || has_bearer {
        tracing::debug!(
            request_id,
            "gateway LOCAL-ONLY authN stub passed — not for production"
        );
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::Unauthorized,
        request_id,
        "gateway LOCAL-ONLY authN stub: provide X-CompanyOS-Dev-* or Bearer",
    ))
}

async fn proxy_hello(State(state): State<GatewayState>, req: Request) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    if let Err(e) = coarse_authn(req.headers(), &request_id) {
        return e.into_response();
    }

    let method = req.method().clone();
    let headers = req.headers().clone();
    let body_bytes = match axum::body::to_bytes(req.into_body(), 1_048_576).await {
        Ok(b) => b,
        Err(e) => {
            return AppError::new(ErrorCode::Internal, request_id, e.to_string()).into_response();
        }
    };

    let url = format!("{}/api/v1/hello", state.core_url.trim_end_matches('/'));
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
                format!("core unreachable: {e}"),
            )
            .into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE) {
        if let Ok(v) = HeaderValue::from_bytes(ct.as_bytes()) {
            builder = builder.header(axum::http::header::CONTENT_TYPE, v);
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
