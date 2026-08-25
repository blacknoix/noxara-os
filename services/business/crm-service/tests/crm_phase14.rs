//! Phase 1.4 CRM / Sales integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).
//!
//! Pattern mirrors `services/core/tests/workspace_phase12.rs` /
//! `dashboard_phase13.rs`: register an owner via core's HTTP API (so
//! `OrgProvisioning` seeds system roles + `role_permission`), then insert
//! additional memberships directly and mint access tokens with
//! `companyos_core::auth::sessions::create_session_with_tokens`. The CRM
//! service's own `AppState` is built with the **same** `KeyRing` so tokens
//! minted "by core" verify against companyos-crm's verifier.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_crm::state::AppState as CrmAppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .ok()?;
    companyos_core::migrate(&pool).await.ok()?;
    companyos_crm::migrate(&pool).await.ok()?;
    Some(pool)
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn crm_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_crm::build_router(CrmAppState::new(pool, ring))
}

/// One org, three memberships: owner (all perms), "sales" role (can create /
/// update / win / lose deals, accept quotes — the day-to-day CRM user), and
/// "member" role (read-only on sales.* — used for the authz-deny test).
struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_token: String,
    sales_token: String,
    member_token: String,
    #[allow(dead_code)]
    owner_id: Uuid,
    #[allow(dead_code)]
    sales_user_id: Uuid,
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
    let pool = pool().await?;
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
                .header("x-request-id", "crm-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "CRM Phase14 Test Co"
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

    let sales_user_id = insert_member_with_role(&pool, org, "sales", "Sales Rep").await;
    let member_user_id = insert_member_with_role(&pool, org, "member", "Plain Member").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (sales_mem_id, sales_policy) = membership_role_and_policy(&pool, org, sales_user_id).await;
    let (member_mem_id, member_policy) = membership_role_and_policy(&pool, org, member_user_id).await;

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
    let sales_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        sales_user_id,
        &PublicId::new(IdKind::User, sales_user_id).as_str(),
        org,
        sales_mem_id,
        &["sales".into()],
        sales_policy,
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
        owner_token: owner_issued.access_token,
        sales_token: sales_issued.access_token,
        member_token: member_issued.access_token,
        owner_id,
        sales_user_id,
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
    let mut req = Request::builder().method(method).uri(uri).header(
        "authorization",
        format!("Bearer {token}"),
    );
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

async fn call_with_header(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    extra_header: (&str, &str),
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header(extra_header.0, extra_header.1);
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

#[tokio::test]
async fn idempotency_key_dedupes_deal_creation() {
    let Some(seeded) = seed_org_with_tokens("crm-phase14-idem").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());
    let key = format!("idem-{}", new_uuid_v7());

    let (status1, first) = call_with_header(
        &app,
        "POST",
        "/api/v1/sales/deals",
        &seeded.owner_token,
        ("idempotency-key", &key),
        Some(json!({ "name": "Idempotent Deal", "amount_minor": 42 })),
    )
    .await;
    assert_eq!(status1, StatusCode::CREATED, "{first:?}");

    let (status2, second) = call_with_header(
        &app,
        "POST",
        "/api/v1/sales/deals",
        &seeded.owner_token,
        ("idempotency-key", &key),
        Some(json!({ "name": "Different Name Should Be Ignored", "amount_minor": 999 })),
    )
    .await;
    assert_eq!(status2, StatusCode::CREATED, "{second:?}");
    assert_eq!(first["id"], second["id"], "same Idempotency-Key must replay the original response");

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM sales_deal WHERE org_id = $1 AND name = 'Idempotent Deal'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count.0, 1, "retried POST with the same key must not create a second deal");
}

#[tokio::test]
async fn if_match_version_mismatch_returns_conflict() {
    let Some(seeded) = seed_org_with_tokens("crm-phase14-if-match").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, customer) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Versioned Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    let (status, _) = call_with_header(
        &app,
        "PATCH",
        &format!("/api/v1/sales/customers/{customer_id}"),
        &seeded.owner_token,
        ("if-match", "99"),
        Some(json!({ "name": "Should Fail" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, updated) = call_with_header(
        &app,
        "PATCH",
        &format!("/api/v1/sales/customers/{customer_id}"),
        &seeded.owner_token,
        ("if-match", "1"),
        Some(json!({ "name": "Should Succeed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated:?}");
    assert_eq!(updated["name"], "Should Succeed");
}

#[tokio::test]
async fn deal_won_duplicate_emits_one_event() {
    let Some(seeded) = seed_org_with_tokens("crm-phase14-deal-won").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/sales/deals",
        &seeded.sales_token,
        Some(json!({ "name": "Big Deal", "amount_minor": 500_000, "currency": "USD" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created:?}");
    let deal_id = created["id"].as_str().unwrap().to_string();

    let (status1, won1) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/deals/{deal_id}/win"),
        &seeded.sales_token,
        Some(json!({ "reason": "signed contract" })),
    )
    .await;
    assert_eq!(status1, StatusCode::OK, "{won1:?}");
    assert_eq!(won1["status"], "won");

    let (status2, won2) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/deals/{deal_id}/win"),
        &seeded.sales_token,
        Some(json!({ "reason": "signed contract again" })),
    )
    .await;
    assert_eq!(status2, StatusCode::OK, "{won2:?}");
    assert_eq!(won2["status"], "won");
    // Idempotent: reason from the first call is retained, not overwritten by the second.
    assert_eq!(won2["won_reason"], "signed contract");

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM outbox_event WHERE org_id = $1 AND subject LIKE '%.sales.deal.won.v1'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count.0, 1, "deal.won.v1 must be emitted exactly once");
}

#[tokio::test]
async fn accepted_quote_edit_creates_new_version() {
    let Some(seeded) = seed_org_with_tokens("crm-phase14-quote-version").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, customer) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Quote Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    let (status, quote) = call(
        &app,
        "POST",
        "/api/v1/sales/quotes",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "lines": [
                { "description": "Widget", "quantity": 2, "unit_price_minor": 1000, "discount_minor": 0, "tax_rate_bps": 0 }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{quote:?}");
    let original_id = quote["id"].as_str().unwrap().to_string();
    assert_eq!(quote["version_number"], 1);

    let (status, accepted) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/quotes/{original_id}/accept"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted:?}");
    assert_eq!(accepted["status"], "accepted");

    let (status, forked) = call(
        &app,
        "PATCH",
        &format!("/api/v1/sales/quotes/{original_id}"),
        &seeded.owner_token,
        Some(json!({
            "notes": "customer asked for one more widget",
            "lines": [
                { "description": "Widget", "quantity": 3, "unit_price_minor": 1000, "discount_minor": 0, "tax_rate_bps": 0 }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "PATCH on an accepted quote must fork a new draft: {forked:?}");
    let new_id = forked["id"].as_str().unwrap().to_string();
    assert_ne!(new_id, original_id, "forked quote must have a new id");
    assert_eq!(forked["status"], "draft");
    assert_eq!(forked["version_number"], 2);
    assert_eq!(forked["previous_quote_id"].as_str().unwrap(), original_id);
    assert_eq!(forked["total_minor"], 3000);

    // The original accepted quote is untouched.
    let (status, original_after) = call(
        &app,
        "GET",
        &format!("/api/v1/sales/quotes/{original_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(original_after["status"], "accepted");
    assert_eq!(original_after["version_number"], 1);
    assert_eq!(original_after["total_minor"], 2000);
}

#[tokio::test]
async fn tenant_isolation_customers_and_deals() {
    let Some(a) = seed_org_with_tokens("crm-phase14-tenant-a").await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org_with_tokens("crm-phase14-tenant-a").await else {
        return;
    };
    let app_a = crm_app(a.pool.clone(), a.ring.clone());
    let app_b = crm_app(b.pool.clone(), b.ring.clone());

    let (status, cust_a) = call(
        &app_a,
        "POST",
        "/api/v1/sales/customers",
        &a.owner_token,
        Some(json!({ "name": "Org A Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{cust_a:?}");

    let (status, cust_b) = call(
        &app_b,
        "POST",
        "/api/v1/sales/customers",
        &b.owner_token,
        Some(json!({ "name": "Org B Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{cust_b:?}");
    let cust_b_id = cust_b["customer"]["id"].as_str().unwrap().to_string();

    let (status, deal_b) = call(
        &app_b,
        "POST",
        "/api/v1/sales/deals",
        &b.owner_token,
        Some(json!({ "name": "Org B Deal", "amount_minor": 100 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{deal_b:?}");
    let deal_b_id = deal_b["id"].as_str().unwrap().to_string();

    // Org A's customer list must never contain Org B's customer.
    let (status, list_a) = call(
        &app_a,
        "GET",
        "/api/v1/sales/customers",
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = list_a["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(!names.contains(&"Org B Customer"));

    // Org A must not be able to fetch Org B's customer/deal by id (RLS + org_id scoping -> 404).
    let (status, _) = call(
        &app_a,
        "GET",
        &format!("/api/v1/sales/customers/{cust_b_id}"),
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call(
        &app_a,
        "GET",
        &format!("/api/v1/sales/deals/{deal_b_id}"),
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Direct RLS check: org A's session must not be able to see org B's row.
    let mut tx = a.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, a.org).await.unwrap();
    let foreign: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM sales_customer WHERE public_id = $1")
            .bind(&cust_b_id)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert!(foreign.is_empty(), "RLS must hide org B's sales_customer rows");
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn authz_deny_deal_update_wrong_scope() {
    let Some(seeded) = seed_org_with_tokens("crm-phase14-authz-deny").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/sales/deals",
        &seeded.owner_token,
        Some(json!({ "name": "Restricted Deal", "amount_minor": 1000 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created:?}");
    let deal_id = created["id"].as_str().unwrap().to_string();

    // The "member" role can read sales.deal but not update it.
    let (status, _) = call(
        &app,
        "PATCH",
        &format!("/api/v1/sales/deals/{deal_id}"),
        &seeded.member_token,
        Some(json!({ "name": "Hijacked" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Winning requires sales.deal.win, also missing for "member".
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/deals/{deal_id}/win"),
        &seeded.member_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Sanity: the owner (with sales.deal.update) can still update it.
    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/api/v1/sales/deals/{deal_id}"),
        &seeded.owner_token,
        Some(json!({ "name": "Renamed by owner" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated:?}");
    assert_eq!(updated["name"], "Renamed by owner");
}

#[tokio::test]
async fn duplicate_detection_exact_and_near_name_recall() {
    let Some(seeded) = seed_org_with_tokens("crm-phase14-dupes").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    // Exact-email fixture: a differently-named customer holding the email
    // the probe row below will reuse.
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Beta Holdings", "email": "contact@acme.example" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Near-name fixture: a similarly-named customer with a different email.
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Acme Corp", "email": "billing@other.example" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Duplicate check via the (non-writing) CSV import preview, which runs
    // the same `find_customer_duplicates` recall used by customer creation
    // but never inserts — a clean way to probe both signals in one row
    // without tripping the `(org_id, lower(email))` uniqueness constraint.
    // The probe reuses "Beta Holdings"'s email (-> exact_email) and a name
    // similar to "Acme Corp" (-> near_name); both must be recalled.
    let csv = "name,email\nAcme Corporation,contact@acme.example\n";
    let (status, preview) = call(
        &app,
        "POST",
        "/api/v1/sales/imports/customers/preview",
        &seeded.owner_token,
        Some(json!({ "csv": csv })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview:?}");

    let rows = preview["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let duplicates = rows[0]["duplicates"].as_array().unwrap();
    assert!(
        duplicates.iter().any(|w| w["reason"] == "exact_email"),
        "expected an exact_email match, got {duplicates:?}"
    );
    assert!(
        duplicates.iter().any(|w| w["reason"] == "near_name"),
        "expected a near_name match, got {duplicates:?}"
    );
    assert_eq!(preview["exact_duplicate_count"], 1);
    assert_eq!(preview["near_duplicate_count"], 1);

    // `strict=true` on the create endpoint rejects while a (near-name)
    // duplicate exists, using a fresh email so the DB's hard email-uniqueness
    // constraint doesn't also fire.
    let (status, conflict) = call(
        &app,
        "POST",
        "/api/v1/sales/customers?strict=true",
        &seeded.owner_token,
        Some(json!({ "name": "Acme Corporation", "email": "fresh@acme.example" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict:?}");

    // Reusing an already-taken email must be a clean 409, not a raw DB 500 —
    // even when the caller didn't ask for strict checking.
    let (status, taken_email) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Someone Else", "email": "contact@acme.example" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{taken_email:?}");
}

#[tokio::test]
async fn quote_money_totals_sum() {
    let Some(seeded) = seed_org_with_tokens("crm-phase14-quote-math").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, customer) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Totals Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    // 2 * 1999 = 3998 gross, discount 0, tax 875bps -> 350 (rounded); 1 * 4999 - 500 discount, no tax; 5 * 101 gross, tax 1000bps.
    let (status, quote) = call(
        &app,
        "POST",
        "/api/v1/sales/quotes",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "lines": [
                { "description": "A", "quantity": 2, "unit_price_minor": 1999, "discount_minor": 0, "tax_rate_bps": 875 },
                { "description": "B", "quantity": 1, "unit_price_minor": 4999, "discount_minor": 500, "tax_rate_bps": 0 },
                { "description": "C", "quantity": 5, "unit_price_minor": 101, "discount_minor": 0, "tax_rate_bps": 1000 }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{quote:?}");

    let lines = quote["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 3);

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
            "line total must equal gross - discount + tax: {line:?}"
        );
        sum_line_totals += total;
    }

    let doc_subtotal = quote["subtotal_minor"].as_i64().unwrap();
    let doc_discount = quote["discount_minor"].as_i64().unwrap();
    let doc_tax = quote["tax_minor"].as_i64().unwrap();
    let doc_total = quote["total_minor"].as_i64().unwrap();

    assert_eq!(sum_line_totals, doc_total, "document total must equal the exact sum of line totals");
    assert_eq!(doc_subtotal - doc_discount + doc_tax, doc_total);

    // Re-fetch and confirm totals are stable (not re-derived/rounded again).
    let quote_id = quote["id"].as_str().unwrap();
    let (status, refetched) = call(
        &app,
        "GET",
        &format!("/api/v1/sales/quotes/{quote_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refetched["total_minor"], doc_total);
}
