//! Shared application state for companyos-hr.

use companyos_auth_token::KeyRing;
use sqlx::PgPool;

use crate::crypto::FieldEncryptor;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: KeyRing,
    pub encryptor: FieldEncryptor,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing, encryptor: FieldEncryptor) -> Self {
        Self {
            pool,
            keyring,
            encryptor,
        }
    }
}
