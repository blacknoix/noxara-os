//! Phase 2.3 Payroll integration tests (DoD).
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_finance::state::AppState as FinanceAppState;
use companyos_hr::crypto::FieldEncryptor;
use companyos_hr::state::AppState as HrAppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATED: OnceLock<()> = OnceLock::new();
static SEED_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
static FINANCE_SPAWN_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

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
        companyos_hr::migrate(pool).await.ok()?;
        companyos_finance::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn hr_app(pool: PgPool, ring: KeyRing) -> Router {
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [9u8; 32]);
    let enc = FieldEncryptor::from_base64(&key, "test").expect("encryptor");
    companyos_hr::build_router(HrAppState::new(pool, ring, enc))
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
    member_token: String,
    #[allow(dead_code)]
    manager_token: String,
    #[allow(dead_code)]
    owner_id: Uuid,
    member_user_id: Uuid,
    manager_user_id: Uuid,
}

struct FinanceServer {
    #[allow(dead_code)]
    url: String,
    handle: JoinHandle<()>,
}

impl Drop for FinanceServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn spawn_finance_server(pool: PgPool, ring: KeyRing) -> FinanceServer {
    let _guard = FINANCE_SPAWN_LOCK.lock().await;
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind finance");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");
    std::env::set_var("FINANCE_SERVICE_URL", &url);
    let app = finance_app(pool, ring);
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("finance test server error: {e}");
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    FinanceServer { url, handle }
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
    let pool = pool().await?;
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
                .header("x-request-id", "hr23-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "HR Phase23 Test Co"
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

    let member_user_id = insert_member_with_role(&pool, org, "member", "Plain Member").await;
    let manager_user_id = insert_member_with_role(&pool, org, "manager", "People Manager").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (member_mem_id, member_policy) =
        membership_role_and_policy(&pool, org, member_user_id).await;
    let (manager_mem_id, manager_policy) =
        membership_role_and_policy(&pool, org, manager_user_id).await;

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
    let manager_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        manager_user_id,
        &PublicId::new(IdKind::User, manager_user_id).as_str(),
        org,
        manager_mem_id,
        &["manager".into()],
        manager_policy,
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
        member_token: member_issued.access_token,
        manager_token: manager_issued.access_token,
        owner_id,
        member_user_id,
        manager_user_id,
    })
}

async fn call(
    app: Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", "hr23-test");
    for (k, v) in extra {
        builder = builder.header(*k, *v);
    }
    let req = if let Some(b) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string()))
    };
    (status, val)
}

async fn link_employee(app: &Router, token: &str, user_id: Uuid, name: &str) -> String {
    let (st, emp) = call(
        app.clone(),
        "POST",
        "/api/v1/people/employees",
        token,
        Some(json!({
            "display_name": name,
            "user_id": PublicId::new(IdKind::User, user_id).as_str(),
            "status": "active"
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{emp}");
    emp["id"].as_str().unwrap().to_string()
}

async fn add_compensation(app: &Router, token: &str, emp_id: &str, amount_minor: i64) {
    let (st, body) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/employees/{emp_id}/compensation"),
        token,
        Some(json!({
            "label": "Base",
            "amount_minor": amount_minor,
            "currency": "USD",
            "effective_from": "2026-01-01"
        })),
        &[("idempotency-key", &format!("comp-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{body}");
}

async fn create_payroll_run(app: &Router, token: &str) -> String {
    let (st, run) = call(
        app.clone(),
        "POST",
        "/api/v1/people/payroll/runs",
        token,
        Some(json!({
            "period_start": "2026-03-01",
            "period_end": "2026-03-31",
            "currency": "USD"
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{run}");
    assert_eq!(run["status"], "draft");
    run["id"].as_str().unwrap().to_string()
}

async fn calculate_run(app: &Router, token: &str, run_id: &str) -> Value {
    let (st, run) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/payroll/runs/{run_id}/calculate"),
        token,
        None,
        &[("idempotency-key", &format!("calc-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{run}");
    assert_eq!(run["status"], "calculated");
    run
}

async fn approve_run(app: &Router, token: &str, run_id: &str) -> Value {
    let (st, run) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/payroll/runs/{run_id}/approve"),
        token,
        None,
        &[("idempotency-key", &format!("appr-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{run}");
    assert_eq!(run["status"], "approved");
    run
}

async fn pay_run(app: &Router, token: &str, run_id: &str) -> Value {
    let (st, run) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/payroll/runs/{run_id}/pay"),
        token,
        None,
        &[("idempotency-key", &format!("pay-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{run}");
    assert_eq!(run["status"], "paid");
    run
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
async fn payroll_lifecycle_calculate_approve_pay_with_finance() {
    let Some(seed) = seed_org_with_tokens("hr23-lifecycle").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let emp = link_employee(&app, &seed.owner_token, seed.member_user_id, "Pay Emp").await;
    add_compensation(&app, &seed.owner_token, &emp, 100_000).await;

    let run_id = create_payroll_run(&app, &seed.owner_token).await;
    calculate_run(&app, &seed.owner_token, &run_id).await;
    approve_run(&app, &seed.owner_token, &run_id).await;

    let _finance = spawn_finance_server(seed.pool.clone(), seed.ring.clone()).await;
    let paid = pay_run(&app, &seed.owner_token, &run_id).await;

    assert!(paid["journal_public_id"]
        .as_str()
        .unwrap()
        .starts_with("jrn_"));
    assert_journals_balanced(&seed.pool, seed.org).await;
}

#[tokio::test]
async fn approved_run_immutable_and_adjustment_creates_new_run() {
    let Some(seed) = seed_org_with_tokens("hr23-immutable").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let emp = link_employee(&app, &seed.owner_token, seed.manager_user_id, "Adj Emp").await;
    add_compensation(&app, &seed.owner_token, &emp, 50_000).await;

    let run_id = create_payroll_run(&app, &seed.owner_token).await;
    calculate_run(&app, &seed.owner_token, &run_id).await;
    approve_run(&app, &seed.owner_token, &run_id).await;

    let (st, recalc) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/payroll/runs/{run_id}/calculate"),
        &seed.owner_token,
        None,
        &[("idempotency-key", &format!("recalc-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{recalc}");

    let (st, adjusted) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/payroll/runs/{run_id}/adjust"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{adjusted}");
    assert_eq!(adjusted["status"], "draft");
    assert_eq!(adjusted["adjustment_of_run_id"], run_id);
    assert_ne!(adjusted["id"].as_str().unwrap(), run_id);
}

#[tokio::test]
async fn payslip_lines_include_calculation_basis() {
    let Some(seed) = seed_org_with_tokens("hr23-basis").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let emp = link_employee(&app, &seed.owner_token, seed.member_user_id, "Basis Emp").await;
    add_compensation(&app, &seed.owner_token, &emp, 80_000).await;

    let run_id = create_payroll_run(&app, &seed.owner_token).await;
    calculate_run(&app, &seed.owner_token, &run_id).await;

    let (st, slips) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/payroll/runs/{run_id}/payslips"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{slips}");
    let items = slips["items"].as_array().unwrap();
    assert!(!items.is_empty(), "expected payslips after calculate");

    for slip in items {
        let lines = slip["lines"].as_array().unwrap();
        assert!(!lines.is_empty(), "payslip {} has no lines", slip["id"]);
        for line in lines {
            let basis = &line["calculation_basis"];
            assert!(
                basis.is_object() && !basis.as_object().unwrap().is_empty(),
                "line {:?} missing calculation_basis",
                line["component_code"]
            );
            assert!(basis.get("method").is_some(), "basis missing method: {basis}");
        }
    }
}

#[tokio::test]
async fn unpaid_leave_reduces_gross_pay() {
    let Some(seed) = seed_org_with_tokens("hr23-unpaid").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let emp_leave =
        link_employee(&app, &seed.owner_token, seed.member_user_id, "Leave Emp").await;
    let emp_clean =
        link_employee(&app, &seed.owner_token, seed.manager_user_id, "Clean Emp").await;
    add_compensation(&app, &seed.owner_token, &emp_leave, 100_000).await;
    add_compensation(&app, &seed.owner_token, &emp_clean, 100_000).await;

    let (st, lt) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-types",
        &seed.owner_token,
        Some(json!({
            "code": "UNPD",
            "name": "Unpaid leave",
            "category": "unpaid",
            "requires_approval": false,
            "accrual_cadence": "none"
        })),
        &[("idempotency-key", &format!("lt-unpd-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{lt}");
    let leave_type_id = lt["id"].as_str().unwrap();

    let (st, lvr) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-requests",
        &seed.owner_token,
        Some(json!({
            "employee_id": emp_leave,
            "leave_type_id": leave_type_id,
            "start_date": "2026-03-10",
            "end_date": "2026-03-14",
            "start_period": "full",
            "end_period": "full",
            "submit": true
        })),
        &[("idempotency-key", &format!("lvr-unpd-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{lvr}");
    assert_eq!(lvr["status"], "approved");
    assert!(lvr["units_milli"].as_i64().unwrap() >= 4000);

    let run_id = create_payroll_run(&app, &seed.owner_token).await;
    calculate_run(&app, &seed.owner_token, &run_id).await;

    let (st, slips) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/payroll/runs/{run_id}/payslips"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{slips}");

    let leave_slip = slips["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["employee_id"] == emp_leave)
        .expect("payslip for leave employee");
    let clean_slip = slips["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["employee_id"] == emp_clean)
        .expect("payslip for clean employee");

    let leave_gross = leave_slip["gross_minor"].as_i64().unwrap();
    let clean_gross = clean_slip["gross_minor"].as_i64().unwrap();
    assert!(
        leave_gross < clean_gross,
        "unpaid leave should reduce gross: leave={leave_gross} clean={clean_gross}"
    );

    let has_unpaid_line = leave_slip["lines"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["component_code"] == "unpaid_leave");
    assert!(
        has_unpaid_line,
        "expected unpaid_leave line, lines={:?}",
        leave_slip["lines"]
    );
}

#[tokio::test]
async fn member_denied_others_payslips_and_runs() {
    let Some(seed) = seed_org_with_tokens("hr23-member").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let member_emp = link_employee(&app, &seed.owner_token, seed.member_user_id, "Member Emp").await;
    let other_emp =
        link_employee(&app, &seed.owner_token, seed.manager_user_id, "Other Emp").await;
    add_compensation(&app, &seed.owner_token, &member_emp, 60_000).await;
    add_compensation(&app, &seed.owner_token, &other_emp, 70_000).await;

    let run_id = create_payroll_run(&app, &seed.owner_token).await;
    calculate_run(&app, &seed.owner_token, &run_id).await;

    let (st, slips) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/payroll/runs/{run_id}/payslips"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{slips}");
    let other_slip_id = slips["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["employee_id"] == other_emp)
        .and_then(|s| s["id"].as_str())
        .expect("other employee payslip");

    let (st, mine) = call(
        app.clone(),
        "GET",
        "/api/v1/people/me/payslips",
        &seed.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{mine}");
    assert!(
        mine["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["employee_id"] == member_emp),
        "member should see own payslip via me/payslips"
    );

    let (st, denied_slip) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/payroll/payslips/{other_slip_id}"),
        &seed.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{denied_slip}");

    let (st, denied_runs) = call(
        app,
        "GET",
        "/api/v1/people/payroll/runs",
        &seed.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{denied_runs}");
}

#[tokio::test]
async fn payslip_read_creates_audit_entry() {
    let Some(seed) = seed_org_with_tokens("hr23-audit").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let emp = link_employee(&app, &seed.owner_token, seed.member_user_id, "Audit Emp").await;
    add_compensation(&app, &seed.owner_token, &emp, 55_000).await;

    let run_id = create_payroll_run(&app, &seed.owner_token).await;
    calculate_run(&app, &seed.owner_token, &run_id).await;

    let (st, slips) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/payroll/runs/{run_id}/payslips"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{slips}");
    let slip_id = slips["items"][0]["id"].as_str().unwrap().to_string();

    let (st, _) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/payroll/payslips/{slip_id}"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT action FROM audit_entry
        WHERE org_id = $1
          AND resource_id = $2
          AND (action LIKE '%payslip%read%' OR action = 'hr.payslip.read')
        ORDER BY created_at DESC
        "#,
    )
    .bind(seed.org.as_uuid())
    .bind(&slip_id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(
        !rows.is_empty(),
        "expected audit_entry for payslip read, got {rows:?}"
    );
}

#[tokio::test]
async fn finance_journal_balanced_and_idempotent() {
    let Some(seed) = seed_org_with_tokens("hr23-journal").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let fin = finance_app(seed.pool.clone(), seed.ring.clone());
    let source_id = new_uuid_v7();
    let journal_body = json!({
        "source_type": "payroll",
        "source_id": source_id.to_string(),
        "currency": "USD",
        "memo": "Test payroll journal",
        "lines": [
            { "account_code": "5100", "debit_minor": 100_000, "credit_minor": 0, "memo": "Wages" },
            { "account_code": "2300", "debit_minor": 0, "credit_minor": 10_000, "memo": "Deductions" },
            { "account_code": "2400", "debit_minor": 0, "credit_minor": 90_000, "memo": "Net pay" }
        ]
    });
    let idem_key = format!("journal-idem-{}", new_uuid_v7());

    let (st1, j1) = call(
        fin.clone(),
        "POST",
        "/api/v1/finance/journals",
        &seed.owner_token,
        Some(journal_body.clone()),
        &[("idempotency-key", &idem_key)],
    )
    .await;
    assert_eq!(st1, StatusCode::CREATED, "{j1}");
    let journal_id = j1["id"].as_str().unwrap().to_string();

    let (st2, j2) = call(
        fin.clone(),
        "POST",
        "/api/v1/finance/journals",
        &seed.owner_token,
        Some(journal_body.clone()),
        &[("idempotency-key", &idem_key)],
    )
    .await;
    assert_eq!(st2, StatusCode::OK, "{j2}");
    assert_eq!(j2["id"], journal_id);

    let (st3, j3) = call(
        fin,
        "POST",
        "/api/v1/finance/journals",
        &seed.owner_token,
        Some(journal_body),
        &[("idempotency-key", &format!("other-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st3, StatusCode::OK, "{j3}");
    assert_eq!(j3["id"], journal_id);

    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let count: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint FROM finance_journal_entry
        WHERE org_id = $1 AND source_type = 'payroll' AND source_id = $2
        "#,
    )
    .bind(seed.org.as_uuid())
    .bind(source_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count.0, 1, "expected exactly one journal entry for source_id");

    assert_journals_balanced(&seed.pool, seed.org).await;
}

#[tokio::test]
async fn tenant_isolation_payslip_not_visible_wrong_org() {
    let Some(a) = seed_org_with_tokens("hr23-iso-a").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(b) = seed_org_with_tokens("hr23-iso-b").await else {
        return;
    };
    let app_a = hr_app(a.pool.clone(), a.ring.clone());
    let emp = link_employee(&app_a, &a.owner_token, a.member_user_id, "Iso Emp").await;
    add_compensation(&app_a, &a.owner_token, &emp, 40_000).await;

    let run_id = create_payroll_run(&app_a, &a.owner_token).await;
    calculate_run(&app_a, &a.owner_token, &run_id).await;

    let (st, slips) = call(
        app_a.clone(),
        "GET",
        &format!("/api/v1/people/payroll/runs/{run_id}/payslips"),
        &a.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{slips}");
    let slip_id = slips["items"][0]["id"].as_str().unwrap();

    let app_b = hr_app(b.pool.clone(), b.ring.clone());
    let (st, miss) = call(
        app_b,
        "GET",
        &format!("/api/v1/people/payroll/payslips/{slip_id}"),
        &b.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{miss}");
}
