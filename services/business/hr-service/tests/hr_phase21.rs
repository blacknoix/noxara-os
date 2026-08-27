//! Phase 2.1 People / HR integration tests (DoD).
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_hr::crypto::FieldEncryptor;
use companyos_hr::state::AppState as HrAppState;
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
        companyos_hr::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn hr_app(pool: PgPool, ring: KeyRing) -> Router {
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [9u8; 32]);
    let enc = FieldEncryptor::from_base64(&key, "test").expect("encryptor");
    companyos_hr::build_router(HrAppState::new(pool, ring, enc))
}

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    #[allow(dead_code)]
    org_public: String,
    owner_token: String,
    member_token: String,
    finance_token: String,
    owner_id: Uuid,
    member_user_id: Uuid,
    #[allow(dead_code)]
    finance_user_id: Uuid,
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
                .header("x-request-id", "hr-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "HR Phase21 Test Co"
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
    let finance_user_id = insert_member_with_role(&pool, org, "finance", "Finance Rep").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (member_mem_id, member_policy) =
        membership_role_and_policy(&pool, org, member_user_id).await;
    let (finance_mem_id, finance_policy) =
        membership_role_and_policy(&pool, org, finance_user_id).await;

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
    tx.commit().await.unwrap();

    Some(Seeded {
        pool,
        ring,
        org,
        org_public,
        owner_token: owner_issued.access_token,
        member_token: member_issued.access_token,
        finance_token: finance_issued.access_token,
        owner_id,
        member_user_id,
        finance_user_id,
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
        .header("x-request-id", "hr-test");
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
        serde_json::from_slice(&bytes).unwrap_or(Value::String(
            String::from_utf8_lossy(&bytes).to_string(),
        ))
    };
    (status, val)
}

#[tokio::test]
async fn member_denied_sensitive_finance_allowed() {
    let Some(seed) = seed_org_with_tokens("hr-sensitive-secret").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());

    let (st, created) = call(
        app.clone(),
        "POST",
        "/api/v1/people/employees",
        &seed.owner_token,
        Some(json!({
            "display_name": "Ada Lovelace",
            "title": "Engineer",
            "government_id": "GOV-SECRET-999",
            "bank_details": "ACCT-SECRET",
            "tax_id": "TAX-SECRET"
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{created}");
    let emp_id = created["id"].as_str().unwrap();

    // Add compensation
    let (st, _) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/employees/{emp_id}/compensation"),
        &seed.owner_token,
        Some(json!({
            "label": "Base",
            "amount_minor": 12000000,
            "currency": "USD",
            "effective_from": "2026-01-01"
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);

    // Member can list directory but never sees restricted fields
    let (st, list) = call(
        app.clone(),
        "GET",
        "/api/v1/people/employees",
        &seed.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{list}");
    let item = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == emp_id)
        .expect("employee in list");
    assert!(item.get("government_id").is_none() || item["government_id"].is_null());
    assert!(item.get("bank_details").is_none() || item["bank_details"].is_null());
    assert!(item.get("tax_id").is_none() || item["tax_id"].is_null());
    assert!(item.get("amount_minor").is_none());

    // Member cannot read compensation
    let (st, body) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/employees/{emp_id}/compensation"),
        &seed.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{body}");

    // Member detail must not include decrypted restricted fields
    let (st, detail) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/employees/{emp_id}"),
        &seed.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{detail}");
    assert!(
        detail.get("government_id").is_none()
            || detail["government_id"].is_null()
            || detail["government_id"] == ""
    );

    // Finance can read sensitive
    let (st, fin_detail) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/employees/{emp_id}"),
        &seed.finance_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{fin_detail}");
    assert_eq!(fin_detail["government_id"], "GOV-SECRET-999");

    let (st, comps) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/employees/{emp_id}/compensation"),
        &seed.finance_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{comps}");
    assert_eq!(comps["items"][0]["amount_minor"], 12000000);
    assert_eq!(comps["items"][0]["currency"], "USD");
}

#[tokio::test]
async fn onboarding_creates_tasks_and_compensates_on_fail_after() {
    let Some(seed) = seed_org_with_tokens("hr-onboard-secret").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());

    let (st, ok) = call(
        app.clone(),
        "POST",
        "/api/v1/people/employees/onboard",
        &seed.owner_token,
        Some(json!({
            "display_name": "New Hire",
            "work_email": "hire@test.local",
            "title": "Analyst",
            "user_id": PublicId::new(IdKind::User, seed.member_user_id).as_str(),
            "role": "member",
            "asset_labels": ["Laptop"],
            "document_titles": ["Offer letter"],
            "task_titles": ["Setup email"]
        })),
        &[("idempotency-key", &format!("onboard-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{ok}");
    assert!(ok["workflow_id"]
        .as_str()
        .unwrap()
        .contains("EmployeeOnboarding"));
    assert!(!ok["tasks"].as_array().unwrap().is_empty());

    // Injected failure compensation via API
    let (st, fail) = call(
        app.clone(),
        "POST",
        "/api/v1/people/employees/onboard",
        &seed.owner_token,
        Some(json!({
            "display_name": "Fail Hire",
            "asset_labels": ["Badge"],
            "fail_after": "allocate_assets"
        })),
        &[("idempotency-key", &format!("onboard-fail-{}", new_uuid_v7()))],
    )
    .await;
    assert!(
        st == StatusCode::CONFLICT || st.is_server_error() || st == StatusCode::UNPROCESSABLE_ENTITY,
        "expected compensation failure status, got {st}: {fail}"
    );
}

#[tokio::test]
async fn offboarding_revokes_access_and_last_owner_blocked() {
    let Some(seed) = seed_org_with_tokens("hr-offboard-secret").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());

    // Create employee linked to member
    let (st, emp) = call(
        app.clone(),
        "POST",
        "/api/v1/people/employees",
        &seed.owner_token,
        Some(json!({
            "display_name": "Leaving Soon",
            "user_id": PublicId::new(IdKind::User, seed.member_user_id).as_str(),
            "status": "active"
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{emp}");
    let emp_id = emp["id"].as_str().unwrap().to_string();

    let (st, off) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/employees/{emp_id}/offboard"),
        &seed.owner_token,
        Some(json!({ "end_date": "2026-08-27", "reason": "resignation" })),
        &[("idempotency-key", &format!("off-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{off}");
    assert!(off["checklist"].as_array().unwrap().iter().all(|c| c["cleared"] == true));

    let (st, audit) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/employees/{emp_id}/access-audit"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{audit}");
    assert_eq!(audit["all_cleared"], true);

    // Prove membership + sessions cleared in DB
    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let active_mem: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM membership WHERE org_id=$1 AND user_id=$2 AND status='active' AND revoked_at IS NULL",
    )
    .bind(seed.org.as_uuid())
    .bind(seed.member_user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let active_sess: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM auth_session WHERE org_id=$1 AND user_id=$2 AND revoked_at IS NULL",
    )
    .bind(seed.org.as_uuid())
    .bind(seed.member_user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(active_mem.0, 0);
    assert_eq!(active_sess.0, 0);

    // Last-owner offboard must fail
    let (st, owner_emp) = call(
        app.clone(),
        "POST",
        "/api/v1/people/employees",
        &seed.owner_token,
        Some(json!({
            "display_name": "Sole Owner Emp",
            "user_id": PublicId::new(IdKind::User, seed.owner_id).as_str(),
            "status": "active"
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{owner_emp}");
    let owner_emp_id = owner_emp["id"].as_str().unwrap();
    let (st, blocked) = call(
        app.clone(),
        "POST",
        &format!("/api/v1/people/employees/{owner_emp_id}/offboard"),
        &seed.owner_token,
        Some(json!({})),
        &[("idempotency-key", &format!("off-owner-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{blocked}");
}

#[tokio::test]
async fn tenant_isolation_employee() {
    let Some(a) = seed_org_with_tokens("hr-iso-a").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(b) = seed_org_with_tokens("hr-iso-b").await else {
        return;
    };
    let app_a = hr_app(a.pool.clone(), a.ring.clone());
    let (st, created) = call(
        app_a,
        "POST",
        "/api/v1/people/employees",
        &a.owner_token,
        Some(json!({ "display_name": "Org A Only" })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let emp_id = created["id"].as_str().unwrap();

    let app_b = hr_app(b.pool.clone(), b.ring.clone());
    let (st, body) = call(
        app_b,
        "GET",
        &format!("/api/v1/people/employees/{emp_id}"),
        &b.owner_token,
        None,
        &[],
    )
    .await;
    assert!(
        st == StatusCode::NOT_FOUND || st == StatusCode::FORBIDDEN,
        "cross-tenant leak: {st} {body}"
    );
}
