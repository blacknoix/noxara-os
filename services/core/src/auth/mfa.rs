//! TOTP MFA + recovery codes.

use companyos_auth_token::hash_token;
use rand::RngCore;
use totp_rs::{Algorithm, Secret, TOTP};

pub fn generate_totp_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    Secret::Raw(bytes.to_vec()).to_encoded().to_string()
}

pub fn totp_from_secret(secret_b32: &str, account: &str) -> Result<TOTP, String> {
    let secret = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| e.to_string())?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        Some("CompanyOS".into()),
        account.to_string(),
    )
    .map_err(|e| e.to_string())
}

pub fn verify_totp(secret_b32: &str, account: &str, code: &str) -> Result<bool, String> {
    let totp = totp_from_secret(secret_b32, account)?;
    Ok(totp.check_current(code).unwrap_or(false))
}

pub fn provisioning_uri(secret_b32: &str, account: &str) -> Result<String, String> {
    let totp = totp_from_secret(secret_b32, account)?;
    Ok(totp.get_url())
}

/// Generate recovery codes (plaintext once) + hashes for storage.
pub fn generate_recovery_codes(count: usize) -> (Vec<String>, Vec<String>) {
    let mut plain = Vec::with_capacity(count);
    let mut hashes = Vec::with_capacity(count);
    for _ in 0..count {
        let mut bytes = [0u8; 5];
        rand::thread_rng().fill_bytes(&mut bytes);
        let code = format!(
            "{:02x}{:02x}-{:02x}{:02x}-{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]
        );
        hashes.push(hash_token(&code));
        plain.push(code);
    }
    (plain, hashes)
}

pub fn recovery_code_matches(code: &str, code_hash: &str) -> bool {
    hash_token(code) == code_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_round_trip() {
        let secret = generate_totp_secret();
        let totp = totp_from_secret(&secret, "user@example.com").unwrap();
        let code = totp.generate_current().unwrap();
        assert!(verify_totp(&secret, "user@example.com", &code).unwrap());
        assert!(!verify_totp(&secret, "user@example.com", "000000").unwrap());
    }

    #[test]
    fn recovery_codes_unique() {
        let (plain, hashes) = generate_recovery_codes(8);
        assert_eq!(plain.len(), 8);
        assert_eq!(hashes.len(), 8);
        assert!(recovery_code_matches(&plain[0], &hashes[0]));
        assert!(!recovery_code_matches(&plain[0], &hashes[1]));
    }
}
