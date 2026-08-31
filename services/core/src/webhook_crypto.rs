//! Envelope encryption for outbound webhook signing secrets.
//!
//! AES-256-GCM. Ciphertext layout: `0x01 || nonce(12) || ciphertext+tag`.
//! Plaintext secrets are returned once at create/rotate and never logged.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use thiserror::Error;

const VERSION: u8 = 0x01;

#[derive(Debug, Error)]
pub enum WebhookCryptoError {
    #[error("WEBHOOK_ENCRYPTION_KEY missing or invalid (need base64 32-byte key)")]
    MissingKey,
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed")]
    Decrypt,
    #[error("unsupported ciphertext version")]
    BadVersion,
}

#[derive(Clone)]
pub struct WebhookEncryptor {
    cipher: Aes256Gcm,
}

impl WebhookEncryptor {
    pub fn from_env() -> Result<Self, WebhookCryptoError> {
        if let Ok(raw) = std::env::var("WEBHOOK_ENCRYPTION_KEY") {
            return Self::from_base64(&raw);
        }
        if let Ok(raw) = std::env::var("HR_FIELD_ENCRYPTION_KEY") {
            return Self::from_base64(&raw);
        }
        // Derive a stable local key from AUTH_JWT_SECRET so core always boots in CI/dev.
        // Production must set WEBHOOK_ENCRYPTION_KEY explicitly.
        let material = std::env::var("AUTH_JWT_SECRET").unwrap_or_else(|_| {
            tracing::warn!("WEBHOOK_ENCRYPTION_KEY unset — deriving from AUTH_JWT_SECRET/fallback");
            "companyos-whk-local-dev-key-32b!".into()
        });
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(format!("companyos-webhook-enc:{material}").as_bytes());
        Ok(Self {
            cipher: Aes256Gcm::new_from_slice(&hash[..32])
                .map_err(|_| WebhookCryptoError::MissingKey)?,
        })
    }

    pub fn from_base64(b64: &str) -> Result<Self, WebhookCryptoError> {
        let bytes = B64
            .decode(b64.trim())
            .map_err(|_| WebhookCryptoError::MissingKey)?;
        if bytes.len() != 32 {
            return Err(WebhookCryptoError::MissingKey);
        }
        Ok(Self {
            cipher: Aes256Gcm::new_from_slice(&bytes).map_err(|_| WebhookCryptoError::MissingKey)?,
        })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, WebhookCryptoError> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| WebhookCryptoError::Encrypt)?;
        let mut out = Vec::with_capacity(1 + 12 + ct.len());
        out.push(VERSION);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>, WebhookCryptoError> {
        if blob.len() < 1 + 12 + 16 || blob[0] != VERSION {
            return Err(WebhookCryptoError::BadVersion);
        }
        let nonce = Nonce::from_slice(&blob[1..13]);
        self.cipher
            .decrypt(nonce, &blob[13..])
            .map_err(|_| WebhookCryptoError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let enc = WebhookEncryptor::from_env().unwrap();
        let pt = b"whsec_test_secret_value";
        let ct = enc.encrypt(pt).unwrap();
        assert_ne!(&ct[13..], pt.as_slice());
        assert_eq!(enc.decrypt(&ct).unwrap(), pt);
    }
}
