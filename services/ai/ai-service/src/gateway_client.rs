//! Forward requests to gateway/domain APIs using the invoking user's bearer token.

use axum::http::{Method, StatusCode};
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use serde_json::Value;
use uuid::Uuid;

use crate::state::AppState;

const AI_ACTOR_HEADER: &str = "x-companyos-actor-is-ai";
const ON_BEHALF_HEADER: &str = "x-companyos-on-behalf-of";

pub async fn forward_user_request(
    state: &AppState,
    bearer: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
    as_ai: bool,
    on_behalf_of: Uuid,
    request_id: &str,
) -> Result<(StatusCode, Value), AppError> {
    let url = format!("{}{}", state.gateway_url.trim_end_matches('/'), path);
    let mut req = state.http.request(method, url).header(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {bearer}"),
    );

    if as_ai {
        req = req.header(AI_ACTOR_HEADER, "true").header(
            ON_BEHALF_HEADER,
            PublicId::new(IdKind::User, on_behalf_of).as_str(),
        );
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::new(ErrorCode::ServiceUnavailable, request_id, e.to_string()))?;

    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);

    Ok((status, v))
}
