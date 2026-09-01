//! File service state.

use companyos_auth_token::KeyRing;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: KeyRing,
    pub minio_endpoint: Option<String>,
    pub minio_access_key: String,
    pub minio_secret_key: String,
    pub bucket: String,
    /// Serving cell region — objects outside this region are rejected.
    pub cell_region: companyos_tenancy::RegionCode,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing) -> Self {
        let cell_region = std::env::var("COMPANYOS_CELL_REGION")
            .ok()
            .and_then(|s| companyos_tenancy::RegionCode::parse(&s).ok())
            .or_else(|| {
                std::env::var("COMPANYOS_CELL_ID")
                    .ok()
                    .and_then(|s| companyos_tenancy::CellId::parse(&s).ok())
                    .map(|c| c.region())
            })
            .unwrap_or(companyos_tenancy::RegionCode::Us);
        Self {
            pool,
            keyring,
            minio_endpoint: std::env::var("MINIO_ENDPOINT")
                .ok()
                .filter(|s| !s.is_empty()),
            minio_access_key: std::env::var("MINIO_ACCESS_KEY")
                .unwrap_or_else(|_| "minioadmin".into()),
            minio_secret_key: std::env::var("MINIO_SECRET_KEY")
                .unwrap_or_else(|_| "minioadmin".into()),
            bucket: std::env::var("FILE_BUCKET").unwrap_or_else(|_| "companyos-files".into()),
            cell_region,
        }
    }
}
