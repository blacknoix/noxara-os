//! Envelope decryption for webhook signing secrets (same layout as core).
//!
//! AES-256-GCM. Ciphertext layout: `0x01 || nonce(12) || ciphertext+tag`.
//! Key derivation mirrors `companyos_core::webhook_crypto::WebhookEncryptor`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use thiserror::Error;

const VERSION: u8 = 0x01;

#[derive(Debug, Error)]
pub enum WebhookCryptoError {
    #[error("WEBHOOK_ENCRYPTION_KEY missing or invalid (need base64 32-byte key)")]
    MissingKey,
    #[error("decryption failed")]
    Decrypt,
    #[error("unsupported ciphertext version")]
    BadVersion,
}

#[derive(Clone)]
pub struct WebhookDecryptor {
    cipher: Aes256Gcm,
}

impl WebhookDecryptor {
    pub fn from_env() -> Result<Self, WebhookCryptoError> {
        if let Ok(raw) = std::env::var("WEBHOOK_ENCRYPTION_KEY") {
            return Self::from_base64(&raw);
        }
        if let Ok(raw) = std::env::var("HR_FIELD_ENCRYPTION_KEY") {
            return Self::from_base64(&raw);
        }
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
