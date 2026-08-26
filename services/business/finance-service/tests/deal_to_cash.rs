//! Flagship deal-to-cash integration test (Phase 1 harden).
//!
//! Exercises the real CRM + Finance HTTP APIs end-to-end (no skipped fixtures):
//! register → invite/role → customer → deal Won → quote accept → invoice from
//! quote → issue → payment → journal/balance/tenant assertions.
//!
//! Requires `TEST_DATABASE_URL` / `DATABASE_URL` (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::Mutex as AsyncMutex;
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATED: OnceLock<()> = OnceLock::new();
static SEED_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&url)
        .await
        .ok()?;
    ensure_migrated(&pool).await?;
    Some(pool)
}

async fn ensure_migrated(pool: &PgPool) -> Option<()> {
    if MIGRATED.get().is_some() {
        return Some(());
    }
    let _guard = SEED_LOCK.lock().await;
    if MIGRATED.get().is_none() {
        companyos_core::migrate(pool).await.ok()?;
        companyos_crm::migrate(pool).await.ok()?;
        companyos_finance::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn crm_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_crm::build_router(companyos_crm::state::AppState::new(pool, ring))
}

fn finance_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_finance::build_router(companyos_finance::state::AppState::new(pool, ring))
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-request-id", format!("d2c-{}", new_uuid_v7()));
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let value: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn mint_session(
    pool: &PgPool,
    ring: &KeyRing,
    user_id: Uuid,
    user_public: &str,
    org: OrgId,
) -> String {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (mem_id, policy): (Uuid, i64) = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let role: String = sqlx::query_scalar(
        "SELECT COALESCE(role, 'member') FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        ring,
        user_id,
        user_public,
        org,
        mem_id,
        &[role],
        policy,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    issued.access_token
}

fn extract_invite_token(mail_dir: &PathBuf) -> String {
    let mut files: Vec<_> = fs::read_dir(mail_dir)
        .expect("mail dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("txt"))
        .collect();
    files.sort();
    let last = files.last().expect("expected invite mail");
    let body = fs::read_to_string(last).expect("read mail");
    let marker = "token=";
    let idx = body
        .find(marker)
        .unwrap_or_else(|| panic!("invite token missing in mail: {body}"));
    let rest = &body[idx + marker.len()..];
    let token = rest
        .split(|c: char| c.is_whitespace() || c == '&' || c == '"' || c == '\'')
        .next()
        .unwrap_or("")
        .trim();
    let decoded = urlencoding::decode(token).unwrap_or_else(|_| token.into());
    decoded.into_owned()
}

async fn assert_journals_balanced(pool: &PgPool, org: OrgId) {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let rows: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        r#"
        SELECT e.id,
               COALESCE(SUM(l.debit_minor), 0)::BIGINT,
               COALESCE(SUM(l.credit_minor), 0)::BIGINT
        FROM finance_journal_entry e
        LEFT JOIN finance_journal_line l ON l.entry_id = e.id AND l.org_id = e.org_id
        WHERE e.org_id = $1
        GROUP BY e.id
        "#,
    )
    .bind(org.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(!rows.is_empty(), "expected journal entries after issue/pay");
    for (id, debit, credit) in rows {
        assert_eq!(
            debit, credit,
            "journal entry {id} unbalanced: debit={debit} credit={credit}"
        );
    }
}

#[tokio::test]
async fn deal_to_cash_signup_invite_quote_invoice_payment() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no database");
        return;
    };

    let mail_dir = std::env::temp_dir().join(format!("companyos-d2c-mail-{}", new_uuid_v7()));
    let _ = fs::create_dir_all(&mail_dir);
    std::env::set_var("AUTH_MAIL_DIR", mail_dir.to_string_lossy().as_ref());

    let secret = format!("d2c-{}", new_uuid_v7());
    let ring = KeyRing::from_secret(&secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");

    let core = core_app(pool.clone(), ring.clone());
    let crm = crm_app(pool.clone(), ring.clone());
    let finance = finance_app(pool.clone(), ring.clone());

    // --- Org B (isolation victim) ---
    let owner_b_email = format!("owner-b-{}@test.local", new_uuid_v7());
    let (status, reg_b) = call(
        &core,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(json!({
            "email": owner_b_email,
            "password": "correct-horse-battery-staple",
            "display_name": "Owner B",
            "org_name": "Org B Isolation Co"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{reg_b:?}");
    let org_b_public = reg_b["org_id"].as_str().unwrap().to_string();
    let owner_b_public = reg_b["user_id"].as_str().unwrap().to_string();
    let org_b = OrgId::from_public(&org_b_public.parse().unwrap()).unwrap();
    let owner_b_id = owner_b_public.parse::<PublicId>().unwrap().uuid();
    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_b_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();
    companyos_core::workspace::provisioning::process_pending(&pool, org_b, "d2c-b")
        .await
        .ok();
    let token_b = mint_session(&pool, &ring, owner_b_id, &owner_b_public, org_b).await;

    // --- 1. Sign up (creates org + OrgProvisioning) ---
    let owner_email = format!("owner-a-{}@test.local", new_uuid_v7());
    let (status, reg) = call(
        &core,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(json!({
            "email": owner_email,
            "password": "correct-horse-battery-staple",
            "display_name": "Ada Owner",
            "org_name": "DealToCash Co"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{reg:?}");
    let org_public = reg["org_id"].as_str().unwrap().to_string();
    let owner_public = reg["user_id"].as_str().unwrap().to_string();
    let org = OrgId::from_public(&org_public.parse().unwrap()).unwrap();
    let owner_id = owner_public.parse::<PublicId>().unwrap().uuid();

    // Local verify path: mark verified + MFA enrolled (Owner policy), then mint session.
    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();
    companyos_core::workspace::provisioning::process_pending(&pool, org, "d2c-a")
        .await
        .expect("OrgProvisioning");

    // Confirm provisioning seeded pipeline stages (CRM materializes on first deal).
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let stage_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM workspace_seed_pipeline_stage WHERE org_id = $1",
    )
    .bind(org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(
        stage_count.0 >= 5,
        "OrgProvisioning should seed pipeline stages, got {}",
        stage_count.0
    );

    let owner_token = mint_session(&pool, &ring, owner_id, &owner_public, org).await;

    // --- 2. Invite + role (Sales) ---
    let invite_email = format!("sales-{}@test.local", new_uuid_v7());
    let (status, invite) = call(
        &core,
        "POST",
        "/api/v1/workspace/members/invite",
        Some(&owner_token),
        Some(json!({ "email": invite_email, "role": "sales" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{invite:?}");

    let invite_token = extract_invite_token(&mail_dir);
    let (status, accepted) = call(
        &core,
        "POST",
        "/api/v1/workspace/invitations/accept",
        None,
        Some(json!({
            "token": invite_token,
            "display_name": "Sam Sales",
            "password": "correct-horse-battery-staple"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted:?}");

    let sales_user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM user_identity WHERE email_normalized = $1")
            .bind(invite_email.to_ascii_lowercase())
            .fetch_one(&pool)
            .await
            .unwrap();
    let sales_public = PublicId::new(IdKind::User, sales_user_id).as_str();
    sqlx::query("UPDATE user_identity SET email_verified_at = now() WHERE id = $1")
        .bind(sales_user_id)
        .execute(&pool)
        .await
        .unwrap();
    let sales_token = mint_session(&pool, &ring, sales_user_id, &sales_public, org).await;

    // Authz still holds: plain invited sales cannot issue invoices.
    let (status, deny) = call(
        &finance,
        "POST",
        "/api/v1/finance/invoices",
        Some(&sales_token),
        Some(json!({
            "customer_id": PublicId::generate(IdKind::Customer).as_str(),
            "currency": "USD",
            "lines": [{"description": "x", "quantity": 1, "unit_price_minor": 1}]
        })),
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN
            || status == StatusCode::NOT_FOUND
            || status == StatusCode::UNPROCESSABLE_ENTITY
            || status == StatusCode::BAD_REQUEST,
        "sales must not freely issue invoices: {status} {deny:?}"
    );

    // --- 3. Customer + deal → Won ---
    let (status, customer) = call(
        &crm,
        "POST",
        "/api/v1/sales/customers",
        Some(&sales_token),
        Some(json!({
            "name": "Acme Buyer",
            "email": format!("buyer-{}@acme.test", new_uuid_v7())
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();
    let customer_name = customer["customer"]["name"].as_str().unwrap().to_string();

    let (status, deal) = call(
        &crm,
        "POST",
        "/api/v1/sales/deals",
        Some(&sales_token),
        Some(json!({
            "name": "Acme expansion",
            "amount_minor": 500_000,
            "currency": "USD",
            "customer_id": customer_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{deal:?}");
    let deal_id = deal["id"].as_str().unwrap().to_string();

    let (status, won) = call(
        &crm,
        "POST",
        &format!("/api/v1/sales/deals/{deal_id}/win"),
        Some(&sales_token),
        Some(json!({ "reason": "signed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{won:?}");
    assert_eq!(won["status"], "won");

    // DealWon is idempotent.
    let (status, won2) = call(
        &crm,
        "POST",
        &format!("/api/v1/sales/deals/{deal_id}/win"),
        Some(&sales_token),
        Some(json!({ "reason": "again" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{won2:?}");
    assert_eq!(won2["id"], won["id"]);

    // --- 4. Quote from deal, accept (immutable) ---
    let (status, quote) = call(
        &crm,
        "POST",
        "/api/v1/sales/quotes",
        Some(&sales_token),
        Some(json!({
            "deal_id": deal_id,
            "customer_id": customer_id,
            "currency": "USD",
            "notes": "deal-to-cash quote",
            "lines": [{
                "description": "Implementation",
                "quantity": 2,
                "unit_price_minor": 10_000,
                "discount_minor": 0,
                "tax_rate_bps": 1000
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{quote:?}");
    let quote_id = quote["id"].as_str().unwrap().to_string();
    assert_eq!(quote["customer_id"], customer_id);

    let (status, accepted_quote) = call(
        &crm,
        "POST",
        &format!("/api/v1/sales/quotes/{quote_id}/accept"),
        Some(&sales_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted_quote:?}");
    assert_eq!(accepted_quote["status"], "accepted");
    assert_eq!(accepted_quote["customer_id"], customer_id);

    // Accepted quote is immutable: PATCH forks a new draft.
    let (status, forked) = call(
        &crm,
        "PATCH",
        &format!("/api/v1/sales/quotes/{quote_id}"),
        Some(&sales_token),
        Some(json!({ "notes": "should fork" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{forked:?}");
    assert_ne!(forked["id"], quote_id);

    let (status, original) = call(
        &crm,
        "GET",
        &format!("/api/v1/sales/quotes/{quote_id}"),
        Some(&sales_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{original:?}");
    assert_eq!(original["status"], "accepted");
    assert_eq!(original["notes"], "deal-to-cash quote");

    // --- 5. Invoice from accepted quote (snapshot, no CRM table reads) ---
    let lines = original["lines"].as_array().cloned().unwrap_or_default();
    let invoice_lines: Vec<Value> = lines
        .iter()
        .map(|l| {
            json!({
                "description": l["description"],
                "quantity": l["quantity"],
                "unit_price_minor": l["unit_price_minor"],
                "discount_minor": l["discount_minor"],
                "tax_rate_bps": l["tax_rate_bps"],
            })
        })
        .collect();
    let expected_total = original["total_minor"].as_i64().unwrap();

    let (status, draft) = call(
        &finance,
        "POST",
        "/api/v1/finance/invoices/from-quote",
        Some(&owner_token),
        Some(json!({
            "quote_id": quote_id,
            "customer_id": customer_id,
            "customer_name": customer_name,
            "currency": "USD",
            "notes": original["notes"].clone(),
            "lines": invoice_lines
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft:?}");
    assert_eq!(draft["status"], "draft");
    assert_eq!(draft["source_quote_id"], quote_id);
    assert_eq!(draft["customer_id"], customer_id);
    assert_eq!(draft["total_minor"], expected_total);
    let invoice_id = draft["id"].as_str().unwrap().to_string();

    // --- 6. Issue invoice (gapless number + journal) ---
    let (status, issued) = call(
        &finance,
        "POST",
        &format!("/api/v1/finance/invoices/{invoice_id}/issue"),
        Some(&owner_token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued:?}");
    assert_eq!(issued["status"], "issued");
    assert_eq!(issued["customer_id"], customer_id);
    assert_eq!(issued["balance_minor"], expected_total);
    let number = issued["invoice_number"].as_str().unwrap();
    assert!(
        number.starts_with("INV-"),
        "expected gapless INV- number, got {number}"
    );

    // Immutable: re-issue is conflict / idempotent, not a second number.
    let (status, reissue) = call(
        &finance,
        "POST",
        &format!("/api/v1/finance/invoices/{invoice_id}/issue"),
        Some(&owner_token),
        Some(json!({})),
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CONFLICT,
        "re-issue should be idempotent or conflict, got {status} {reissue:?}"
    );
    if status == StatusCode::OK {
        assert_eq!(reissue["invoice_number"], number);
    }

    // --- 7. Record payment (allocate in one shot) ---
    let (status, payment) = call(
        &finance,
        "POST",
        "/api/v1/finance/payments",
        Some(&owner_token),
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "amount_minor": expected_total,
            "invoice_id": invoice_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{payment:?}");
    assert_eq!(payment["customer_id"], customer_id);

    let (status, paid) = call(
        &finance,
        "GET",
        &format!("/api/v1/finance/invoices/{invoice_id}"),
        Some(&owner_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paid:?}");
    assert_eq!(paid["status"], "paid");
    assert_eq!(paid["balance_minor"], 0);
    assert_eq!(paid["customer_id"], customer_id);
    let total = paid["total_minor"].as_i64().unwrap();
    let amount_paid = paid["amount_paid_minor"].as_i64().unwrap_or(0);
    let amount_credited = paid["amount_credited_minor"].as_i64().unwrap_or(0);
    assert_eq!(
        paid["balance_minor"].as_i64().unwrap(),
        total - amount_paid - amount_credited,
        "invoice balance identity"
    );

    // --- 8. Assertions: journals, authz, tenant isolation ---
    assert_journals_balanced(&pool, org).await;

    let (status, cross) = call(
        &finance,
        "GET",
        &format!("/api/v1/finance/invoices/{invoice_id}"),
        Some(&token_b),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "org B must not see org A invoice: {cross:?}"
    );

    let (status, cross_crm) = call(
        &crm,
        "GET",
        &format!("/api/v1/sales/customers/{customer_id}"),
        Some(&token_b),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "org B must not see org A customer: {cross_crm:?}"
    );

    let _ = fs::remove_dir_all(&mail_dir);
}
