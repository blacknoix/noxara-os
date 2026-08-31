//! Phase 3.5 CRM depth — orders, contracts, territories.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).
//!
//! Seed pattern mirrors `crm_phase14.rs`.

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

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    #[allow(dead_code)]
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
                .header("x-request-id", "crm-p35-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "CRM Phase35 Test Co"
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
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
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
async fn order_from_accepted_quote_money_exact_and_rls() {
    let Some(seeded_a) = seed_org_with_tokens("crm-p35-order-quote-a").await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(seeded_b) = seed_org_with_tokens("crm-p35-order-quote-b").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app_a = crm_app(seeded_a.pool.clone(), seeded_a.ring.clone());
    let app_b = crm_app(seeded_b.pool.clone(), seeded_b.ring.clone());

    let (status, customer) = call(
        &app_a,
        "POST",
        "/api/v1/sales/customers",
        &seeded_a.owner_token,
        Some(json!({ "name": "Acme Quote Co" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    let (status, quote) = call(
        &app_a,
        "POST",
        "/api/v1/sales/quotes",
        &seeded_a.sales_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "lines": [
                {
                    "description": "Widget",
                    "quantity": 2,
                    "unit_price_minor": 5000,
                    "discount_minor": 1000,
                    "tax_rate_bps": 1000
                },
                {
                    "description": "Gadget",
                    "quantity": 1,
                    "unit_price_minor": 1999,
                    "discount_minor": 0,
                    "tax_rate_bps": 875
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{quote:?}");
    let quote_id = quote["id"].as_str().unwrap().to_string();
    let quote_subtotal = quote["subtotal_minor"].as_i64().unwrap();
    let quote_discount = quote["discount_minor"].as_i64().unwrap();
    let quote_tax = quote["tax_minor"].as_i64().unwrap();
    let quote_total = quote["total_minor"].as_i64().unwrap();

    let (status, accepted) = call(
        &app_a,
        "POST",
        &format!("/api/v1/sales/quotes/{quote_id}/accept"),
        &seeded_a.sales_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted:?}");
    assert_eq!(accepted["status"], "accepted");

    let (status, order) = call(
        &app_a,
        "POST",
        "/api/v1/sales/orders/from-quote",
        &seeded_a.sales_token,
        Some(json!({ "quote_id": quote_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{order:?}");
    assert!(order["id"].as_str().unwrap().starts_with("ord_"));
    assert_eq!(order["quote_id"].as_str().unwrap(), quote_id);
    assert_eq!(order["subtotal_minor"].as_i64().unwrap(), quote_subtotal);
    assert_eq!(order["discount_minor"].as_i64().unwrap(), quote_discount);
    assert_eq!(order["tax_minor"].as_i64().unwrap(), quote_tax);
    assert_eq!(order["total_minor"].as_i64().unwrap(), quote_total);
    assert_eq!(
        order["total_minor"].as_i64().unwrap(),
        order["subtotal_minor"].as_i64().unwrap()
            - order["discount_minor"].as_i64().unwrap()
            + order["tax_minor"].as_i64().unwrap()
    );

    let order_id = order["id"].as_str().unwrap().to_string();

    // Second from-quote for same quote must conflict.
    let (status, dup) = call(
        &app_a,
        "POST",
        "/api/v1/sales/orders/from-quote",
        &seeded_a.sales_token,
        Some(json!({ "quote_id": quote_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{dup:?}");

    // Org B cannot see Org A's order (RLS).
    let (status, hidden) = call(
        &app_b,
        "GET",
        &format!("/api/v1/sales/orders/{order_id}"),
        &seeded_b.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden:?}");
}

#[tokio::test]
async fn order_from_won_deal() {
    let Some(seeded) = seed_org_with_tokens("crm-p35-order-deal").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, customer) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Won Deal Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    let (status, deal) = call(
        &app,
        "POST",
        "/api/v1/sales/deals",
        &seeded.sales_token,
        Some(json!({
            "name": "Big Opportunity",
            "amount_minor": 125_000,
            "currency": "USD",
            "customer_id": customer_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{deal:?}");
    let deal_id = deal["id"].as_str().unwrap().to_string();

    let (status, won) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/deals/{deal_id}/win"),
        &seeded.sales_token,
        Some(json!({ "reason": "signed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{won:?}");

    let (status, order) = call(
        &app,
        "POST",
        "/api/v1/sales/orders/from-deal",
        &seeded.sales_token,
        Some(json!({ "deal_id": deal_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{order:?}");
    assert_eq!(order["deal_id"].as_str().unwrap(), deal_id);
    assert_eq!(order["total_minor"].as_i64().unwrap(), 125_000);
    assert_eq!(order["lines"].as_array().unwrap().len(), 1);
    assert_eq!(
        order["lines"][0]["unit_price_minor"].as_i64().unwrap(),
        125_000
    );
    assert_eq!(order["lines"][0]["quantity"].as_i64().unwrap(), 1);

    let (status, confirmed) = call(
        &app,
        "PATCH",
        &format!("/api/v1/sales/orders/{}", order["id"].as_str().unwrap()),
        &seeded.sales_token,
        Some(json!({ "status": "confirmed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirmed:?}");
    assert_eq!(confirmed["status"], "confirmed");
}

#[tokio::test]
async fn contract_publish_renewals_and_accepted_quote_immutable() {
    let Some(seeded) = seed_org_with_tokens("crm-p35-contract").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, customer) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Contract Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    // Accepted quote remains immutable (fork-on-edit).
    let (status, quote) = call(
        &app,
        "POST",
        "/api/v1/sales/quotes",
        &seeded.sales_token,
        Some(json!({
            "customer_id": customer_id,
            "lines": [{ "description": "Svc", "quantity": 1, "unit_price_minor": 1000 }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{quote:?}");
    let quote_id = quote["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/quotes/{quote_id}/accept"),
        &seeded.sales_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, forked) = call(
        &app,
        "PATCH",
        &format!("/api/v1/sales/quotes/{quote_id}"),
        &seeded.sales_token,
        Some(json!({ "notes": "change after accept" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{forked:?}");
    assert_ne!(forked["id"].as_str().unwrap(), quote_id);
    assert_eq!(forked["status"], "draft");

    // end_date within 90 days; auto_renew also qualifies for renewal pipeline.
    let (status, contract) = call(
        &app,
        "POST",
        "/api/v1/sales/contracts",
        &seeded.sales_token,
        Some(json!({
            "customer_id": customer_id,
            "title": "Annual Support",
            "term_months": 12,
            "start_date": "2026-01-01",
            "end_date": "2026-09-15",
            "value_minor": 50_000,
            "auto_renew": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{contract:?}");
    let contract_id = contract["id"].as_str().unwrap().to_string();
    assert!(contract_id.starts_with("sct_"));
    assert_eq!(contract["status"], "draft");

    let (status, published) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/contracts/{contract_id}/publish"),
        &seeded.sales_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{published:?}");
    assert_eq!(published["status"], "active");
    assert!(published["published_at"].as_str().is_some());

    let (status, patch_denied) = call(
        &app,
        "PATCH",
        &format!("/api/v1/sales/contracts/{contract_id}"),
        &seeded.sales_token,
        Some(json!({ "title": "Nope" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{patch_denied:?}");

    let (status, renewals) = call(
        &app,
        "GET",
        "/api/v1/sales/contracts/renewals?within_days=90",
        &seeded.sales_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renewals:?}");
    assert_eq!(renewals["within_days"], 90);
    let items = renewals["items"].as_array().unwrap();
    assert!(
        items.iter().any(|c| c["id"] == contract_id),
        "published auto_renew contract must appear in renewal pipeline: {renewals:?}"
    );
}

#[tokio::test]
async fn territory_assign_scoped_member_cannot_manage() {
    let Some(seeded) = seed_org_with_tokens("crm-p35-territory").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, customer) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Territory Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    let (status, territory) = call(
        &app,
        "POST",
        "/api/v1/sales/territories",
        &seeded.sales_token,
        Some(json!({ "name": "West Coast", "description": "CA/OR/WA" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{territory:?}");
    let territory_id = territory["id"].as_str().unwrap().to_string();
    assert!(territory_id.starts_with("ter_"));

    let (status, assigned) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/territories/{territory_id}/assign"),
        &seeded.sales_token,
        Some(json!({ "customer_id": customer_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{assigned:?}");
    assert_eq!(assigned["territory_id"], territory_id);
    assert_eq!(assigned["customer_id"].as_str().unwrap(), customer_id);

    // Member can read territories but cannot manage/assign.
    let (status, listed) = call(
        &app,
        "GET",
        "/api/v1/sales/territories",
        &seeded.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed:?}");

    let (status, denied) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/territories/{territory_id}/assign"),
        &seeded.member_token,
        Some(json!({ "customer_id": customer_id })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied:?}");

    let (status, create_denied) = call(
        &app,
        "POST",
        "/api/v1/sales/territories",
        &seeded.member_token,
        Some(json!({ "name": "Hijack" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{create_denied:?}");
}

#[tokio::test]
async fn member_cannot_publish_contract() {
    let Some(seeded) = seed_org_with_tokens("crm-p35-member-publish").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = crm_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, customer) = call(
        &app,
        "POST",
        "/api/v1/sales/customers",
        &seeded.owner_token,
        Some(json!({ "name": "Publish Gate Customer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer:?}");
    let customer_id = customer["customer"]["id"].as_str().unwrap().to_string();

    let (status, contract) = call(
        &app,
        "POST",
        "/api/v1/sales/contracts",
        &seeded.sales_token,
        Some(json!({
            "customer_id": customer_id,
            "title": "Draft Only",
            "value_minor": 1000
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{contract:?}");
    let contract_id = contract["id"].as_str().unwrap().to_string();

    // Member can read but cannot publish.
    let (status, read_ok) = call(
        &app,
        "GET",
        &format!("/api/v1/sales/contracts/{contract_id}"),
        &seeded.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read_ok:?}");

    let (status, publish_denied) = call(
        &app,
        "POST",
        &format!("/api/v1/sales/contracts/{contract_id}/publish"),
        &seeded.member_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{publish_denied:?}");
}
