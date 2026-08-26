//! `/api/v1/finance/webhooks/stripe` — fixture-friendly Stripe payment webhook.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use serde::Deserialize;
use uuid::Uuid;

use super::{internal, is_unique_violation, not_found, parse_public_id, validation};
use crate::auth::{self};
use crate::handlers::payments::apply_allocation;
use crate::journal::{ensure_ledger_accounts, payment_entry, post_journal};
use crate::state::AppState;
use crate::types::{StripeWebhookFixture, WebhookAck};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/finance/webhooks/stripe", post(stripe_webhook))
}

#[derive(Debug, Deserialize)]
pub struct WebhookQuery {
    pub org_id: Option<String>,
}

fn extract_org_id(
    headers: &HeaderMap,
    q: &WebhookQuery,
    request_id: &str,
) -> Result<OrgId, AppError> {
    if let Some(raw) = headers
        .get("x-companyos-org-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return auth::parse_org_public_id(raw)
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail));
    }
    if let Some(raw) = q.org_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return auth::parse_org_public_id(raw)
            .map_err(|e| AppError::new(e.code, request_id.to_string(), e.detail));
    }
    Err(validation(
        request_id,
        "org required via x-companyos-org-id header or org_id query",
    ))
}

fn webhook_secret_ok(headers: &HeaderMap) -> bool {
    let expected = match std::env::var("FINANCE_WEBHOOK_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => return false,
    };
    headers
        .get("x-companyos-webhook-secret")
        .or_else(|| headers.get("x-finance-webhook-secret"))
        .and_then(|v| v.to_str().ok())
        .is_some_and(|got| got == expected)
}

/// POST /api/v1/finance/webhooks/stripe
#[utoipa::path(post, path = "/api/v1/finance/webhooks/stripe", tag = "finance-webhooks",
    request_body = StripeWebhookFixture,
    responses((status = 200, body = WebhookAck)))]
pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WebhookQuery>,
    Json(body): Json<StripeWebhookFixture>,
) -> Result<Json<WebhookAck>, AppError> {
    let request_id = auth::request_id(&headers);

    let mut actor = Actor::human(Uuid::nil());
    let mut authed = false;

    if webhook_secret_ok(&headers) {
        authed = true;
    }

    if auth::local_auth_enabled() {
        if let (Some(org_s), Some(user_s)) = (
            headers
                .get("x-companyos-dev-org-id")
                .and_then(|v| v.to_str().ok()),
            headers
                .get("x-companyos-dev-user-id")
                .and_then(|v| v.to_str().ok()),
        ) {
            let _ = org_s;
            if let Ok(uid) = auth::parse_user_public_id(user_s) {
                actor = Actor::human(uid);
                authed = true;
            }
        }
    }

    if let Some(auth_header) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            // Presence of a bearer token counts as auth attempt; verify via keyring.
            if companyos_auth_token::verify_access_token(&state.keyring, token).is_ok() {
                authed = true;
            }
        }
    }

    if !authed {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "webhook requires auth or FINANCE_WEBHOOK_SECRET",
        ));
    }

    let org_id = extract_org_id(&headers, &q, &request_id)?;
    let org_uuid = org_id.as_uuid();

    // Never store card data — only id/amount/currency/customer/invoice/status.
    let payload = serde_json::json!({
        "id": body.id,
        "type": body.type_field,
        "created": body.created,
        "data": {
            "object": {
                "id": body.data.object.id,
                "amount": body.data.object.amount,
                "currency": body.data.object.currency,
                "customer_id": body.data.object.customer_id,
                "invoice_id": body.data.object.invoice_id,
                "status": body.data.object.status,
            }
        }
    });

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_uuid)
        .await
        .map_err(internal(&request_id))?;

    let event_row_id = new_uuid_v7();
    let insert = sqlx::query(
        r#"
        INSERT INTO finance_webhook_event (
            id, org_id, provider, event_id, event_type, payload, processed_at
        ) VALUES ($1,$2,'stripe',$3,$4,$5,now())
        "#,
    )
    .bind(event_row_id)
    .bind(org_uuid)
    .bind(&body.id)
    .bind(&body.type_field)
    .bind(&payload)
    .execute(&mut *tx)
    .await;

    match insert {
        Ok(_) => {}
        Err(e) if is_unique_violation(&e, "finance_webhook_event_org_id_provider_event_id_key") => {
            tx.commit().await.map_err(internal(&request_id))?;
            return Ok(Json(WebhookAck {
                received: true,
                duplicate: true,
                payment_id: None,
            }));
        }
        Err(e) => return Err(internal(&request_id)(e)),
    }

    // Out-of-order safe: process by event id uniqueness; retries return duplicate.
    let mut payment_public: Option<String> = None;
    if body.type_field == "payment_intent.succeeded"
        || body.type_field == "payment.succeeded"
        || body.type_field == "charge.succeeded"
    {
        let obj = &body.data.object;
        if obj.amount <= 0 {
            return Err(validation(&request_id, "payment amount must be positive"));
        }
        let currency_code = obj.currency.to_uppercase();
        let currency = Currency::new(&currency_code)
            .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;

        let _: Uuid = parse_public_id(IdKind::Customer, &obj.customer_id, &request_id)?;
        let customer_id: Uuid = sqlx::query_scalar(
            "SELECT id FROM finance_customer WHERE org_id = $1 AND public_id = $2",
        )
        .bind(org_uuid)
        .bind(&obj.customer_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "customer"))?;

        let invoice_uuid = match obj.invoice_id.as_deref() {
            Some(s) => Some(parse_public_id(IdKind::Invoice, s, &request_id)?),
            None => None,
        };

        let public_id = PublicId::generate(IdKind::Payment);
        let payment_id = public_id.uuid();

        let mut allocate_amount = 0i64;
        if let Some(inv_id) = invoice_uuid {
            let bal: Option<(i64, String)> = sqlx::query_as(
                "SELECT balance_minor, status FROM finance_invoice WHERE org_id = $1 AND id = $2",
            )
            .bind(org_uuid)
            .bind(inv_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            if let Some((balance, status)) = bal {
                if !matches!(status.as_str(), "draft" | "void") {
                    allocate_amount = obj.amount.min(balance);
                }
            }
        }
        let unapplied = obj.amount - allocate_amount;

        sqlx::query(
            r#"
            INSERT INTO finance_payment (
                id, org_id, public_id, customer_id, owner_user_id, currency,
                amount_minor, amount_allocated_minor, amount_unapplied_minor,
                method, provider, provider_event_id, notes
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,0,$7,'stripe_webhook','stripe',$8,$9)
            "#,
        )
        .bind(payment_id)
        .bind(org_uuid)
        .bind(public_id.as_str())
        .bind(customer_id)
        .bind(actor.user_id)
        .bind(&currency_code)
        .bind(obj.amount)
        .bind(&body.id)
        .bind(format!("stripe {}", obj.id))
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if is_unique_violation(&e, "finance_payment_provider_event_uidx") {
                // Treat as duplicate webhook processing.
                AppError::new(
                    ErrorCode::Conflict,
                    request_id.clone(),
                    "duplicate provider event payment",
                )
            } else {
                internal(&request_id)(e)
            }
        })?;

        let journal = payment_entry(payment_id, currency, allocate_amount, unapplied)
            .map_err(|e| validation(&request_id, format!("journal: {e}")))?;
        post_journal(&mut tx, org_uuid, &journal)
            .await
            .map_err(internal(&request_id))?;

        if let Some(inv_id) = invoice_uuid {
            if allocate_amount > 0 {
                apply_allocation(
                    &mut tx,
                    org_uuid,
                    payment_id,
                    inv_id,
                    allocate_amount,
                    &request_id,
                )
                .await?;
            }
        }

        let envelope = EventEnvelope::new(
            org_id,
            Context::Finance,
            "payment",
            "received",
            1,
            actor.clone(),
            serde_json::json!({
                "id": public_id.as_str(),
                "provider": "stripe",
                "provider_event_id": body.id,
                "amount_minor": obj.amount,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

        payment_public = Some(public_id.as_str());
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(WebhookAck {
        received: true,
        duplicate: false,
        payment_id: payment_public,
    }))
}
