//! Phase 3.5 — Finance depth: tax validity windows, dunning profiles,
//! multi-entity isolation, authz deny, list_invoices batch lines.
//! Requires TEST_DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_finance::state::AppState as FinanceAppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_events::{Context, EventEnvelope};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use companyos_testkit::test_database_url;
use companyos_workflow_host::catalogue::invoice_dunning::{self, Status as DunningStatus};
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
    member_token: String,
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

async fn seed_org(secret: &str) -> Option<Seeded> {
    let pool = pool().await?;
    let _guard = SEED_LOCK.lock().await;
    let ring = KeyRing::from_secret(secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    let app = core_app(pool.clone(), ring.clone());

    let owner_email = format!("owner-35-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "fin35-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner35",
                        "org_name": "Finance Phase35 Test Co"
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
    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
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
        member_token: member_issued.access_token,
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
        .header("x-request-id", format!("fin35-{}", new_uuid_v7()));
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
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::String(
            String::from_utf8_lossy(&bytes).into_owned(),
        ))
    };
    (status, val)
}

async fn project_customer(app: &Router, seeded: &Seeded, name: &str) -> String {
    let customer_id = PublicId::generate(IdKind::Customer).as_str();
    let envelope = EventEnvelope::new(
        seeded.org,
        Context::Sales,
        "customer",
        "created",
        1,
        Actor::human(Uuid::nil()),
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

#[test]
fn invoice_dunning_custom_profile_unit() {
    let mut s = invoice_dunning::State::start("inv_x", vec![1, 2]);
    assert!(s.signal_timer());
    assert!(s.signal_timer());
    assert!(!s.signal_timer());
    assert_eq!(s.reminders_sent, 2);
    assert_eq!(s.status, DunningStatus::FinalNoticeSent);

    let offsets = invoice_dunning::State::offsets_from_steps([-3, 3, 7, 14]);
    assert_eq!(offsets, vec![-3, 3, 7, 14]);
}

#[tokio::test]
async fn tax_validity_window_preserves_prior_rate() {
    let Some(seeded) = seed_org("fin-phase35-tax").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Tax Co").await;

    let (status, group) = call(
        &app,
        "POST",
        "/api/v1/finance/tax/groups",
        &seeded.owner_token,
        Some(json!({ "name": "VAT", "description": "Standard" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{group:?}");
    let group_id = group["id"].as_str().unwrap();

    let (status, rate1) = call(
        &app,
        "POST",
        "/api/v1/finance/tax/rates",
        &seeded.owner_token,
        Some(json!({
            "name": "VAT 10%",
            "rate_bps": 1000,
            "valid_from": "2026-01-01",
            "tax_group_id": group_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rate1:?}");
    let rate1_id = rate1["id"].as_str().unwrap().to_string();

    let (status, inv) = call(
        &app,
        "POST",
        "/api/v1/finance/invoices",
        &seeded.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "USD",
            "lines": [{
                "description": "Widget",
                "quantity": 1,
                "unit_price_minor": 10_000,
                "discount_minor": 0,
                "tax_rate_bps": 1000,
                "tax_group_id": group_id
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{inv:?}");
    let inv_id = inv["id"].as_str().unwrap();

    let (status, issued) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/invoices/{inv_id}/issue"),
        &seeded.owner_token,
        Some(json!({ "issue_date": "2026-01-15" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued:?}");
    assert_eq!(issued["lines"][0]["tax_rate_bps"], 1000);

    let (status, rate2) = call(
        &app,
        "POST",
        "/api/v1/finance/tax/rates",
        &seeded.owner_token,
        Some(json!({
            "name": "VAT 20%",
            "rate_bps": 2000,
            "valid_from": "2026-02-01",
            "tax_group_id": group_id,
            "supersedes_id": rate1_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rate2:?}");
    assert_eq!(rate2["rate_bps"], 2000);

    let (status, resolved) = call(
        &app,
        "GET",
        &format!("/api/v1/finance/tax/resolve?group_id={group_id}&as_of=2026-01-15"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resolved:?}");
    assert_eq!(
        resolved["rate_bps"], 1000,
        "as_of D1 must still resolve the old rate"
    );

    let (status, resolved2) = call(
        &app,
        "GET",
        &format!("/api/v1/finance/tax/resolve?group_id={group_id}&as_of=2026-02-01"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resolved2:?}");
    assert_eq!(resolved2["rate_bps"], 2000);
}

#[tokio::test]
async fn dunning_profile_changes_schedule() {
    let Some(seeded) = seed_org("fin-phase35-dunning").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Dunning Co").await;

    let (status, profile) = call(
        &app,
        "POST",
        "/api/v1/finance/dunning/profiles",
        &seeded.owner_token,
        Some(json!({
            "name": "Aggressive",
            "steps": [
                {"offset_days": 1, "channel": "email", "label": "r1"},
                {"offset_days": 2, "channel": "email", "label": "r2"}
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{profile:?}");
    let profile_id = profile["id"].as_str().unwrap();

    let (status, set) = call(
        &app,
        "POST",
        &format!("/api/v1/finance/customers/{customer_id}/dunning-profile"),
        &seeded.owner_token,
        Some(json!({ "profile_id": profile_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set:?}");

    let (status, schedule) = call(
        &app,
        "GET",
        &format!("/api/v1/finance/dunning/schedule?customer_id={customer_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{schedule:?}");
    assert_eq!(
        schedule["schedule_offsets_days"],
        json!([1, 2]),
        "{schedule:?}"
    );

    let offsets: Vec<i32> = schedule["schedule_offsets_days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    let mut wf = invoice_dunning::State::start("inv_from_profile", offsets);
    assert!(wf.signal_timer());
    assert!(wf.signal_timer());
    assert!(!wf.signal_timer());
}

#[tokio::test]
async fn entity_isolation_and_filter() {
    let Some(a) = seed_org("fin-phase35-ent-a").await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org("fin-phase35-ent-b").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app_a = finance_app(a.pool.clone(), a.ring.clone());
    let app_b = finance_app(b.pool.clone(), b.ring.clone());

    let (status, ent) = call(
        &app_a,
        "POST",
        "/api/v1/finance/entities",
        &a.owner_token,
        Some(json!({
            "name": "EU Sub",
            "code": "EU",
            "currency": "EUR",
            "is_default": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{ent:?}");
    let entity_id = ent["id"].as_str().unwrap().to_string();

    let customer_id = project_customer(&app_a, &a, "Entity Co").await;
    let (status, inv) = call(
        &app_a,
        "POST",
        "/api/v1/finance/invoices",
        &a.owner_token,
        Some(json!({
            "customer_id": customer_id,
            "currency": "EUR",
            "entity_id": entity_id,
            "lines": [{
                "description": "EU only",
                "quantity": 1,
                "unit_price_minor": 5000,
                "tax_rate_bps": 0
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{inv:?}");
    assert_eq!(inv["entity_id"].as_str().unwrap(), entity_id);
    let inv_id = inv["id"].as_str().unwrap().to_string();

    // Org B cannot see org A's invoice via list or RLS.
    let (status, list_b) = call(
        &app_b,
        "GET",
        "/api/v1/finance/invoices",
        &b.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list_b["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(!ids.contains(&inv_id.as_str()));

    let mut tx = b.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, b.org).await.unwrap();
    let foreign: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM finance_invoice WHERE public_id = $1")
            .bind(&inv_id)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert!(foreign.is_empty());
    tx.commit().await.unwrap();

    // Filter by entity_id within org A.
    let (status, filtered) = call(
        &app_a,
        "GET",
        &format!("/api/v1/finance/invoices?entity_id={entity_id}"),
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filtered:?}");
    let filtered_ids: Vec<&str> = filtered["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(filtered_ids.contains(&inv_id.as_str()));
}

#[tokio::test]
async fn member_denied_dunning_manage() {
    let Some(seeded) = seed_org("fin-phase35-authz").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, denied) = call(
        &app,
        "POST",
        "/api/v1/finance/dunning/profiles",
        &seeded.member_token,
        Some(json!({
            "name": "Nope",
            "steps": [{"offset_days": 3, "channel": "email", "label": "r1"}]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied:?}");

    // Member can still read (default profile seeded on list).
    let (status, listed) = call(
        &app,
        "GET",
        "/api/v1/finance/dunning/profiles",
        &seeded.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed:?}");
}

#[tokio::test]
async fn list_invoices_batch_lines_totals_match() {
    let Some(seeded) = seed_org("fin-phase35-batch").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = finance_app(seeded.pool.clone(), seeded.ring.clone());
    let customer_id = project_customer(&app, &seeded, "Batch Co").await;

    let mut expected_totals = Vec::new();
    for i in 0..3 {
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
                        "description": format!("Line A {i}"),
                        "quantity": 1,
                        "unit_price_minor": 1000 * (i + 1),
                        "tax_rate_bps": 0
                    },
                    {
                        "description": format!("Line B {i}"),
                        "quantity": 2,
                        "unit_price_minor": 500,
                        "tax_rate_bps": 0
                    }
                ]
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{inv:?}");
        expected_totals.push((
            inv["id"].as_str().unwrap().to_string(),
            inv["total_minor"].as_i64().unwrap(),
            inv["lines"].as_array().unwrap().len(),
        ));
    }

    // list_invoices: before=1+N line queries, after=2 (invoices + batch lines).
    // Instrumented via RedTimer key `{org}:list_invoices`.
    let (status, list) = call(
        &app,
        "GET",
        "/api/v1/finance/invoices",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list:?}");
    let items = list["items"].as_array().unwrap();
    assert!(items.len() >= 3);

    for (id, total, line_count) in &expected_totals {
        let found = items.iter().find(|i| i["id"] == *id).expect("invoice in list");
        assert_eq!(found["total_minor"].as_i64().unwrap(), *total);
        assert_eq!(found["lines"].as_array().unwrap().len(), *line_count);
    }

    let org_public = seeded.org.to_public().as_str();
    let snap = companyos_telemetry::global_red_meter()
        .get(&format!("{org_public}:list_invoices"))
        .expect("RedTimer should have recorded list_invoices");
    assert!(snap.requests >= 1);
}
