//! App-level envelope encryption for HR restricted fields.
//!
//! AES-256-GCM with a master key from `HR_FIELD_ENCRYPTION_KEY` (base64 32 bytes).
//! Ciphertext layout: `0x01 || nonce(12) || ciphertext+tag`.
//! Plaintext of compensation / government IDs / bank / tax must never be logged.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use thiserror::Error;

pub const DEFAULT_KEY_ID: &str = "hr-v1";

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("HR_FIELD_ENCRYPTION_KEY missing or invalid (need base64 32-byte key)")]
    MissingKey,
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed")]
    Decrypt,
    #[error("unsupported ciphertext version")]
    BadVersion,
}

#[derive(Clone)]
pub struct FieldEncryptor {
    cipher: Aes256Gcm,
    key_id: String,
}

impl FieldEncryptor {
    /// Build from env; falls back to a deterministic local-dev key when
    /// `COMPANYOS_LOCAL_AUTH=1` and the env key is unset (tests only).
    pub fn from_env() -> Result<Self, CryptoError> {
        if let Ok(raw) = std::env::var("HR_FIELD_ENCRYPTION_KEY") {
            return Self::from_base64(&raw, DEFAULT_KEY_ID);
        }
        let local = matches!(
            std::env::var("COMPANYOS_LOCAL_AUTH").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        );
        if local || cfg!(test) {
            tracing::warn!(
                "HR_FIELD_ENCRYPTION_KEY unset — using local-dev ephemeral key (not for production)"
            );
            let mut key = [0u8; 32];
            key.copy_from_slice(b"companyos-hr-local-dev-key-32b!!");
            return Ok(Self {
                cipher: Aes256Gcm::new_from_slice(&key).map_err(|_| CryptoError::MissingKey)?,
                key_id: DEFAULT_KEY_ID.to_string(),
            });
        }
        Err(CryptoError::MissingKey)
    }

    pub fn from_base64(b64: &str, key_id: &str) -> Result<Self, CryptoError> {
        let bytes = B64.decode(b64.trim()).map_err(|_| CryptoError::MissingKey)?;
        if bytes.len() != 32 {
            return Err(CryptoError::MissingKey);
        }
        Ok(Self {
            cipher: Aes256Gcm::new_from_slice(&bytes).map_err(|_| CryptoError::MissingKey)?,
            key_id: key_id.to_string(),
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
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
        out.push(0x01);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < 1 + 12 + 16 || blob[0] != 0x01 {
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

    pub fn encrypt_i64(&self, v: i64) -> Result<Vec<u8>, CryptoError> {
        self.encrypt(&v.to_le_bytes())
    }

    pub fn decrypt_i64(&self, blob: &[u8]) -> Result<i64, CryptoError> {
        let bytes = self.decrypt(blob)?;
        if bytes.len() != 8 {
            return Err(CryptoError::Decrypt);
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&bytes);
        Ok(i64::from_le_bytes(arr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_string_and_i64() {
        let enc = FieldEncryptor::from_base64(
            &B64.encode([7u8; 32]),
            "test",
        )
        .unwrap();
        let ct = enc.encrypt_str("SSN-SECRET").unwrap();
        assert!(!ct.windows(6).any(|w| w == b"SECRET"));
        assert_eq!(enc.decrypt_str(&ct).unwrap(), "SSN-SECRET");
        let ct2 = enc.encrypt_i64(1_234_500).unwrap();
        assert_eq!(enc.decrypt_i64(&ct2).unwrap(), 1_234_500);
    }
}
