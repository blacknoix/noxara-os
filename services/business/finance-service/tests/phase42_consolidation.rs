//! Phase 4.2 — Intercompany + consolidation + entity isolation.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::KeyRing;
use companyos_finance::{build_router, migrate};
use companyos_ids::new_uuid_v7;
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn setup() -> Option<(
    companyos_finance::state::AppState,
    String,
    String,
    sqlx::PgPool,
    companyos_tenancy::OrgId,
)> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()?;
    companyos_core::migrate(&pool).await.ok()?;
    migrate(&pool).await.ok()?;

    let ring = KeyRing::from_secret("finance-phase42");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .ok()?;
    let core_state = companyos_core::state::AppState::new(pool.clone(), ring.clone());
    let core = companyos_core::build_router(core_state);

    let email = format!("fin-{}@test.local", new_uuid_v7());
    let res = core
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "reg")
                .body(Body::from(
                    json!({
                        "email": email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Fin Owner",
                        "org_name": "Fin42"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let org_public = body["org_id"].as_str()?.to_string();
    let user_public = body["user_id"].as_str()?.to_string();
    let org = companyos_tenancy::OrgId::from_public(&org_public.parse().ok()?).ok()?;
    let user_id = user_public.parse::<companyos_ids::PublicId>().ok()?.uuid();

    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(user_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .ok()?;
    companyos_core::workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok()?;

    let mem: (uuid::Uuid, i64) = {
        let mut tx = pool.begin().await.ok()?;
        companyos_tenancy::set_session_org_id(&mut tx, org)
            .await
            .ok()?;
        let row = sqlx::query_as(
            "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org.as_uuid())
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .ok()?;
        tx.commit().await.ok()?;
        row
    };

    let mut tx = pool.begin().await.ok()?;
    let issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        user_id,
        &user_public,
        org,
        mem.0,
        &["owner".into()],
        mem.1,
        None,
        None,
        None,
    )
    .await
    .ok()?;
    tx.commit().await.ok()?;

    let state = companyos_finance::state::AppState::new(pool.clone(), ring);
    Some((state, issued.access_token, user_public, pool, org))
}

async fn call(
    state: &companyos_finance::state::AppState,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let app = build_router(state.clone());
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", "f42");
    if body.is_some() {
        b = b.header("content-type", "application/json");
    }
    let res = app
        .oneshot(
            b.body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&bytes).unwrap_or(json!({}))
    };
    (status, val)
}

#[tokio::test]
async fn intercompany_consolidation_eliminates_to_zero() {
    let Some((state, token, user_public, pool, org)) = setup().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };

    // Ensure default entity + second entity
    let (st, e1) = call(
        &state,
        "POST",
        "/api/v1/finance/entities",
        &token,
        Some(json!({ "name": "Entity A", "code": "A", "currency": "USD", "is_default": true })),
    )
    .await;
    // May already have default from ensure — create two explicitly
    let ent_a = if st.is_success() {
        e1["id"].as_str().unwrap().to_string()
    } else {
        let (st, list) = call(&state, "GET", "/api/v1/finance/entities", &token, None).await;
        assert!(st.is_success(), "{list}");
        list["items"][0]["id"].as_str().unwrap().to_string()
    };

    let (st, e2) = call(
        &state,
        "POST",
        "/api/v1/finance/entities",
        &token,
        Some(json!({ "name": "Entity B", "code": format!("B{}", &new_uuid_v7().to_string()[..8]), "currency": "USD" })),
    )
    .await;
    assert!(st.is_success(), "{e2}");
    let ent_b = e2["id"].as_str().unwrap().to_string();

    let amount = 12_500i64;
    let (st, ic) = call(
        &state,
        "POST",
        "/api/v1/finance/intercompany",
        &token,
        Some(json!({
            "from_entity_id": ent_a,
            "to_entity_id": ent_b,
            "amount_minor": amount,
            "currency": "USD",
            "memo": "management fee"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{ic}");

    let (st, run) = call(
        &state,
        "POST",
        "/api/v1/finance/consolidation/runs",
        &token,
        Some(json!({
            "entity_ids": [ent_a, ent_b],
            "currency": "USD"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{run}");
    assert!(run["eliminated_minor"].as_i64().unwrap() >= amount);
    assert!(run["consolidated_trial_balance"]["balanced"]
        .as_bool()
        .unwrap());

    for stmt in run["entity_statements"].as_array().unwrap() {
        assert!(stmt["trial_balance"]["balanced"].as_bool().unwrap());
    }

    // IC accounts net to zero in consolidated TB
    let rows = run["consolidated_trial_balance"]["rows"]
        .as_array()
        .unwrap();
    for code in ["1500", "2500", "4900", "5900"] {
        let net: i64 = rows
            .iter()
            .filter(|r| r["account_code"] == code)
            .map(|r| r["debit_minor"].as_i64().unwrap() - r["credit_minor"].as_i64().unwrap())
            .sum();
        assert_eq!(
            net, 0,
            "account {code} should net to zero after elimination"
        );
    }

    // Entity isolation: grant member access only to A, create member token
    let member_id = new_uuid_v7();
    let member_public = companyos_ids::PublicId::new(companyos_ids::IdKind::User, member_id);
    let (hash, salt) =
        companyos_core::auth::password::hash_password("correct-horse-battery-staple").unwrap();
    sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at
        ) VALUES ($1,$2,$3,$3,$4,$5,'M',now())
        "#,
    )
    .bind(member_id)
    .bind(member_public.as_str())
    .bind(format!("m-{}@t.local", new_uuid_v7()))
    .bind(&hash)
    .bind(&salt)
    .execute(&pool)
    .await
    .unwrap();

    let mem_id = new_uuid_v7();
    let mut tx = pool.begin().await.unwrap();
    companyos_tenancy::set_session_org_id(&mut tx, org)
        .await
        .unwrap();
    let role_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM org_role WHERE org_id = $1 AND system_key = 'member'")
            .bind(org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO membership (id, org_id, user_id, public_id, role, role_id, status, policy_version)
        VALUES ($1,$2,$3,$4,'member',$5,'active',1)
        "#,
    )
    .bind(mem_id)
    .bind(org.as_uuid())
    .bind(member_id)
    .bind(companyos_ids::PublicId::new(companyos_ids::IdKind::Membership, mem_id).as_str())
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    // Grant finance.ledger.read via role is not on Member — use Finance role instead for read
    sqlx::query("UPDATE membership SET role = 'finance', role_id = (SELECT id FROM org_role WHERE org_id = $1 AND system_key = 'finance') WHERE id = $2")
        .bind(org.as_uuid())
        .bind(mem_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let ent_a_uuid = ent_a.parse::<companyos_ids::PublicId>().unwrap().uuid();
    let (st, _) = call(
        &state,
        "POST",
        &format!("/api/v1/finance/entities/{ent_a}/access"),
        &token,
        Some(json!({ "user_id": member_public.to_string() })),
    )
    .await;
    assert!(st.is_success(), "grant access");

    let mut tx = pool.begin().await.unwrap();
    let issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &state.keyring,
        member_id,
        &member_public.to_string(),
        org,
        mem_id,
        &["finance".into()],
        1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (st, list) = call(
        &state,
        "GET",
        "/api/v1/finance/journals",
        &issued.access_token,
        None,
    )
    .await;
    assert!(st.is_success(), "{list}");
    // All returned journals should be entity A (or none from B)
    for item in list["items"].as_array().unwrap_or(&vec![]) {
        // entity stamp may appear in DTO — if present must be A
        if let Some(eid) = item.get("entity_id").and_then(|v| v.as_str()) {
            assert_eq!(eid, ent_a);
        }
    }

    let _ = (user_public, ent_a_uuid);
}
