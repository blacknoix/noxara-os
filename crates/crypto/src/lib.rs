//! CompanyOS envelope encryption — AES-256-GCM with KMS-wrapped per-org DEKs.
//!
//! # Ciphertext layout (field blobs)
//!
//! `0x01 || nonce(12) || ciphertext+tag` — same framing as HR / webhook field
//! encryptors. CMEK does **not** invent a second crypto stack; it changes
//! which master wrap key protects the org data encryption key (DEK).
//!
//! # Key hierarchy
//!
//! ```text
//! CMK (customer-managed, or platform mock master)
//!   └── wraps Org DEK (32 bytes)
//!         └── encrypts sensitive field blobs
//! ```
//!
//! CI uses [`MockKms`]. Production can swap in a real KMS client implementing
//! [`Kms`] without changing ciphertext format.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Default ciphertext version byte (matches HR / webhook encryptors).
pub const CIPHERTEXT_VERSION: u8 = 0x01;

/// Wrapped-key blob version for DEK storage.
pub const WRAPPED_DEK_VERSION: u8 = 0x01;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CryptoError {
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed")]
    Decrypt,
    #[error("unsupported ciphertext version")]
    BadVersion,
    #[error("key not found: {0}")]
    KeyNotFound(String),
    #[error("key revoked: {0}")]
    KeyRevoked(String),
    #[error("invalid key material")]
    InvalidKey,
    #[error("kms error: {0}")]
    Kms(String),
}

/// Customer-managed (or platform) wrap key identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CmkId(pub String);

impl CmkId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Opaque wrapped DEK bytes stored per org (versioned).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedDek {
    pub version: u8,
    pub cmk_id: CmkId,
    /// Base64 of KMS-wrapped DEK ciphertext.
    pub wrapped_b64: String,
}

/// Pluggable KMS: wrap / unwrap / rotate / revoke.
pub trait Kms: Send + Sync {
    fn create_key(&self, alias: &str) -> Result<CmkId, CryptoError>;
    fn wrap(&self, cmk_id: &CmkId, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>;
    fn unwrap(&self, cmk_id: &CmkId, wrapped: &[u8]) -> Result<Vec<u8>, CryptoError>;
    /// Mark key revoked — subsequent unwrap/wrap MUST fail.
    fn revoke(&self, cmk_id: &CmkId) -> Result<(), CryptoError>;
    fn is_revoked(&self, cmk_id: &CmkId) -> bool;
}

#[derive(Clone)]
struct MockKeyState {
    material: [u8; 32],
    revoked: bool,
}

/// In-memory mock KMS for CI — AES-GCM wrap with per-key material.
#[derive(Clone, Default)]
pub struct MockKms {
    inner: Arc<Mutex<HashMap<String, MockKeyState>>>,
}

impl MockKms {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, MockKeyState>> {
        self.inner.lock().expect("mock kms lock")
    }
}

impl Kms for MockKms {
    fn create_key(&self, alias: &str) -> Result<CmkId, CryptoError> {
        let mut salt = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut salt);
        let id = format!(
            "cmk_mock_{}",
            hex::encode(Sha256::digest(
                format!("{alias}-{}", hex::encode(salt)).as_bytes()
            ))
            .chars()
            .take(16)
            .collect::<String>()
        );
        let mut material = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut material);
        self.lock().insert(
            id.clone(),
            MockKeyState {
                material,
                revoked: false,
            },
        );
        Ok(CmkId(id))
    }

    fn wrap(&self, cmk_id: &CmkId, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let guard = self.lock();
        let state = guard
            .get(cmk_id.as_str())
            .ok_or_else(|| CryptoError::KeyNotFound(cmk_id.0.clone()))?;
        if state.revoked {
            return Err(CryptoError::KeyRevoked(cmk_id.0.clone()));
        }
        let cipher =
            Aes256Gcm::new_from_slice(&state.material).map_err(|_| CryptoError::InvalidKey)?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CryptoError::Encrypt)?;
        let mut out = Vec::with_capacity(1 + 12 + ct.len());
        out.push(WRAPPED_DEK_VERSION);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    fn unwrap(&self, cmk_id: &CmkId, wrapped: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let guard = self.lock();
        let state = guard
            .get(cmk_id.as_str())
            .ok_or_else(|| CryptoError::KeyNotFound(cmk_id.0.clone()))?;
        if state.revoked {
            return Err(CryptoError::KeyRevoked(cmk_id.0.clone()));
        }
        if wrapped.len() < 1 + 12 + 16 || wrapped[0] != WRAPPED_DEK_VERSION {
            return Err(CryptoError::BadVersion);
        }
        let cipher =
            Aes256Gcm::new_from_slice(&state.material).map_err(|_| CryptoError::InvalidKey)?;
        let nonce = Nonce::from_slice(&wrapped[1..13]);
        cipher
            .decrypt(nonce, &wrapped[13..])
            .map_err(|_| CryptoError::Decrypt)
    }

    fn revoke(&self, cmk_id: &CmkId) -> Result<(), CryptoError> {
        let mut guard = self.lock();
        let state = guard
            .get_mut(cmk_id.as_str())
            .ok_or_else(|| CryptoError::KeyNotFound(cmk_id.0.clone()))?;
        state.revoked = true;
        Ok(())
    }

    fn is_revoked(&self, cmk_id: &CmkId) -> bool {
        self.lock()
            .get(cmk_id.as_str())
            .map(|s| s.revoked)
            .unwrap_or(true)
    }
}

/// Generate a fresh 32-byte DEK and wrap it under `cmk_id`.
pub fn generate_wrapped_dek(kms: &dyn Kms, cmk_id: &CmkId) -> Result<WrappedDek, CryptoError> {
    let mut dek = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut dek);
    let wrapped = kms.wrap(cmk_id, &dek)?;
    Ok(WrappedDek {
        version: WRAPPED_DEK_VERSION,
        cmk_id: cmk_id.clone(),
        wrapped_b64: B64.encode(wrapped),
    })
}

/// Re-wrap an existing DEK under a new CMK (rotation without changing DEK).
pub fn rewrap_dek(
    kms: &dyn Kms,
    old: &WrappedDek,
    new_cmk: &CmkId,
) -> Result<WrappedDek, CryptoError> {
    let wrapped = B64
        .decode(old.wrapped_b64.as_bytes())
        .map_err(|_| CryptoError::InvalidKey)?;
    let dek = kms.unwrap(&old.cmk_id, &wrapped)?;
    let new_wrapped = kms.wrap(new_cmk, &dek)?;
    Ok(WrappedDek {
        version: WRAPPED_DEK_VERSION,
        cmk_id: new_cmk.clone(),
        wrapped_b64: B64.encode(new_wrapped),
    })
}

/// Rotate: create new CMK, re-wrap DEK, optionally revoke old CMK.
pub struct RotationResult {
    pub new_cmk_id: CmkId,
    pub wrapped_dek: WrappedDek,
    pub old_cmk_id: CmkId,
}

pub fn rotate_org_key(
    kms: &dyn Kms,
    old_wrapped: &WrappedDek,
    alias: &str,
    revoke_old: bool,
) -> Result<RotationResult, CryptoError> {
    let new_cmk = kms.create_key(alias)?;
    let wrapped_dek = rewrap_dek(kms, old_wrapped, &new_cmk)?;
    let old_cmk_id = old_wrapped.cmk_id.clone();
    if revoke_old {
        kms.revoke(&old_cmk_id)?;
    }
    Ok(RotationResult {
        new_cmk_id: new_cmk,
        wrapped_dek,
        old_cmk_id,
    })
}

/// Field encryptor bound to an unwrapped org DEK.
#[derive(Clone)]
pub struct OrgDataKey {
    cipher: Aes256Gcm,
    pub key_id: String,
}

impl OrgDataKey {
    pub fn from_wrapped(kms: &dyn Kms, wrapped: &WrappedDek) -> Result<Self, CryptoError> {
        let raw = B64
            .decode(wrapped.wrapped_b64.as_bytes())
            .map_err(|_| CryptoError::InvalidKey)?;
        let dek = kms.unwrap(&wrapped.cmk_id, &raw)?;
        if dek.len() != 32 {
            return Err(CryptoError::InvalidKey);
        }
        Ok(Self {
            cipher: Aes256Gcm::new_from_slice(&dek).map_err(|_| CryptoError::InvalidKey)?,
            key_id: wrapped.cmk_id.0.clone(),
        })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CryptoError::Encrypt)?;
        let mut out = Vec::with_capacity(1 + 12 + ct.len());
        out.push(CIPHERTEXT_VERSION);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < 1 + 12 + 16 || blob[0] != CIPHERTEXT_VERSION {
            return Err(CryptoError::BadVersion);
        }
        let nonce = Nonce::from_slice(&blob[1..13]);
        self.cipher
            .decrypt(nonce, &blob[13..])
            .map_err(|_| CryptoError::Decrypt)
    }

    pub fn encrypt_str(&self, s: &str) -> Result<Vec<u8>, CryptoError> {
        self.encrypt(s.as_bytes())
    }

    pub fn decrypt_str(&self, blob: &[u8]) -> Result<String, CryptoError> {
        let bytes = self.decrypt(blob)?;
        String::from_utf8(bytes).map_err(|_| CryptoError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_kms_rotate_and_revoke() {
        let kms = MockKms::new();
        let cmk = kms.create_key("org-a").unwrap();
        let wrapped = generate_wrapped_dek(&kms, &cmk).unwrap();
        let dek = OrgDataKey::from_wrapped(&kms, &wrapped).unwrap();
        let ct = dek.encrypt_str("secret-pii").unwrap();
        assert_eq!(dek.decrypt_str(&ct).unwrap(), "secret-pii");

        let rot = rotate_org_key(&kms, &wrapped, "org-a-v2", true).unwrap();
        assert!(kms.is_revoked(&rot.old_cmk_id));
        // Old wrap cannot decrypt
        assert!(OrgDataKey::from_wrapped(&kms, &wrapped).is_err());
        // New wrap can
        let new_dek = OrgDataKey::from_wrapped(&kms, &rot.wrapped_dek).unwrap();
        // Same DEK material — old ciphertext still decrypts under new wrap
        assert_eq!(new_dek.decrypt_str(&ct).unwrap(), "secret-pii");
        let ct2 = new_dek.encrypt_str("after-rotate").unwrap();
        assert_eq!(new_dek.decrypt_str(&ct2).unwrap(), "after-rotate");
    }

    #[test]
    fn revoked_key_cannot_wrap() {
        let kms = MockKms::new();
        let cmk = kms.create_key("x").unwrap();
        kms.revoke(&cmk).unwrap();
        assert!(kms.wrap(&cmk, b"dek").is_err());
    }
}
