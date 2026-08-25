//! Phase 1.2 Workspace DoD integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::KeyRing;
use companyos_authz::{is_allowed, perms, PermissionId, Principal, Role, SENSITIVE_ACTIONS};
use companyos_core::auth::password;
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
    let ring = KeyRing::from_secret("test-auth-secret-phase12");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    AppState::new(pool, ring)
}

struct Seeded {
    pool: sqlx::PgPool,
    org: OrgId,
    owner_id: Uuid,
    owner_public: String,
    #[allow(dead_code)]
    member_id: Uuid,
    member_public: String,
    owner_token: String,
    member_token: String,
}

async fn seed_org_with_tokens() -> Option<Seeded> {
    let pool = pool().await?;
    let state = app_state(pool.clone()).await;
    let app = build_router(state.clone());

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let member_email = format!("member-{}@test.local", new_uuid_v7());

    // Register owner org via API
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "reg1")
                .body(Body::from(
                    serde_json::json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Workspace Test Co"
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

    // Verify email
    let token_row: (String,) = sqlx::query_as(
        r#"
        SELECT encode(decode(substring(token_hash from 1 for 8), 'hex'), 'hex')
        FROM email_token WHERE purpose = 'email_verify' ORDER BY created_at DESC LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(("x".into(),));
    let _ = token_row;

    // Directly mark verified + disable MFA for test tokens via local path:
    // Insert sessions using local auth is easier — use COMPANYOS_LOCAL_AUTH for member ops,
    // but we need JWT for policy_version tests. Seed manually and issue tokens.
    let org = OrgId::from_public(&org_public.parse().unwrap()).unwrap();
    let owner_id = owner_public.parse::<PublicId>().unwrap().uuid();

    sqlx::query("UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1")
        .bind(owner_id)
        .bind(companyos_core::auth::mfa::generate_totp_secret())
        .execute(&pool)
        .await
        .unwrap();

    // Ensure provisioning completed
    workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok();

    // Create member user + membership
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

    // Issue tokens via switch-org style session helpers
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
        member_public,
        owner_token: owner_issued.access_token,
        member_token: member_issued.access_token,
    })
}

#[tokio::test]
async fn permission_catalogue_matches_db() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no database");
        return;
    };
    workspace::assert_matches_catalogue(&pool)
        .await
        .expect("permission_definition must match catalogue");
}

#[tokio::test]
async fn org_provisioning_seeds_usable_workspace() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let roles: (i64,) =
        sqlx::query_as("SELECT COUNT(*)::BIGINT FROM org_role WHERE org_id = $1 AND is_system")
            .bind(seeded.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(roles.0 >= 7, "expected system roles, got {}", roles.0);
    let owners: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM membership WHERE org_id = $1 AND role = 'owner' AND status = 'active'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(owners.0, 1);
    let cmd: (String,) = sqlx::query_as(
        "SELECT status FROM workspace_command WHERE org_id = $1 AND command_type = 'OrgProvisioning'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(cmd.0, "completed");
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn last_owner_cannot_be_revoked_or_demoted() {
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
                .uri(format!(
                    "/api/v1/workspace/members/{}/revoke",
                    seeded.owner_public
                ))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "rev-owner")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/v1/workspace/members/{}/role",
                    seeded.owner_public
                ))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .header("x-request-id", "demote-owner")
                .body(Body::from(r#"{"role":"member"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/workspace/members/{}/suspend",
                    seeded.owner_public
                ))
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "sus-owner")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn member_denied_sensitive_actions_via_api() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    // invite
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workspace/members/invite")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"email":"x@test.local","role":"member"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // settings
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/workspace/organizations/settings")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"currency":"EUR"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // role change
    let res = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/v1/workspace/members/{}/role",
                    seeded.member_public
                ))
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"role":"admin"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn system_role_deny_matrix_unit() {
    for role in Role::all_system() {
        if *role == Role::Owner || *role == Role::Admin {
            continue;
        }
        let p = Principal::with_roles(vec![*role]);
        for perm in SENSITIVE_ACTIONS {
            // Finance may approve invoices
            if *role == Role::Finance && *perm == "finance.invoice.approve" {
                assert!(is_allowed(&p, &PermissionId::from(*perm)));
                continue;
            }
            // Manager may invite
            if *role == Role::Manager && *perm == "workspace.member.invite" {
                assert!(is_allowed(&p, &PermissionId::from(*perm)));
                continue;
            }
            assert!(
                !is_allowed(&p, &PermissionId::from(*perm)),
                "{role:?} must deny {perm}"
            );
        }
    }
}

#[tokio::test]
async fn role_change_invalidates_policy_within_5s() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    // Member can read org
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/workspace/organizations")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Owner bumps policy by updating settings
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/workspace/organizations/settings")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"timezone":"Europe/London"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Within 5s, old member token (stale policy_version) must be rejected
    tokio::time::sleep(Duration::from_millis(50)).await;
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/workspace/organizations")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tenant_isolation_members_and_roles() {
    let Some(a) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org_with_tokens().await else {
        return;
    };
    let state = app_state(a.pool.clone()).await;
    let app = build_router(state);

    // Org A token listing members — should not see org B users when querying under A session
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/workspace/members")
                .header("authorization", format!("Bearer {}", a.owner_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let items = body["items"].as_array().unwrap();
    for item in items {
        let email = item["email"].as_str().unwrap_or("");
        assert!(!email.contains(&b.owner_id.to_string()));
    }
    let emails: Vec<&str> = items.iter().filter_map(|i| i["user_id"].as_str()).collect();
    assert!(!emails.contains(&b.owner_public.as_str()));
    assert!(!emails.contains(&b.member_public.as_str()));

    // Direct RLS planted query
    let mut tx = a.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, a.org).await.unwrap();
    let foreign: Vec<(Uuid,)> = sqlx::query_as("SELECT user_id FROM membership WHERE user_id = $1")
        .bind(b.owner_id)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert!(foreign.is_empty(), "RLS must hide org B memberships");
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn owner_can_invite_and_update_settings() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let invite_email = format!("invitee-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workspace/members/invite")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"email": invite_email, "role": "member"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let res = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/workspace/organizations/settings")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"currency":"GBP","branding":{"logo_url":""}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[test]
fn owner_allows_all_sensitive() {
    let p = Principal::with_roles(vec![Role::Owner]);
    for perm in SENSITIVE_ACTIONS {
        assert!(is_allowed(&p, &PermissionId::from(*perm)));
    }
    assert!(is_allowed(&p, &perms::workspace_member_invite()));
}
