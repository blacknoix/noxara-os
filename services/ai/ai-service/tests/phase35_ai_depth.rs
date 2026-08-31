//! Phase 3.5 AI depth — insights agents, meeting summaries, golden Q&A.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_ai::state::AppState as AiAppState;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
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
    companyos_ai::migrate(&pool).await.ok()?;
    Some(pool)
}

fn ai_app(pool: PgPool, ring: KeyRing) -> Router {
    std::env::set_var("AI_CALENDAR_PROVIDER", "mock");
    companyos_ai::build_router(AiAppState::new(pool, ring))
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

async fn session_token(
    pool: &PgPool,
    ring: &KeyRing,
    org: OrgId,
    user_id: Uuid,
    role: &str,
) -> String {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (mem_id, pol): (Uuid, i64) = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let tok = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        ring,
        user_id,
        &PublicId::new(IdKind::User, user_id).as_str(),
        org,
        mem_id,
        &[role.into()],
        pol,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    tok.access_token
}

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_token: String,
    member_token: String,
}

async fn seed(secret: &str) -> Option<Seeded> {
    let pool = pool().await?;
    let ring = KeyRing::from_secret(secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .ok()?;
    let core = companyos_core::build_router(companyos_core::state::AppState::new(
        pool.clone(),
        ring.clone(),
    ));

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let res = core
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "AI Phase35"
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
    let org = OrgId::from_public(&body["org_id"].as_str().unwrap().parse().unwrap()).unwrap();
    let owner_id = body["user_id"]
        .as_str()
        .unwrap()
        .parse::<PublicId>()
        .unwrap()
        .uuid();

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

    let member_id = insert_member_with_role(&pool, org, "member", "Member").await;
    let owner_token = session_token(&pool, &ring, org, owner_id, "owner").await;
    let member_token = session_token(&pool, &ring, org, member_id, "member").await;

    Some(Seeded {
        pool,
        ring,
        org,
        owner_token,
        member_token,
    })
}

async fn json_req(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
    extra_headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", "phase35");
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    let req = builder
        .body(match body {
            Some(b) => Body::from(b.to_string()),
            None => Body::empty(),
        })
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, val)
}

#[tokio::test]
async fn insight_refresh_is_propose_only() {
    let Some(seeded) = seed("ai-p35-insights").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    // Snapshot CRM/finance tables that must not be mutated by refresh.
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let deals_before: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'deal'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let mut deal_count_before = 0i64;
    let mut invoice_count_before = 0i64;
    if deals_before.0 > 0 {
        deal_count_before = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM deal WHERE org_id = $1")
            .bind(seeded.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map(|r| r.0)
            .unwrap_or(0);
        invoice_count_before =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM invoice WHERE org_id = $1")
                .bind(seeded.org.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map(|r| r.0)
                .unwrap_or(0);
    }
    tx.commit().await.unwrap();

    let (status, body) = json_req(
        &app,
        "POST",
        "/api/v1/ai/insights/refresh",
        &seeded.owner_token,
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["created"].as_u64().unwrap_or(0) >= 2, "{body}");
    let obs = body["observations"].as_array().unwrap();
    let types: Vec<&str> = obs
        .iter()
        .filter_map(|o| o["insight_type"].as_str())
        .collect();
    assert!(
        types.iter().any(|t| *t == "stale_deal"),
        "expected stale_deal: {body}"
    );
    assert!(
        types
            .iter()
            .any(|t| *t == "overdue_invoice" || *t == "upcoming_renewal"),
        "expected finance/renewal insight: {body}"
    );

    let pending = body["pending_proposals"].as_array().unwrap();
    assert!(
        !pending.is_empty(),
        "suggested writes must create pending proposals: {body}"
    );

    // Proposals remain pending — never auto-confirmed.
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    for pid in pending {
        let id = Uuid::parse_str(pid.as_str().unwrap()).unwrap();
        let row: (String,) =
            sqlx::query_as("SELECT status FROM ai_proposal WHERE id = $1 AND org_id = $2")
                .bind(id)
                .bind(seeded.org.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(row.0, "pending");
    }

    if deals_before.0 > 0 {
        let deal_after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM deal WHERE org_id = $1")
            .bind(seeded.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let inv_after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM invoice WHERE org_id = $1")
            .bind(seeded.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(deal_after.0, deal_count_before, "refresh must not mutate deals");
        assert_eq!(
            inv_after.0, invoice_count_before,
            "refresh must not mutate invoices"
        );
    }
    tx.commit().await.unwrap();

    let (get_status, get_body) = json_req(
        &app,
        "GET",
        "/api/v1/ai/insights",
        &seeded.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(get_status, StatusCode::OK, "{get_body}");
    assert!(
        !get_body["observations"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{get_body}"
    );
}

#[tokio::test]
async fn meeting_summary_mock_calendar_accept_path() {
    let Some(seeded) = seed("ai-p35-meetings").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, body) = json_req(
        &app,
        "POST",
        "/api/v1/ai/meeting-summaries/from-calendar",
        &seeded.member_token,
        Some(json!({
            "calendar_event_id": "evt_mock_001",
            "title": "Pipeline sync"
        })),
        &[("x-mock-calendar-connector", "calendar.microsoft")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "suggested");
    assert_eq!(body["calendar_connector"], "calendar.microsoft");
    assert!(body["public_id"].as_str().unwrap().starts_with("mts_"));
    assert!(body["summary_markdown"].as_str().unwrap().contains("Acme"));
    let id = body["id"].as_str().unwrap().to_string();

    let (list_status, list_body) = json_req(
        &app,
        "GET",
        "/api/v1/ai/meeting-summaries",
        &seeded.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    assert!(!list_body["items"].as_array().unwrap().is_empty());

    let (accept_status, accept_body) = json_req(
        &app,
        "POST",
        &format!("/api/v1/ai/meeting-summaries/{id}/accept"),
        &seeded.member_token,
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(accept_status, StatusCode::OK, "{accept_body}");
    assert_eq!(accept_body["status"], "accepted");

    // Accept does not auto-create tasks.
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let task_table: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'task'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    if task_table.0 > 0 {
        let tasks: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM task WHERE org_id = $1")
            .bind(seeded.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(tasks.0, 0, "accept must not auto-create tasks");
    }
    let audits: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_entry WHERE org_id = $1 AND action = 'ai.meeting_summary.accept'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(audits.0 >= 1, "accept must write audit");
    tx.commit().await.unwrap();

    // Reject path on a fresh suggestion.
    let (status2, body2) = json_req(
        &app,
        "POST",
        "/api/v1/ai/meeting-summaries/from-calendar",
        &seeded.member_token,
        Some(json!({ "calendar_event_id": "evt_mock_002" })),
        &[("x-mock-calendar-connector", "calendar.microsoft")],
    )
    .await;
    assert_eq!(status2, StatusCode::OK, "{body2}");
    let id2 = body2["id"].as_str().unwrap();
    let (rej_status, _) = json_req(
        &app,
        "POST",
        &format!("/api/v1/ai/meeting-summaries/{id2}/reject"),
        &seeded.member_token,
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(rej_status, StatusCode::OK);
}

#[tokio::test]
async fn golden_qa_multi_context_citations() {
    let Some(seeded) = seed("ai-p35-golden").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let cases: &[(&str, &[&str], &[&str])] = &[
        (
            "What is the status of the Acme deal and overdue invoice INV-1001?",
            &["sales", "finance"],
            &["Acme", "INV-1001"],
        ),
        (
            "Is Project Phoenix blocked and is Travel expense Q3 pending?",
            &["ops", "finance"],
            &["Phoenix", "Travel"],
        ),
        (
            "Summarise Northwind renewal and Cutover checklist finance dependency",
            &["sales", "ops"],
            &["Northwind", "Cutover"],
        ),
    ];

    for (question, expected_contexts, entities) in cases {
        let (status, body) = json_req(
            &app,
            "POST",
            "/api/v1/ai/ask",
            &seeded.owner_token,
            Some(json!({ "query": question })),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{question} => {body}");
        assert_eq!(body["kind"], "read");
        let citations = body["citations"].as_array().expect("citations");
        assert!(citations.len() >= 2, "{question}: {body}");

        let typed: Vec<companyos_ai::types::Citation> = citations
            .iter()
            .filter_map(|c| serde_json::from_value(c.clone()).ok())
            .collect();
        let contexts = companyos_ai::retrieval::citation_contexts(&typed);
        for ctx in *expected_contexts {
            assert!(
                contexts.iter().any(|c| c == ctx),
                "{question}: missing context {ctx} in {contexts:?} body={body}"
            );
        }

        let answer = body["message"].as_str().unwrap_or("");
        let score = companyos_ai::retrieval::score_qa_answer(answer, &typed, entities);
        assert!(
            score.passes(),
            "{question} failed golden score: {score:?}\nanswer={answer}\nbody={body}"
        );
    }
}

#[tokio::test]
async fn phase19_insights_still_compatible() {
    let Some(seeded) = seed("ai-p35-compat").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, body) = json_req(
        &app,
        "GET",
        "/api/v1/ai/insights",
        &seeded.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Empty org still returns propose-only observation cards (fixture or persisted).
    let obs = body["observations"].as_array().unwrap();
    assert!(!obs.is_empty() || body.get("empty_reason").is_some());
    for o in obs {
        assert!(o.get("title").is_some());
        assert!(o.get("body").is_some());
        assert!(o.get("evidence").is_some());
        assert!(o.get("estimate").is_some());
    }

    let (chat_status, chat_body) = json_req(
        &app,
        "POST",
        "/api/v1/ai/chat",
        &seeded.owner_token,
        Some(json!({ "message": "hello phase35 compat" })),
        &[],
    )
    .await;
    assert_eq!(chat_status, StatusCode::OK, "{chat_body}");
    assert!(chat_body.get("session_id").is_some());
}
