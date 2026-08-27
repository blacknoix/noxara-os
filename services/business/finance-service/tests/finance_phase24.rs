//! Phase 2.4 — CoA, periods, trial balance, bank rec, expense policy.
//! Requires TEST_DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use chrono::Datelike;
use companyos_auth_token::KeyRing;
use companyos_finance::state::AppState as FinanceAppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
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

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_token: String,
    #[allow(dead_code)]
    owner_id: Uuid,
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

async fn seed_org(secret: &str) -> Option<Seeded> {
    let pool = pool().await?;
    let _guard = SEED_LOCK.lock().await;
    let ring = KeyRing::from_secret(secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    let app = core_app(pool.clone(), ring.clone());

    let owner_email = format!("owner-24-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "fin24-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner24",
                        "org_name": "Finance Phase24 Test Co"
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

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
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
    tx.commit().await.unwrap();

    Some(Seeded {
        pool,
        ring,
        org,
        owner_token: owner_issued.access_token,
        owner_id,
    })
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call_with_headers(app, method, uri, token, body, &[]).await
}

async fn call_with_headers(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", format!("fin24-{}", new_uuid_v7()));
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
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

async fn assert_journals_balanced(pool: &PgPool, org: OrgId) {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let rows: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        r#"
        SELECT entry_id,
               COALESCE(SUM(debit_minor),0)::bigint,
               COALESCE(SUM(credit_minor),0)::bigint
        FROM finance_journal_line
        WHERE org_id = $1
        GROUP BY entry_id
        "#,
    )
    .bind(org.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for (entry_id, debit, credit) in rows {
        assert_eq!(
            debit, credit,
            "journal {entry_id} unbalanced: debit={debit} credit={credit}"
        );
    }
}

#[tokio::test]
async fn trial_balance_always_balances_after_manual_journal() {
    let Some(seeded) = seed_org("phase24-tb").await else {
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());

    // Seed CoA via accounts list (triggers ensure).
    let (st, tree) = call(
        &app,
        "GET",
        "/api/v1/finance/accounts",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(tree["roots"].as_array().unwrap().len() >= 1);

    let source_id = new_uuid_v7().to_string();
    let (st, je) = call_with_headers(
        &app,
        "POST",
        "/api/v1/finance/journals",
        &seeded.owner_token,
        Some(json!({
            "source_type": "manual",
            "source_id": source_id,
            "currency": "USD",
            "memo": "TB fixture",
            "lines": [
                {"account_code": "1000", "debit_minor": 5000, "credit_minor": 0},
                {"account_code": "4000", "debit_minor": 0, "credit_minor": 5000}
            ]
        })),
        &[("idempotency-key", "tb-j1")],
    )
    .await;
    assert!(
        st == StatusCode::CREATED || st == StatusCode::OK,
        "journal post failed: {st} {je}"
    );

    let (st, tb) = call(
        &app,
        "GET",
        "/api/v1/finance/reports/trial-balance?currency=USD",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{tb}");
    assert_eq!(tb["balanced"], true);
    assert_eq!(
        tb["total_debit_minor"].as_i64().unwrap(),
        tb["total_credit_minor"].as_i64().unwrap()
    );
    assert_journals_balanced(&seeded.pool, seeded.org).await;
}

#[tokio::test]
async fn unbalanced_manual_journal_rejected() {
    let Some(seeded) = seed_org("phase24-unbal").await else {
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let _ = call(
        &app,
        "GET",
        "/api/v1/finance/accounts",
        &seeded.owner_token,
        None,
    )
    .await;

    let (st, body) = call(
        &app,
        "POST",
        "/api/v1/finance/journals",
        &seeded.owner_token,
        Some(json!({
            "source_type": "manual",
            "currency": "USD",
            "lines": [
                {"account_code": "1000", "debit_minor": 100, "credit_minor": 0},
                {"account_code": "4000", "debit_minor": 0, "credit_minor": 50}
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "validation_failed");
}

#[tokio::test]
async fn closed_period_rejects_post_reopen_allows() {
    let Some(seeded) = seed_org("phase24-period").await else {
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let _ = call(
        &app,
        "GET",
        "/api/v1/finance/accounts",
        &seeded.owner_token,
        None,
    )
    .await;

    let today = chrono::Utc::now().date_naive();
    let start = chrono::NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let end = if today.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(today.year() + 1, 1, 1)
            .unwrap()
            .pred_opt()
            .unwrap()
    } else {
        chrono::NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1)
            .unwrap()
            .pred_opt()
            .unwrap()
    };
    let code = format!("{:04}-{:02}", today.year(), today.month());

    let (st, period) = call(
        &app,
        "POST",
        "/api/v1/finance/periods",
        &seeded.owner_token,
        Some(json!({
            "code": code,
            "name": "Current month",
            "start_date": start.to_string(),
            "end_date": end.to_string()
        })),
    )
    .await;
    // Created or already exists from ensure on prior post.
    assert!(
        st == StatusCode::CREATED || st == StatusCode::OK || st == StatusCode::CONFLICT,
        "{st} {period}"
    );

    // Ensure period exists via list.
    let (st, list) = call(
        &app,
        "GET",
        "/api/v1/finance/periods",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let items = list["items"].as_array().cloned().unwrap_or_default();
    let period_id = items
        .iter()
        .find(|p| p["code"] == code)
        .or_else(|| items.first())
        .and_then(|p| p["id"].as_str())
        .expect("period")
        .to_string();

    let (st, closed) = call_with_headers(
        &app,
        "POST",
        &format!("/api/v1/finance/periods/{period_id}/close"),
        &seeded.owner_token,
        Some(json!({})),
        &[("idempotency-key", "close-1")],
    )
    .await;
    assert!(
        st == StatusCode::OK || st == StatusCode::CREATED,
        "close: {st} {closed}"
    );
    assert_eq!(closed["status"], "closed");

    let (st, reject) = call(
        &app,
        "POST",
        "/api/v1/finance/journals",
        &seeded.owner_token,
        Some(json!({
            "source_type": "manual",
            "currency": "USD",
            "entry_date": today.to_string(),
            "lines": [
                {"account_code": "1000", "debit_minor": 1000, "credit_minor": 0},
                {"account_code": "4000", "debit_minor": 0, "credit_minor": 1000}
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{reject}");
    assert_eq!(reject["code"], "conflict");
    let detail = reject["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("closed") || detail.contains("locked"),
        "detail={detail}"
    );

    let (st, reopened) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/periods/{period_id}/reopen"),
        &seeded.owner_token,
        Some(json!({ "reason": "month-end correction for accrual" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["status"], "open");

    // Audit trail for reopen.
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let audit_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM audit_entry
        WHERE org_id = $1 AND action = 'finance.period.reopen'
        "#,
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(audit_count >= 1, "reopen must be audited");

    let (st, ok) = call_with_headers(
        &app,
        "POST",
        "/api/v1/finance/journals",
        &seeded.owner_token,
        Some(json!({
            "source_type": "manual",
            "currency": "USD",
            "entry_date": today.to_string(),
            "lines": [
                {"account_code": "1000", "debit_minor": 1000, "credit_minor": 0},
                {"account_code": "4000", "debit_minor": 0, "credit_minor": 1000}
            ]
        })),
        &[("idempotency-key", "after-reopen")],
    )
    .await;
    assert!(
        st == StatusCode::CREATED || st == StatusCode::OK,
        "post after reopen: {st} {ok}"
    );
}

#[tokio::test]
async fn bank_rec_auto_match_at_least_90_percent() {
    let Some(seeded) = seed_org("phase24-bank").await else {
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let _ = call(
        &app,
        "GET",
        "/api/v1/finance/accounts",
        &seeded.owner_token,
        None,
    )
    .await;

    // Need a customer + invoice + payments for matching, or insert payments directly.
    // Use finance payment API after creating a customer projection + invoice.
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    companyos_finance::journal::ensure_ledger_accounts(&mut tx, seeded.org.as_uuid())
        .await
        .unwrap();
    let cus_id = new_uuid_v7();
    let cus_public = PublicId::new(IdKind::Customer, cus_id).as_str();
    sqlx::query(
        r#"
        INSERT INTO finance_customer (id, org_id, public_id, sales_customer_public_id, name, currency)
        VALUES ($1,$2,$3,$4,'Bank Rec Customer','USD')
        "#,
    )
    .bind(cus_id)
    .bind(seeded.org.as_uuid())
    .bind(&cus_public)
    .bind(format!("cus_sales_{}", new_uuid_v7()))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (st, inv) = call(
        &app,
        "POST",
        "/api/v1/finance/invoices",
        &seeded.owner_token,
        Some(json!({
            "customer_id": cus_public,
            "currency": "USD",
            "lines": [{"description": "Svc", "quantity": 1, "unit_price_minor": 10_000, "tax_rate_bps": 0}]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{inv}");
    let inv_id = inv["id"].as_str().unwrap();

    let (st, issued) = call_with_headers(
        &app,
        "POST",
        &format!("/api/v1/finance/invoices/{inv_id}/issue"),
        &seeded.owner_token,
        Some(json!({})),
        &[("idempotency-key", "issue-bank")],
    )
    .await;
    assert!(
        st == StatusCode::OK || st == StatusCode::CREATED,
        "{issued}"
    );

    // Ten payments of $100.00 each — statement will include 10 matching lines + 1 noise = 90%+.
    let today = chrono::Utc::now().date_naive().to_string();
    let mut pay_refs = Vec::new();
    for i in 0..10 {
        let (st, pay) = call_with_headers(
            &app,
            "POST",
            "/api/v1/finance/payments",
            &seeded.owner_token,
            Some(json!({
                "customer_id": cus_public,
                "currency": "USD",
                "amount_minor": 1000,
                "invoice_id": inv_id,
                "received_at": format!("{today}T12:00:00Z"),
                "notes": format!("bank-fix-{i}")
            })),
            &[("idempotency-key", &format!("pay-{i}"))],
        )
        .await;
        assert!(
            st == StatusCode::CREATED || st == StatusCode::OK,
            "pay {i}: {st} {pay}"
        );
        pay_refs.push(pay["id"].as_str().unwrap().to_string());
    }

    let (st, bank) = call(
        &app,
        "POST",
        "/api/v1/finance/bank/accounts",
        &seeded.owner_token,
        Some(json!({
            "name": "Operating",
            "currency": "USD",
            "ledger_account_id": "1000"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{bank}");
    let bank_id = bank["id"].as_str().unwrap();

    let mut csv = String::from("date,amount,reference,description\n");
    for (i, pref) in pay_refs.iter().enumerate() {
        csv.push_str(&format!("{today},10.00,{pref},Payment {i}\n"));
    }
    // One unmatched noise line (still ≥90% when 10/11 match).
    csv.push_str(&format!("{today},99.99,NOISE,Unmatched noise\n"));

    let (st, imported) = call_with_headers(
        &app,
        "POST",
        &format!("/api/v1/finance/bank/accounts/{bank_id}/statements/import"),
        &seeded.owner_token,
        Some(json!({
            "csv": csv,
            "statement_date": today
        })),
        &[("idempotency-key", "stmt-1")],
    )
    .await;
    assert!(
        st == StatusCode::CREATED || st == StatusCode::OK,
        "{imported}"
    );
    let stmt_id = imported["statement"]["id"].as_str().unwrap();
    assert_eq!(imported["lines_imported"], 11);

    let (st, rec) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/bank/statements/{stmt_id}/auto-match"),
        &seeded.owner_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{rec}");
    let rate = rec["match_rate"].as_f64().unwrap();
    assert!(rate >= 0.90, "expected ≥90% auto-match, got {rate}: {rec}");
}

#[tokio::test]
async fn expense_over_policy_limit_rejects() {
    let Some(seeded) = seed_org("phase24-policy").await else {
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let _ = call(
        &app,
        "GET",
        "/api/v1/finance/accounts",
        &seeded.owner_token,
        None,
    )
    .await;

    let (st, pol) = call(
        &app,
        "PUT",
        "/api/v1/finance/expense-policies",
        &seeded.owner_token,
        Some(json!({
            "name": "Strict",
            "over_limit_action": "reject",
            "mileage_rate_minor": 6700,
            "per_diem_minor": 7500,
            "category_limits": [
                {"category_code": "meals", "max_amount_minor": 5000, "currency": "USD"}
            ]
        })),
    )
    .await;
    assert!(st == StatusCode::OK || st == StatusCode::CREATED, "{pol}");

    let (st, exp) = call(
        &app,
        "POST",
        "/api/v1/finance/expenses",
        &seeded.owner_token,
        Some(json!({
            "currency": "USD",
            "amount_minor": 12_000,
            "description": "Fancy dinner over limit",
            "category_code": "meals"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{exp}");
    assert_eq!(exp["code"], "validation_failed");
}

#[tokio::test]
async fn tenant_isolation_on_new_tables() {
    let Some(a) = seed_org("phase24-iso-a").await else {
        return;
    };
    let Some(b) = seed_org("phase24-iso-b").await else {
        return;
    };
    let app_a = finance_app(a.pool.clone(), a.ring.clone());
    let _ = call(
        &app_a,
        "GET",
        "/api/v1/finance/accounts",
        &a.owner_token,
        None,
    )
    .await;
    let (st, period) = call(
        &app_a,
        "POST",
        "/api/v1/finance/periods",
        &a.owner_token,
        Some(json!({
            "code": "2099-01",
            "name": "Far future",
            "start_date": "2099-01-01",
            "end_date": "2099-01-31"
        })),
    )
    .await;
    assert!(
        st == StatusCode::CREATED || st == StatusCode::OK,
        "{period}"
    );
    let period_id = period["id"].as_str().unwrap().to_string();

    // Org B must not see org A's period via RLS when querying with B's session.
    let mut tx = b.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, b.org).await.unwrap();
    let leaked: Option<(String,)> =
        sqlx::query_as("SELECT public_id FROM finance_fiscal_period WHERE public_id = $1")
            .bind(&period_id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert!(
        leaked.is_none(),
        "org B must not read org A fiscal period via RLS"
    );

    // Also cover bank + policy tables exist under RLS.
    for table in [
        "finance_bank_account",
        "finance_bank_statement",
        "finance_bank_statement_line",
        "finance_bank_reconciliation",
        "finance_expense_policy",
        "finance_reimbursement_batch",
        "finance_card_transaction",
    ] {
        let mut tx = a.pool.begin().await.unwrap();
        set_session_org_id(&mut tx, a.org).await.unwrap();
        let q = format!("SELECT COUNT(*) FROM {table}");
        let _: i64 = sqlx::query_scalar(&q).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
}

#[tokio::test]
async fn pnl_and_balance_sheet_readable() {
    let Some(seeded) = seed_org("phase24-reports").await else {
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let _ = call(
        &app,
        "GET",
        "/api/v1/finance/accounts",
        &seeded.owner_token,
        None,
    )
    .await;
    let (st, _) = call_with_headers(
        &app,
        "POST",
        "/api/v1/finance/journals",
        &seeded.owner_token,
        Some(json!({
            "source_type": "manual",
            "currency": "USD",
            "lines": [
                {"account_code": "1000", "debit_minor": 2000, "credit_minor": 0},
                {"account_code": "4000", "debit_minor": 0, "credit_minor": 2000}
            ]
        })),
        &[("idempotency-key", "rep-j")],
    )
    .await;
    assert!(st == StatusCode::CREATED || st == StatusCode::OK, "{st}");

    let (st, pnl) = call(
        &app,
        "GET",
        "/api/v1/finance/reports/profit-and-loss?currency=USD",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pnl}");

    let (st, bs) = call(
        &app,
        "GET",
        "/api/v1/finance/reports/balance-sheet?currency=USD",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{bs}");
}
