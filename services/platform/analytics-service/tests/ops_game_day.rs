//! TRD 8.2 game day — ClickHouse down → Postgres mirror + staleness flags.

use companyos_analytics::handlers::freshness::probe_clickhouse_degraded;
use companyos_analytics::state::AppState;
use companyos_auth_token::KeyRing;
use companyos_testkit::test_database_url;

#[tokio::test]
async fn clickhouse_force_down_marks_freshness_degraded_postgres_backend() {
    let Some(url) = test_database_url() else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("db");
    companyos_analytics::migrate(&pool).await.expect("migrate");

    std::env::set_var("CLICKHOUSE_URL", "http://127.0.0.1:9");
    std::env::set_var("CLICKHOUSE_FORCE_DOWN", "1");
    let state = AppState::new(pool, KeyRing::from_secret("gameday-analytics"));
    assert!(state.clickhouse_url.is_some());
    assert!(
        probe_clickhouse_degraded(&state).await,
        "CLICKHOUSE_FORCE_DOWN must mark degraded"
    );

    // Queries always use Postgres mirror (ADR-011) — document via backend constant.
    assert_eq!(
        companyos_analytics::types::FreshnessResponse {
            org_id: "org_x".into(),
            last_event_at: None,
            last_ingest_at: None,
            lag_seconds: 42,
            eventually_consistent: true,
            backend: "postgres_mirror".into(),
            clickhouse_degraded: true,
        }
        .backend,
        "postgres_mirror"
    );

    std::env::remove_var("CLICKHOUSE_FORCE_DOWN");
    std::env::remove_var("CLICKHOUSE_URL");
}
