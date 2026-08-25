//! Breach-list checks for passwords.
//!
//! Production: Have I Been Pwned Passwords API (k-anonymity range lookup).
//! Set `HIBP_ENABLED=1` and optional `HIBP_API_BASE` (default https://api.pwnedpasswords.com).
//!
//! Tests / local without network: fixture list via `BREACH_FIXTURE_PASSWORDS`
//! (comma-separated) or the built-in tiny fixture when `HIBP_ENABLED` is off.

use sha1::{Digest, Sha1};

/// Built-in fixture used when HIBP is disabled (tests + offline local).
const FIXTURE: &[&str] = &[
    "password",
    "password123",
    "1234567890",
    "qwertyuiop",
    "letmein1234",
    "companyos-breached-fixture",
];

#[derive(Debug, Clone)]
pub enum BreachCheckMode {
    /// Use HIBP k-anonymity HTTP range API.
    Hibp { api_base: String },
    /// Compare against fixture + optional env list.
    Fixture { extras: Vec<String> },
}

impl BreachCheckMode {
    pub fn from_env() -> Self {
        let hibp = matches!(
            std::env::var("HIBP_ENABLED").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        );
        if hibp {
            let api_base = std::env::var("HIBP_API_BASE")
                .unwrap_or_else(|_| "https://api.pwnedpasswords.com".into());
            Self::Hibp { api_base }
        } else {
            let extras = std::env::var("BREACH_FIXTURE_PASSWORDS")
                .ok()
                .map(|s| {
                    s.split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            Self::Fixture { extras }
        }
    }
}

pub async fn is_breached(mode: &BreachCheckMode, password: &str) -> Result<bool, String> {
    match mode {
        BreachCheckMode::Fixture { extras } => {
            let lower = password.to_ascii_lowercase();
            if FIXTURE.iter().any(|p| *p == lower || *p == password) {
                return Ok(true);
            }
            Ok(extras.iter().any(|p| p == password || p.as_str() == lower))
        }
        BreachCheckMode::Hibp { api_base } => hibp_range_check(api_base, password).await,
    }
}

async fn hibp_range_check(api_base: &str, password: &str) -> Result<bool, String> {
    let mut hasher = Sha1::new();
    hasher.update(password.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02X}")).collect();
    let (prefix, suffix) = hex.split_at(5);
    let url = format!("{}/range/{}", api_base.trim_end_matches('/'), prefix);
    let client = reqwest::Client::new();
    let body = client
        .get(&url)
        .header("Add-Padding", "true")
        .header("User-Agent", "CompanyOS-Auth/1.1")
        .send()
        .await
        .map_err(|e| format!("HIBP request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("HIBP status: {e}"))?
        .text()
        .await
        .map_err(|e| format!("HIBP body: {e}"))?;
    for line in body.lines() {
        let (hash_suffix, _count) = line.split_once(':').unwrap_or((line, "0"));
        if hash_suffix.eq_ignore_ascii_case(suffix) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fixture_detects_known_bad() {
        let mode = BreachCheckMode::Fixture { extras: vec![] };
        assert!(is_breached(&mode, "password").await.unwrap());
        assert!(is_breached(&mode, "companyos-breached-fixture")
            .await
            .unwrap());
        assert!(!is_breached(&mode, "unique-never-breached-zzx91!")
            .await
            .unwrap());
    }
}
