//! Phase 2.6 governance integration tests — access review, audit verify,
//! retention, and API keys.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration, Utc};
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_core::governance::entitlement;
use companyos_core::state::AppState;
use companyos_core::{build_router, migrate, workspace};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::Value;
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
    let ring = KeyRing::from_secret("test-auth-secret-phase26");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    AppState::new(pool, ring)
}

struct Seeded {
    pool: sqlx::PgPool,
    org: OrgId,
    owner_id: Uuid,
    #[allow(dead_code)]
    owner_public: String,
    #[allow(dead_code)]
    member_id: Uuid,
    owner_membership_id: Uuid,
    owner_token: String,
    member_token: String,
}

async fn seed_org_with_tokens() -> Option<Seeded> {
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
                .header("x-request-id", "reg-gov")
                .body(Body::from(
                    serde_json::json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Governance Test Co"
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
    .bind(PublicId::new(IdKind::Membership, mem_id).as_str())
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
        pool,
        org,
        owner_id,
        owner_public,
        member_id,
        owner_membership_id: owner_mem.0,
        owner_token: owner_issued.access_token,
        member_token: member_issued.access_token,
    })
}

#[tokio::test]
async fn access_review_requires_admin_perm() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/governance/access-review/who-could-see?permission_id=hr.payroll.read&period_start=2020-01-01T00:00:00Z&period_end=2030-01-01T00:00:00Z")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("x-request-id", "gov-403")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn access_review_who_could_see_and_who_did_and_kickoff_export() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let period_start = Utc::now() - Duration::days(1);
    let period_end = Utc::now() + Duration::days(1);

    // Seed entitlement history directly (provisioning hook is wired separately).
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    entitlement::record_entitlements_for_membership(
        &mut tx,
        seeded.org,
        seeded.owner_id,
        seeded.owner_membership_id,
        "owner",
        &[("hr.payroll.read".to_string(), "allow".to_string())],
        Utc::now() - Duration::hours(2),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // Seed a matching audit read (via the payslip alias) so who-did has data.
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO audit_entry (id, org_id, actor_user_id, actor_on_behalf_of, action, resource_type, resource_id, metadata)
        VALUES ($1,$2,$3,$3,'hr.payslip.read','payslip','slip-1','{}'::jsonb)
        "#,
    )
    .bind(new_uuid_v7())
    .bind(seeded.org.as_uuid())
    .bind(seeded.owner_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let query = format!(
        "permission_id=hr.payroll.read&period_start={}&period_end={}",
        urlencoding::encode(&period_start.to_rfc3339()),
        urlencoding::encode(&period_end.to_rfc3339()),
    );

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/governance/access-review/who-could-see?{query}"
                ))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-could")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["role_key"], "owner");

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/governance/access-review/who-did?{query}"))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-did")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["action"], "hr.payslip.read");

    // Kickoff a run — snapshots could_see + did_see findings.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/governance/access-review/runs")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "gov-kickoff")
                .header("idempotency-key", "kickoff-1")
                .body(Body::from(
                    serde_json::json!({
                        "permission_id": "hr.payroll.read",
                        "period_start": period_start.to_rfc3339(),
                        "period_end": period_end.to_rfc3339(),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let run: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();
    assert_eq!(run["summary"]["could_see_count"], 1);
    assert_eq!(run["summary"]["did_see_count"], 1);

    // Idempotency-Key replay must return the exact same run, not a second one.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/governance/access-review/runs")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "gov-kickoff-2")
                .header("idempotency-key", "kickoff-1")
                .body(Body::from(
                    serde_json::json!({
                        "permission_id": "hr.payroll.read",
                        "period_start": period_start.to_rfc3339(),
                        "period_end": period_end.to_rfc3339(),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let replay: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(replay["id"], run_id);

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let run_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM access_review_run WHERE org_id = $1 AND permission_id = 'hr.payroll.read'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        run_count.0, 1,
        "idempotent replay must not create a second run"
    );

    // Fetch the run directly.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/governance/access-review/runs/{run_id}"))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-getrun")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Export JSON.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/governance/access-review/runs/{run_id}/export?format=json"
                ))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-export-json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let exported: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let items = exported["items"].as_array().unwrap();
    assert!(
        items.len() >= 3,
        "expect could_see + did_see + role_summary rows"
    );

    // Export CSV.
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/governance/access-review/runs/{run_id}/export?format=csv"
                ))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-export-csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get("content-type").unwrap(), "text/csv");
    let csv =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(csv.starts_with("kind,user_id,role_key,permission_id,created_at,detail"));
    assert!(csv.contains("could_see"));
    assert!(csv.contains("did_see"));
}

async fn seed_audit_rows(seeded: &Seeded, n: usize) {
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    for i in 0..n {
        sqlx::query(
            r#"
            INSERT INTO audit_entry (id, org_id, actor_user_id, actor_on_behalf_of, action, resource_type, resource_id, metadata)
            VALUES ($1,$2,$3,$3,'workspace.member.invite','membership',$4,'{}'::jsonb)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(seeded.org.as_uuid())
        .bind(seeded.owner_id)
        .bind(format!("mem-{i}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn audit_verify_ok_on_untampered_chain() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    seed_audit_rows(&seeded, 3).await;
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/governance/audit/verify")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "gov-verify")
                .body(Body::from(
                    serde_json::json!({ "partition_key": null }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["ok"], true);
    assert!(body["first_break"].is_null());
    assert_eq!(body["rows_checked"], 3);
}

#[tokio::test]
async fn audit_verify_detects_tampering() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    seed_audit_rows(&seeded, 3).await;
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    sqlx::query(
        "UPDATE audit_entry SET resource_id = resource_id || '-tampered' WHERE org_id = $1",
    )
    .bind(seeded.org.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/governance/audit/verify")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "gov-verify-tamper")
                .body(Body::from(
                    serde_json::json!({ "partition_key": null }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["ok"], false);
    assert!(body["first_break"]
        .as_str()
        .unwrap()
        .contains("content_hash"));
}

#[tokio::test]
async fn retention_defaults_update_idempotency_and_dry_run() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/governance/retention")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-ret-get")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["default_retention_days"], 2555);
    assert_eq!(body["version"], 0);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/governance/retention")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "gov-ret-put")
                .header("idempotency-key", "ret-1")
                .body(Body::from(
                    serde_json::json!({ "default_retention_days": 400, "overrides": null })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(updated["default_retention_days"], 400);
    assert_eq!(updated["version"], 1);

    // Replay with the same Idempotency-Key must not bump the version again.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/governance/retention")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "gov-ret-put-2")
                .header("idempotency-key", "ret-1")
                .body(Body::from(
                    serde_json::json!({ "default_retention_days": 400, "overrides": null })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let replay: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        replay["version"], 1,
        "idempotent replay must not double-apply"
    );

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/governance/retention/dry-run")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-ret-dryrun")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let dry: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(dry["would_affect_estimate"].as_i64().unwrap() >= 0);
    assert!(!dry["partitions"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn api_keys_lifecycle_create_rotate_revoke() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/governance/api-keys")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "gov-key-create")
                .body(Body::from(
                    serde_json::json!({
                        "name": "CI integration key",
                        "scopes": ["read:reports"],
                        "expires_at": null,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let secret = created["secret"].as_str().unwrap().to_string();
    let key_id = created["key"]["id"].as_str().unwrap().to_string();
    assert!(!secret.is_empty());
    assert_eq!(created["key"]["key_prefix"].as_str().unwrap(), &secret[..8]);
    assert!(created["key"].get("key_hash").is_none());
    assert!(created["key"].get("hash").is_none());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/governance/api-keys")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-key-list")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/governance/api-keys/{key_id}/rotate"))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-key-rotate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let rotated: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let new_secret = rotated["secret"].as_str().unwrap().to_string();
    let new_key_id = rotated["key"]["id"].as_str().unwrap().to_string();
    assert_ne!(new_secret, secret);
    assert_ne!(new_key_id, key_id);

    // Rotating the now-revoked old key must fail.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/governance/api-keys/{key_id}/rotate"))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-key-rotate-old")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/governance/api-keys/{new_key_id}/revoke"))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "gov-key-revoke")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let revoked: (Option<chrono::DateTime<Utc>>,) =
        sqlx::query_as("SELECT revoked_at FROM org_api_key WHERE public_id = $1")
            .bind(&new_key_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert!(revoked.0.is_some());
}

#[tokio::test]
async fn api_keys_require_admin_perm() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/governance/api-keys")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("x-request-id", "gov-key-403")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
