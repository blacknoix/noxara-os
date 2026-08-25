//! Auth keyring handle stored in AppState.

use companyos_auth_token::KeyRing;

#[derive(Clone)]
pub struct AuthKeys {
    pub ring: KeyRing,
}

impl AuthKeys {
    pub fn new(ring: KeyRing) -> Self {
        Self { ring }
    }
}
