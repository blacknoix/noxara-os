//! Phase 1.5 Finance integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).
//!
//! Pattern mirrors `services/business/crm-service/tests/crm_phase14.rs`:
//! register an owner via core's HTTP API (so `OrgProvisioning` seeds system
//! roles + `role_permission`), insert additional memberships, mint access
//! tokens with `companyos_core::auth::sessions::create_session_with_tokens`
//! on a shared `KeyRing`, then migrate core then finance and drive
//! `companyos_finance::build_router` with `tower::ServiceExt::oneshot`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_events::{Context, EventEnvelope};
use companyos_finance::state::AppState as FinanceAppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex as AsyncMutex;
use tower::ServiceExt;
use uuid::Uuid;

/// Migrate once — concurrent DDL from every test is racy under FORCE RLS.
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
        companyos_finance::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn finance_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_finance::build_router(FinanceAppState::new(pool, ring))
}

/// One org, three memberships: owner (all perms), finance role (day-to-day
/// finance ops), and member (no `finance.invoice.issue` — authz deny).
struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    org_public: String,
    owner_token: String,
    #[allow(dead_code)]
    finance_token: String,
    member_token: String,
    owner_id: Uuid,
    #[allow(dead_code)]
    finance_user_id: Uuid,
    #[allow(dead_code)]
    member_user_id: Uuid,
}

async fn membership_role_and_policy(pool: &PgPool, org: OrgId, user_id: Uuid) -> (Uuid, i64) {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let row: (Uuid, i64) = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    row
}

async fn insert_member_with_role(
    pool: &PgPool,
    org: OrgId,
    role_key: &str,
    display_name: &str,
) -> Uuid {
    let email = format!("{role_key}-{}@test.local", new_uuid_v7());
    let user_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::User, user_id).as_str();
    let (hash, salt) = password::hash_password("correct-horse-battery-staple").unwrap();
    sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,now())
        "#,
    )
    .bind(user_id)
    .bind(&public_id)
    .bind(&email)
    .bind(email.to_ascii_lowercase())
    .bind(&hash)
    .bind(&salt)
    .bind(display_name)
    .execute(pool)
    .await
    .unwrap();

    let mem_id = new_uuid_v7();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let role_id: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_role WHERE org_id = $1 AND system_key = $2")
            .bind(org.as_uuid())
            .bind(role_key)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO membership (id, org_id, user_id, public_id, role, role_id, policy_version, status)
        VALUES ($1,$2,$3,$4,$5,$6,1,'active')
        "#,
    )
    .bind(mem_id)
    .bind(org.as_uuid())
    .bind(user_id)
    .bind(PublicId::new(IdKind::Membership, mem_id).as_str())
    .bind(role_key)
    .bind(role_id.map(|r| r.0))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    user_id
}

async fn seed_org_with_tokens(secret: &str) -> Option<Seeded> {
    // Connect + migrate before taking the seed lock (SEED_LOCK is not reentrant).
    let pool = pool().await?;
    // Serialize org registration + membership inserts; parallel seeds race on
    // shared auth tables / RLS session bindings under load.
    let _guard = SEED_LOCK.lock().await;
    let ring = KeyRing::from_secret(secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    let app = core_app(pool.clone(), ring.clone());

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "fin-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Finance Phase15 Test Co"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let org_public = body["org_id"].as_str().unwrap().to_string();
    let owner_public = body["user_id"].as_str().unwrap().to_string();

    let org = OrgId::from_public(&org_public.parse().unwrap()).unwrap();
    let owner_id = owner_public.parse::<PublicId>().unwrap().uuid();

    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();

    companyos_core::workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok();

    let finance_user_id = insert_member_with_role(&pool, org, "finance", "Finance Rep").await;
    let member_user_id = insert_member_with_role(&pool, org, "member", "Plain Member").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (finance_mem_id, finance_policy) =
        membership_role_and_policy(&pool, org, finance_user_id).await;
    let (member_mem_id, member_policy) =
        membership_role_and_policy(&pool, org, member_user_id).await;

    let mut tx = pool.begin().await.unwrap();
    let owner_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        owner_id,
        &owner_public,
        org,
        owner_mem_id,
        &["owner".into()],
        owner_policy,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let finance_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        finance_user_id,
        &PublicId::new(IdKind::User, finance_user_id).as_str(),
        org,
        finance_mem_id,
        &["finance".into()],
        finance_policy,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let member_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        member_user_id,
        &PublicId::new(IdKind::User, member_user_id).as_str(),
        org,
        member_mem_id,
        &["member".into()],
        member_policy,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    Some(Seeded {
        pool,
        ring,
        org,
        org_public,
        owner_token: owner_issued.access_token,
        finance_token: finance_issued.access_token,
        member_token: member_issued.access_token,
        owner_id,
        finance_user_id,
        member_user_id,
    })
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", format!("fin-{}", new_uuid_v7()));
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

async fn call_with_headers(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    extra: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-request-id", format!("fin-{}", new_uuid_v7()));
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
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

async fn project_customer(app: &Router, seeded: &Seeded, name: &str) -> String {
    let customer_id = PublicId::generate(IdKind::Customer).as_str();
    let envelope = EventEnvelope::new(
        seeded.org,
        Context::Sales,
        "customer",
        "created",
        1,
        Actor::human(seeded.owner_id),
        json!({
            "id": customer_id,
            "name": name,
            "email": format!("{}@test.local", new_uuid_v7()),
            "currency": "USD",
        }),
    );
    let (status, resp) = call(
        app,
        "POST",
        "/api/v1/finance/events/sales/apply",
        &seeded.owner_token,
        Some(json!({ "envelope": envelope })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp:?}");
    assert_eq!(resp["applied"], true);
    customer_id
}

async fn create_and_issue_invoice(
    app: &Router,
    token: &str,
    customer_id: &str,
    lines: Value,
) -> Value {
    let (status, draft) = call(
        app,
        "POST",
        "/api/v1/finance/invoices",
        token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "lines": lines,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft:?}");
    let id = draft["id"].as_str().unwrap();
    let (status, issued) = call(
        app,
        "POST",
        &format!("/api/v1/finance/invoices/{id}/issue"),
        token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued:?}");
    assert_eq!(issued["status"], "issued");
    issued
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
    assert!(!rows.is_empty(), "expected at least one journal entry");
    for (id, debit, credit) in rows {
        assert_eq!(
            debit, credit,
            "journal entry {id} unbalanced: debit={debit} credit={credit}"
        );
    }
}

#[tokio::test]
async fn customer_projection_via_sales_apply() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-proj").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Projected Co").await;

    let (status, cust) = call(
        &app,
        "GET",
        &format!("/api/v1/finance/customers/{customer_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cust:?}");
    assert_eq!(cust["name"], "Projected Co");
    assert_eq!(cust["sales_customer_id"], customer_id);
}

#[tokio::test]
async fn invoice_balance_identity_after_issue_partial_payment_and_credit() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-balance").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Balance Co").await;

    // 2 * 5000 = 10000, no tax.
    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "Service",
            "quantity": 2,
            "unit_price_minor": 5000,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_id = issued["id"].as_str().unwrap().to_string();
    let total = issued["total_minor"].as_i64().unwrap();
    assert_eq!(total, 10_000);
    assert_eq!(issued["balance_minor"], total);
    assert_eq!(issued["amount_paid_minor"], 0);
    assert_eq!(issued["amount_credited_minor"], 0);

    // Partial payment of 3000.
    let (status, pay) = call(
        &app,
        "POST",
        "/api/v1/finance/payments",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "amount_minor": 3000,
            "invoice_id": inv_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{pay:?}");

    let (status, after_pay) = call(
        &app,
        "GET",
        &format!("/api/v1/finance/invoices/{inv_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after_pay:?}");
    assert_eq!(after_pay["amount_paid_minor"], 3000);
    assert_eq!(after_pay["balance_minor"], 7000);
    assert_eq!(
        after_pay["total_minor"].as_i64().unwrap()
            - after_pay["amount_paid_minor"].as_i64().unwrap()
            - after_pay["amount_credited_minor"].as_i64().unwrap(),
        after_pay["balance_minor"].as_i64().unwrap()
    );
    assert_eq!(after_pay["status"], "partially_paid");

    // Credit note for 2000.
    let (status, credit) = call(
        &app,
        "POST",
        "/api/v1/finance/credit-notes",
        &seeded.owner_token,
        Some(json!({
            "invoice_id": inv_id,
            "reason": "partial refund",
            "lines": [{
                "description": "Credit",
                "quantity": 1,
                "unit_price_minor": 2000,
                "discount_minor": 0,
                "tax_rate_bps": 0
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{credit:?}");
    assert_eq!(credit["total_minor"], 2000);

    let (status, after_credit) = call(
        &app,
        "GET",
        &format!("/api/v1/finance/invoices/{inv_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after_credit:?}");
    assert_eq!(after_credit["amount_credited_minor"], 2000);
    assert_eq!(after_credit["balance_minor"], 5000);
    assert_eq!(
        after_credit["total_minor"].as_i64().unwrap()
            - after_credit["amount_paid_minor"].as_i64().unwrap()
            - after_credit["amount_credited_minor"].as_i64().unwrap(),
        after_credit["balance_minor"].as_i64().unwrap(),
        "balance identity: total - paid - credited = balance"
    );
}

#[tokio::test]
async fn credit_note_reduces_balance() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-credit").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Credit Co").await;
    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "Item",
            "quantity": 1,
            "unit_price_minor": 8000,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_id = issued["id"].as_str().unwrap();
    let before = issued["balance_minor"].as_i64().unwrap();

    let (status, credit) = call(
        &app,
        "POST",
        "/api/v1/finance/credit-notes",
        &seeded.owner_token,
        Some(json!({
            "invoice_id": inv_id,
            "lines": [{
                "description": "Adjustment",
                "quantity": 1,
                "unit_price_minor": 1500,
                "discount_minor": 0,
                "tax_rate_bps": 0
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{credit:?}");

    let (status, after) = call(
        &app,
        "GET",
        &format!("/api/v1/finance/invoices/{inv_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after["balance_minor"].as_i64().unwrap(), before - 1500);
}

#[tokio::test]
async fn allocation_exceeding_total_is_rejected() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-overalloc").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Alloc Co").await;
    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "Widget",
            "quantity": 1,
            "unit_price_minor": 1000,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_id = issued["id"].as_str().unwrap().to_string();

    // Unallocated payment of 500.
    let (status, pay) = call(
        &app,
        "POST",
        "/api/v1/finance/payments",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "amount_minor": 500,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{pay:?}");
    let pay_id = pay["id"].as_str().unwrap();

    // Over-allocate relative to unapplied (and would exceed remaining if larger).
    let (status, err) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/payments/{pay_id}/allocate"),
        &seeded.owner_token,
        Some(json!({
            "invoice_id": inv_id,
            "amount_minor": 9999,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err:?}");

    // Allocate the full unapplied, then try again past invoice balance.
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/payments/{pay_id}/allocate"),
        &seeded.owner_token,
        Some(json!({ "invoice_id": inv_id, "amount_minor": 500 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, pay2) = call(
        &app,
        "POST",
        "/api/v1/finance/payments",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "amount_minor": 2000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{pay2:?}");
    let pay2_id = pay2["id"].as_str().unwrap();

    // Invoice balance remaining is 500; allocating 600 must fail.
    let (status, err) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/payments/{pay2_id}/allocate"),
        &seeded.owner_token,
        Some(json!({ "invoice_id": inv_id, "amount_minor": 600 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err:?}");
}

#[tokio::test]
async fn journal_debits_equal_credits_after_invoice_payment_credit_expense() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-journal").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Journal Co").await;

    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "Consulting",
            "quantity": 1,
            "unit_price_minor": 10_000,
            "discount_minor": 0,
            "tax_rate_bps": 1000
        }]),
    )
    .await;
    let inv_id = issued["id"].as_str().unwrap();

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/finance/payments",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "amount_minor": 5000,
            "invoice_id": inv_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/finance/credit-notes",
        &seeded.owner_token,
        Some(json!({
            "invoice_id": inv_id,
            "lines": [{
                "description": "Credit",
                "quantity": 1,
                "unit_price_minor": 1000,
                "discount_minor": 0,
                "tax_rate_bps": 0
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Auto-approved expense (owner role has NULL approval_limit → unlimited).
    let (status, expense) = call(
        &app,
        "POST",
        "/api/v1/finance/expenses",
        &seeded.owner_token,
        Some(json!({
            "currency": "USD",
            "amount_minor": 2500,
            "description": "Office supplies",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{expense:?}");
    assert_eq!(expense["status"], "posted");

    assert_journals_balanced(&seeded.pool, seeded.org).await;
}

#[tokio::test]
async fn issued_invoice_document_fields_immutable_at_db() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-immut-upd").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Immut Co").await;
    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "Locked",
            "quantity": 1,
            "unit_price_minor": 4200,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_public = issued["id"].as_str().unwrap();
    let original_total = issued["total_minor"].as_i64().unwrap();

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let err = sqlx::query(
        "UPDATE finance_invoice SET total_minor = $2 WHERE org_id = $1 AND public_id = $3",
    )
    .bind(seeded.org.as_uuid())
    .bind(original_total + 1)
    .bind(inv_public)
    .execute(&mut *tx)
    .await;
    assert!(
        err.is_err(),
        "issued invoice total_minor UPDATE must fail at DB trigger"
    );
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("immutability") || msg.contains("cannot mutate"),
        "unexpected error: {msg}"
    );
    let _ = tx.rollback().await;

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let still: (i64,) =
        sqlx::query_as("SELECT total_minor FROM finance_invoice WHERE public_id = $1")
            .bind(inv_public)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(still.0, original_total);
}

#[tokio::test]
async fn issued_invoice_delete_fails_at_db() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-immut-del").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Delete Co").await;
    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "Keep",
            "quantity": 1,
            "unit_price_minor": 1000,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_public = issued["id"].as_str().unwrap();

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let err = sqlx::query("DELETE FROM finance_invoice WHERE org_id = $1 AND public_id = $2")
        .bind(seeded.org.as_uuid())
        .bind(inv_public)
        .execute(&mut *tx)
        .await;
    assert!(
        err.is_err(),
        "DELETE of issued invoice must fail at DB trigger"
    );
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("immutability") || msg.contains("cannot delete"),
        "unexpected error: {msg}"
    );
    let _ = tx.rollback().await;
}

#[tokio::test]
async fn invoice_numbering_unique_under_concurrent_issue() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-concurrent").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = Arc::new(finance_app(seeded.pool.clone(), seeded.ring.clone()));
    let customer_id = project_customer(&app, &seeded, "Concurrent Co").await;
    let token = seeded.owner_token.clone();

    // Create several drafts first, then issue concurrently.
    let mut draft_ids = Vec::new();
    for i in 0..6 {
        let (status, draft) = call(
            &app,
            "POST",
            "/api/v1/finance/invoices",
            &token,
            Some(json!({
                "customer_id": customer_id,
                "currency": "USD",
                "lines": [{
                    "description": format!("Line {i}"),
                    "quantity": 1,
                    "unit_price_minor": 1000 + i,
                    "discount_minor": 0,
                    "tax_rate_bps": 0
                }]
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{draft:?}");
        draft_ids.push(draft["id"].as_str().unwrap().to_string());
    }

    let mut handles = Vec::new();
    for id in draft_ids {
        let app = Arc::clone(&app);
        let token = token.clone();
        handles.push(tokio::spawn(async move {
            let (status, issued) = call(
                &app,
                "POST",
                &format!("/api/v1/finance/invoices/{id}/issue"),
                &token,
                Some(json!({})),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{issued:?}");
            issued["invoice_number"]
                .as_str()
                .unwrap()
                .to_string()
        }));
    }

    let mut numbers = HashSet::new();
    for h in handles {
        let n = h.await.unwrap();
        assert!(numbers.insert(n.clone()), "duplicate invoice_number: {n}");
    }
    assert_eq!(numbers.len(), 6);
}

#[tokio::test]
async fn payment_webhook_idempotent_on_replayed_event_id() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-wh-idem").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Webhook Co").await;
    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "WH",
            "quantity": 1,
            "unit_price_minor": 5000,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_id = issued["id"].as_str().unwrap();

    let event_id = format!("evt_{}", new_uuid_v7());
    let fixture = json!({
        "id": event_id,
        "type": "payment_intent.succeeded",
        "created": 1_700_000_000,
        "data": {
            "object": {
                "id": format!("pi_{}", new_uuid_v7()),
                "amount": 2000,
                "currency": "usd",
                "customer_id": customer_id,
                "invoice_id": inv_id,
                "status": "succeeded"
            }
        }
    });

    let (status1, ack1) = call_with_headers(
        &app,
        "POST",
        "/api/v1/finance/webhooks/stripe",
        Some(&seeded.owner_token),
        &[("x-companyos-org-id", &seeded.org_public)],
        Some(fixture.clone()),
    )
    .await;
    assert_eq!(status1, StatusCode::OK, "{ack1:?}");
    assert_eq!(ack1["received"], true);
    assert_eq!(ack1["duplicate"], false);
    assert!(ack1["payment_id"].as_str().is_some());

    let (status2, ack2) = call_with_headers(
        &app,
        "POST",
        "/api/v1/finance/webhooks/stripe",
        Some(&seeded.owner_token),
        &[("x-companyos-org-id", &seeded.org_public)],
        Some(fixture),
    )
    .await;
    assert_eq!(status2, StatusCode::OK, "{ack2:?}");
    assert_eq!(ack2["received"], true);
    assert_eq!(ack2["duplicate"], true);

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM finance_payment WHERE org_id = $1 AND provider_event_id = $2",
    )
    .bind(seeded.org.as_uuid())
    .bind(&event_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count.0, 1, "replayed webhook must not create a second payment");
}

#[tokio::test]
async fn payment_webhook_out_of_order_second_event_still_works() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-wh-ooo").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "OOO Co").await;
    let issued = create_and_issue_invoice(
        &app,
        &seeded.owner_token,
        &customer_id,
        json!([{
            "description": "OOO",
            "quantity": 1,
            "unit_price_minor": 9000,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_id = issued["id"].as_str().unwrap();

    let evt_a = format!("evt_a_{}", new_uuid_v7());
    let evt_b = format!("evt_b_{}", new_uuid_v7());

    // Process "later" event first (out of order), then an earlier distinct event.
    for (event_id, amount) in [(&evt_b, 3000i64), (&evt_a, 2000i64)] {
        let (status, ack) = call_with_headers(
            &app,
            "POST",
            "/api/v1/finance/webhooks/stripe",
            Some(&seeded.owner_token),
            &[("x-companyos-org-id", &seeded.org_public)],
            Some(json!({
                "id": event_id,
                "type": "payment_intent.succeeded",
                "created": 1_700_000_000,
                "data": {
                    "object": {
                        "id": format!("pi_{}", new_uuid_v7()),
                        "amount": amount,
                        "currency": "usd",
                        "customer_id": customer_id,
                        "invoice_id": inv_id,
                        "status": "succeeded"
                    }
                }
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{ack:?}");
        assert_eq!(ack["duplicate"], false, "{ack:?}");
        assert!(ack["payment_id"].as_str().is_some(), "{ack:?}");
    }

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM finance_payment WHERE org_id = $1 AND provider = 'stripe'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count.0, 2);
}

#[tokio::test]
async fn quote_to_invoice_copies_lines_via_from_quote_snapshot() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-from-quote").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = PublicId::generate(IdKind::Customer).as_str();
    let quote_id = PublicId::generate(IdKind::Quote).as_str();

    let (status, inv) = call(
        &app,
        "POST",
        "/api/v1/finance/invoices/from-quote",
        &seeded.owner_token,
        Some(json!({
            "quote_id": quote_id,
            "customer_id": customer_id,
            "customer_name": "Snapshot Customer",
            "currency": "USD",
            "notes": "from accepted quote",
            "lines": [
                {
                    "description": "Alpha",
                    "quantity": 2,
                    "unit_price_minor": 1500,
                    "discount_minor": 0,
                    "tax_rate_bps": 0
                },
                {
                    "description": "Beta",
                    "quantity": 1,
                    "unit_price_minor": 2500,
                    "discount_minor": 100,
                    "tax_rate_bps": 1000
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{inv:?}");
    assert_eq!(inv["status"], "draft");
    assert_eq!(inv["source_quote_id"], quote_id);
    assert_eq!(inv["customer_id"], customer_id);
    let lines = inv["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["description"], "Alpha");
    assert_eq!(lines[0]["quantity"], 2);
    assert_eq!(lines[0]["unit_price_minor"], 1500);
    assert_eq!(lines[1]["description"], "Beta");
    // 2500 - 100 + tax(2400*10%=240) = 2640
    assert_eq!(lines[1]["line_total_minor"], 2640);
    assert_eq!(inv["total_minor"], 1500 * 2 + 2640);
}

#[tokio::test]
async fn tenant_isolation_org_a_cannot_see_org_b_invoice() {
    let Some(a) = seed_org_with_tokens("fin-phase15-tenant-a").await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org_with_tokens("fin-phase15-tenant-b").await else {
        return;
    };
    let app_a = finance_app(a.pool.clone(), a.ring.clone());
    let app_b = finance_app(b.pool.clone(), b.ring.clone());

    let cust_b = project_customer(&app_b, &b, "Org B Customer").await;
    let issued_b = create_and_issue_invoice(
        &app_b,
        &b.owner_token,
        &cust_b,
        json!([{
            "description": "Secret",
            "quantity": 1,
            "unit_price_minor": 7777,
            "discount_minor": 0,
            "tax_rate_bps": 0
        }]),
    )
    .await;
    let inv_b = issued_b["id"].as_str().unwrap().to_string();

    let (status, _) = call(
        &app_a,
        "GET",
        &format!("/api/v1/finance/invoices/{inv_b}"),
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, list_a) = call(
        &app_a,
        "GET",
        "/api/v1/finance/invoices",
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list_a["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&inv_b.as_str()));

    let mut tx = a.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, a.org).await.unwrap();
    let foreign: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM finance_invoice WHERE public_id = $1")
            .bind(&inv_b)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert!(
        foreign.is_empty(),
        "RLS must hide org B finance_invoice rows from org A"
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn authz_deny_member_cannot_issue_invoice() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-authz").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Authz Co").await;

    let (status, draft) = call(
        &app,
        "POST",
        "/api/v1/finance/invoices",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "lines": [{
                "description": "Restricted",
                "quantity": 1,
                "unit_price_minor": 1000,
                "discount_minor": 0,
                "tax_rate_bps": 0
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft:?}");
    let inv_id = draft["id"].as_str().unwrap();

    let (status, denied) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/invoices/{inv_id}/issue"),
        &seeded.member_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied:?}");

    // Owner can still issue.
    let (status, issued) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/invoices/{inv_id}/issue"),
        &seeded.owner_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued:?}");
}

#[tokio::test]
async fn rounding_half_up_tax_totals_consistent() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-round").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Rounding Co").await;

    // 101 * 5.5% = 5.555 → 6 half-up (matches invoice_math unit test).
    let (status, inv) = call(
        &app,
        "POST",
        "/api/v1/finance/invoices",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "lines": [
                {
                    "description": "Half-up",
                    "quantity": 1,
                    "unit_price_minor": 101,
                    "discount_minor": 0,
                    "tax_rate_bps": 550
                },
                {
                    "description": "Exact",
                    "quantity": 2,
                    "unit_price_minor": 1999,
                    "discount_minor": 0,
                    "tax_rate_bps": 875
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{inv:?}");

    let lines = inv["lines"].as_array().unwrap();
    assert_eq!(lines[0]["tax_minor"], 6);
    assert_eq!(lines[0]["line_total_minor"], 107);

    let mut sum_line_totals: i64 = 0;
    for line in lines {
        let unit_price = line["unit_price_minor"].as_i64().unwrap();
        let quantity = line["quantity"].as_i64().unwrap();
        let discount = line["discount_minor"].as_i64().unwrap();
        let tax = line["tax_minor"].as_i64().unwrap();
        let total = line["line_total_minor"].as_i64().unwrap();
        assert_eq!(
            quantity * unit_price - discount + tax,
            total,
            "line identity: {line:?}"
        );
        sum_line_totals += total;
    }

    let doc_subtotal = inv["subtotal_minor"].as_i64().unwrap();
    let doc_discount = inv["discount_minor"].as_i64().unwrap();
    let doc_tax = inv["tax_minor"].as_i64().unwrap();
    let doc_total = inv["total_minor"].as_i64().unwrap();
    assert_eq!(sum_line_totals, doc_total);
    assert_eq!(doc_subtotal - doc_discount + doc_tax, doc_total);
}

#[tokio::test]
async fn expense_above_approval_limit_is_pending_approval() {
    let Some(seeded) = seed_org_with_tokens("fin-phase15-expense-limit").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());

    // Cap the finance role so expenses above the limit need approval.
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    sqlx::query(
        r#"
        UPDATE org_role
        SET approval_limit_amount_minor = 10_000, approval_limit_currency = 'USD'
        WHERE org_id = $1 AND system_key = 'finance'
        "#,
    )
    .bind(seeded.org.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, expense) = call(
        &app,
        "POST",
        "/api/v1/finance/expenses",
        &seeded.finance_token,
        Some(json!({
            "currency": "USD",
            "amount_minor": 25_000,
            "description": "Needs approval",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{expense:?}");
    assert_eq!(expense["status"], "pending_approval");

    // Under the limit → auto-post.
    let (status, small) = call(
        &app,
        "POST",
        "/api/v1/finance/expenses",
        &seeded.finance_token,
        Some(json!({
            "currency": "USD",
            "amount_minor": 5_000,
            "description": "Petty cash",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{small:?}");
    assert_eq!(small["status"], "posted");
}
