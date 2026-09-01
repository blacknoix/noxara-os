//! Phase 4.2 — Enterprise multi-tenancy integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration, Utc};
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_core::state::AppState;
use companyos_core::{build_router, migrate, workspace};
use companyos_crypto::{CmkId, Kms, OrgDataKey};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

async fn pool() -> Option<sqlx::PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()?;
    migrate(&pool).await.ok()?;
    Some(pool)
}

async fn app_state(pool: sqlx::PgPool) -> AppState {
    let ring = KeyRing::from_secret("test-auth-secret-phase42");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    AppState::new(pool, ring)
}

struct Seeded {
    state: AppState,
    pool: sqlx::PgPool,
    org: OrgId,
    org_public: String,
    owner_token: String,
    member_token: String,
    member_membership_public: String,
    member_id: Uuid,
}

async fn seed_org() -> Option<Seeded> {
    let pool = pool().await?;
    let state = app_state(pool.clone()).await;
    let app = build_router(state.clone());

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let member_email = format!("member-{}@test.local", new_uuid_v7());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "reg-42")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Phase42 Co"
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

    workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok();

    let member_id = new_uuid_v7();
    let member_public = PublicId::new(IdKind::User, member_id).as_str();
    let (hash, salt) = password::hash_password("correct-horse-battery-staple").unwrap();
    sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at
        ) VALUES ($1,$2,$3,$4,$5,$6,'Member',now())
        "#,
    )
    .bind(member_id)
    .bind(&member_public)
    .bind(&member_email)
    .bind(member_email.to_ascii_lowercase())
    .bind(&hash)
    .bind(&salt)
    .execute(&pool)
    .await
    .unwrap();

    let mem_id = new_uuid_v7();
    let mem_public = PublicId::new(IdKind::Membership, mem_id).as_str();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let role_id: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_role WHERE org_id = $1 AND system_key = 'member'")
            .bind(org.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO membership (id, org_id, user_id, public_id, role, role_id, policy_version, status)
        VALUES ($1,$2,$3,$4,'member',$5,1,'active')
        "#,
    )
    .bind(mem_id)
    .bind(org.as_uuid())
    .bind(member_id)
    .bind(&mem_public)
    .bind(role_id.map(|r| r.0))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let owner_mem: (Uuid, i64) = {
        let mut tx = pool.begin().await.unwrap();
        set_session_org_id(&mut tx, org).await.unwrap();
        let row = sqlx::query_as(
            "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org.as_uuid())
        .bind(owner_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        row
    };
    let member_mem: (Uuid, i64) = {
        let mut tx = pool.begin().await.unwrap();
        set_session_org_id(&mut tx, org).await.unwrap();
        let row = sqlx::query_as(
            "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org.as_uuid())
        .bind(member_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        row
    };

    let mut tx = pool.begin().await.unwrap();
    let owner_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        owner_id,
        &owner_public,
        org,
        owner_mem.0,
        &["owner".into()],
        owner_mem.1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let member_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        member_id,
        &member_public,
        org,
        member_mem.0,
        &["member".into()],
        member_mem.1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    Some(Seeded {
        state,
        pool,
        org,
        org_public,
        owner_token: owner_issued.access_token,
        member_token: member_issued.access_token,
        member_membership_public: mem_public,
        member_id,
    })
}

async fn json_req(
    state: &AppState,
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
        .header("x-request-id", "t42");
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
        serde_json::from_slice(&bytes).unwrap_or(json!({ "raw": String::from_utf8_lossy(&bytes) }))
    };
    (status, val)
}

#[tokio::test]
async fn cmk_rotate_then_revoke_old_cannot_decrypt() {
    let Some(seed) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };

    let (st, body) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/cmk",
        &seed.owner_token,
        Some(json!({ "alias": "test" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let old_provider = body["provider_key_id"].as_str().unwrap().to_string();

    let wrapped =
        companyos_core::enterprise::cmk::load_wrapped_dek(&seed.state, seed.org.as_uuid(), "t")
            .await
            .unwrap();
    let dek = OrgDataKey::from_wrapped(seed.state.kms.as_ref(), &wrapped).unwrap();
    let ct = dek.encrypt_str("secret-field").unwrap();

    let (st, rot) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/cmk/rotate",
        &seed.owner_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{rot}");

    let (st, rev) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/cmk/revoke",
        &seed.owner_token,
        Some(json!({ "provider_key_id": old_provider })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{rev}");
    assert!(seed.state.kms.is_revoked(&CmkId(old_provider)));

    assert!(OrgDataKey::from_wrapped(seed.state.kms.as_ref(), &wrapped).is_err());

    let new_wrapped =
        companyos_core::enterprise::cmk::load_wrapped_dek(&seed.state, seed.org.as_uuid(), "t")
            .await
            .unwrap();
    let new_dek = OrgDataKey::from_wrapped(seed.state.kms.as_ref(), &new_wrapped).unwrap();
    assert_eq!(new_dek.decrypt_str(&ct).unwrap(), "secret-field");

    let (st, _) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/cmk/rotate",
        &seed.member_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn scim_two_idps_crud_and_deprovision() {
    let Some(seed) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };

    let (st, tok_a) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/scim/tokens",
        &seed.owner_token,
        Some(json!({ "name": "IdP A", "idp_label": "idp-a" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{tok_a}");
    let token_a = tok_a["token"].as_str().unwrap().to_string();

    let (st, tok_b) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/scim/tokens",
        &seed.owner_token,
        Some(json!({ "name": "IdP B", "idp_label": "idp-b" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{tok_b}");
    let token_b = tok_b["token"].as_str().unwrap().to_string();

    async fn scim(
        state: &AppState,
        token: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let app = build_router(state.clone());
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .header("x-request-id", "scim");
        if body.is_some() {
            b = b.header("content-type", "application/scim+json");
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

    let email_a = format!("alice-{}@ex.com", new_uuid_v7());
    let (st, u) = scim(
        &seed.state,
        &token_a,
        "POST",
        "/api/v1/scim/v2/Users",
        Some(json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
            "userName": email_a,
            "externalId": "alice-a",
            "emails": [{ "value": email_a, "primary": true }],
            "active": true
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{u}");

    let (st, g) = scim(
        &seed.state,
        &token_b,
        "POST",
        "/api/v1/scim/v2/Groups",
        Some(json!({
            "displayName": format!("Engineering-{}", new_uuid_v7()),
            "externalId": "eng-b",
            "members": []
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{g}");

    let (st, _) = scim(
        &seed.state,
        &token_a,
        "DELETE",
        "/api/v1/scim/v2/Users/alice-a",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);

    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let status: String = sqlx::query_scalar(
        r#"
        SELECT m.status FROM membership m
        JOIN scim_external_identity s ON s.user_id = m.user_id AND s.org_id = m.org_id
        WHERE s.external_id = 'alice-a' AND m.org_id = $1
        "#,
    )
    .bind(seed.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(status, "revoked");
}

#[tokio::test]
async fn inheritance_delegation_expiry_and_deny() {
    let Some(seed) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };

    let parent_id = new_uuid_v7();
    let parent_public = PublicId::new(IdKind::Team, parent_id);
    let child_id = new_uuid_v7();
    let child_public = PublicId::new(IdKind::Team, child_id);
    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    sqlx::query("INSERT INTO team (id, org_id, public_id, name) VALUES ($1,$2,$3,$4)")
        .bind(parent_id)
        .bind(seed.org.as_uuid())
        .bind(parent_public.as_str())
        .bind(format!("Parent-{}", new_uuid_v7()))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO team (id, org_id, public_id, name, parent_team_id) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(child_id)
    .bind(seed.org.as_uuid())
    .bind(child_public.as_str())
    .bind(format!("Child-{}", new_uuid_v7()))
    .bind(parent_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE membership SET team_id = $2 WHERE org_id = $1 AND user_id = $3")
        .bind(seed.org.as_uuid())
        .bind(child_id)
        .bind(seed.member_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (st, _) = json_req(
        &seed.state,
        "POST",
        "/api/v1/workspace/grants/inherit",
        &seed.owner_token,
        Some(json!({
            "team_id": parent_public.to_string(),
            "permission_id": "finance.report.read",
            "effect": "allow"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (principal, _, _) = workspace::load_principal(&seed.pool, seed.org, seed.member_id, "t")
        .await
        .unwrap();
    assert!(companyos_authz::is_allowed(
        &principal,
        &companyos_authz::perms::finance_report_read()
    ));

    let (st, _) = json_req(
        &seed.state,
        "POST",
        "/api/v1/workspace/grants/inherit",
        &seed.owner_token,
        Some(json!({
            "team_id": child_public.to_string(),
            "permission_id": "finance.report.read",
            "effect": "deny"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (principal, _, _) = workspace::load_principal(&seed.pool, seed.org, seed.member_id, "t")
        .await
        .unwrap();
    assert!(!companyos_authz::is_allowed(
        &principal,
        &companyos_authz::perms::finance_report_read()
    ));

    let (st, del) = json_req(
        &seed.state,
        "POST",
        "/api/v1/workspace/grants/delegate",
        &seed.owner_token,
        Some(json!({
            "to_membership_id": seed.member_membership_public,
            "permission_id": "finance.ledger.read",
            "expires_at": (Utc::now() + Duration::seconds(2)).to_rfc3339()
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{del}");
    let (principal, _, _) = workspace::load_principal(&seed.pool, seed.org, seed.member_id, "t")
        .await
        .unwrap();
    assert!(companyos_authz::is_allowed(
        &principal,
        &companyos_authz::perms::finance_ledger_read()
    ));

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let (principal, _, _) = workspace::load_principal(&seed.pool, seed.org, seed.member_id, "t")
        .await
        .unwrap();
    assert!(!companyos_authz::is_allowed(
        &principal,
        &companyos_authz::perms::finance_ledger_read()
    ));
}

#[tokio::test]
async fn network_allowlist_denies_non_listed_source() {
    let Some(seed) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };

    let (st, _) = json_req(
        &seed.state,
        "PUT",
        "/api/v1/governance/network",
        &seed.owner_token,
        Some(json!({
            "infra_tier": "dedicated",
            "allowlist_enabled": true,
            "cidr_allowlist": ["10.0.0.0/8"],
            "mtls_client_ids": []
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let app = build_router(seed.state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/internal/network-gate")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "org_id": seed.org_public,
                        "source_ip": "203.0.113.10",
                        "mtls_client_id": null
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["allowed"], false);

    let res = build_router(seed.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/internal/network-gate")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "org_id": seed.org_public,
                        "source_ip": "10.1.2.3",
                        "mtls_client_id": null
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["allowed"], true);
}

#[tokio::test]
async fn ediscovery_export_hash_chain_verifies() {
    let Some(seed) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };

    let _ = json_req(
        &seed.state,
        "PUT",
        "/api/v1/governance/sla",
        &seed.owner_token,
        Some(json!({ "availability_pct_bps": 9995, "latency_p99_ms": 300 })),
    )
    .await;

    let (st, hold) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/ediscovery/holds",
        &seed.owner_token,
        Some(json!({ "reason": "litigation-hold-1" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{hold}");

    let (st, job) = json_req(
        &seed.state,
        "POST",
        "/api/v1/governance/ediscovery/exports",
        &seed.owner_token,
        Some(json!({
            "kind": "ediscovery",
            "include_contexts": ["audit"],
            "legal_hold_id": hold["id"]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "completed");
    assert_eq!(job["hash_chain_ok"], true);

    let id = job["id"].as_str().unwrap();
    let app = build_router(seed.state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/governance/ediscovery/exports/{id}/download"
                ))
                .header("authorization", format!("Bearer {}", seed.owner_token))
                .header("x-request-id", "dl")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert!(!bytes.is_empty());
    let pack: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(pack["hash_chain_ok"], true);
}
