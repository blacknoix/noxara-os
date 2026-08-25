//! Password hashing (Argon2id + per-user salt) and strength helpers.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Argon2, Params, Version};
use rand::rngs::OsRng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("password hashing failed: {0}")]
    Hash(String),
    #[error("invalid password hash")]
    InvalidHash,
    #[error("password does not match")]
    Mismatch,
    #[error("password too weak: {0}")]
    Weak(String),
}

/// Hash a password with Argon2id and a fresh per-user salt.
/// Returns (PHC hash string, salt string).
pub fn hash_password(password: &str) -> Result<(String, String), PasswordError> {
    validate_strength(password)?;
    let salt = SaltString::generate(&mut OsRng);
    let argon = Argon2::new(
        argon2::Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19456, 2, 1, None).map_err(|e| PasswordError::Hash(e.to_string()))?,
    );
    let hash = argon
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| PasswordError::Hash(e.to_string()))?
        .to_string();
    Ok((hash, salt.to_string()))
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<(), PasswordError> {
    let parsed = PasswordHash::new(password_hash).map_err(|_| PasswordError::InvalidHash)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| PasswordError::Mismatch)
}

pub fn validate_strength(password: &str) -> Result<(), PasswordError> {
    if password.len() < 10 {
        return Err(PasswordError::Weak(
            "password must be at least 10 characters".into(),
        ));
    }
    if password.chars().count() > 128 {
        return Err(PasswordError::Weak(
            "password must be at most 128 characters".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let (hash, salt) = hash_password("correct-horse-battery").unwrap();
        assert!(!salt.is_empty());
        verify_password("correct-horse-battery", &hash).unwrap();
        assert!(verify_password("wrong", &hash).is_err());
    }

    #[test]
    fn rejects_short() {
        assert!(hash_password("short").is_err());
    }
}
