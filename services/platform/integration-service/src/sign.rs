//! HMAC-SHA256 webhook signing: `X-CompanyOS-Signature: t={unix},v1={hex}`.
//!
//! Signed payload is `{t}.{body}` (unix timestamp + raw body bytes).

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// Reject signatures whose timestamp is older (or newer) than this window.
pub const MAX_SKEW_SECS: i64 = 5 * 60;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignError {
    #[error("invalid signature header")]
    InvalidHeader,
    #[error("signature mismatch")]
    Mismatch,
    #[error("stale or future timestamp")]
    Stale,
    #[error("hmac error")]
    Hmac,
}

/// Build `X-CompanyOS-Signature` header value for `body` using `secret`.
pub fn sign(secret: &[u8], body: &[u8], timestamp: i64) -> Result<String, SignError> {
    let sig = sign_v1(secret, body, timestamp)?;
    Ok(format!("t={timestamp},v1={sig}"))
}

/// Sign with current UTC unix seconds.
pub fn sign_now(secret: &[u8], body: &[u8]) -> Result<String, SignError> {
    sign(secret, body, Utc::now().timestamp())
}

fn sign_v1(secret: &[u8], body: &[u8], timestamp: i64) -> Result<String, SignError> {
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| SignError::Hmac)?;
    let payload = format!("{timestamp}.");
    mac.update(payload.as_bytes());
    mac.update(body);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// Parse and verify an `X-CompanyOS-Signature` header against `body` + `secret`.
///
/// Rejects timestamps outside ±[`MAX_SKEW_SECS`] of `now`.
pub fn verify(secret: &[u8], body: &[u8], header: &str, now: i64) -> Result<(), SignError> {
    let (t, v1) = parse_header(header)?;
    if (now - t).abs() > MAX_SKEW_SECS {
        return Err(SignError::Stale);
    }
    let expected = sign_v1(secret, body, t)?;
    if !constant_time_eq(expected.as_bytes(), v1.as_bytes()) {
        return Err(SignError::Mismatch);
    }
    Ok(())
}

fn parse_header(header: &str) -> Result<(i64, String), SignError> {
    let mut t: Option<i64> = None;
    let mut v1: Option<String> = None;
    for part in header.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("t=") {
            t = Some(rest.parse().map_err(|_| SignError::InvalidHeader)?);
        } else if let Some(rest) = part.strip_prefix("v1=") {
            if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(SignError::InvalidHeader);
            }
            v1 = Some(rest.to_ascii_lowercase());
        }
    }
    match (t, v1) {
        (Some(t), Some(v1)) => Ok((t, v1)),
        _ => Err(SignError::InvalidHeader),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_accepts() {
        let secret = b"whsec_test_secret";
        let body = br#"{"hello":"world"}"#;
        let now = 1_700_000_000_i64;
        let header = sign(secret, body, now).expect("sign");
        verify(secret, body, &header, now).expect("verify");
    }

    #[test]
    fn mismatch_rejects() {
        let secret = b"whsec_test_secret";
        let body = br#"{"hello":"world"}"#;
        let now = 1_700_000_000_i64;
        let header = sign(secret, body, now).expect("sign");
        let err = verify(b"other_secret", body, &header, now).unwrap_err();
        assert_eq!(err, SignError::Mismatch);
    }

    #[test]
    fn stale_rejects() {
        let secret = b"whsec_test_secret";
        let body = br#"{"hello":"world"}"#;
        let t = 1_700_000_000_i64;
        let header = sign(secret, body, t).expect("sign");
        let err = verify(secret, body, &header, t + MAX_SKEW_SECS + 1).unwrap_err();
        assert_eq!(err, SignError::Stale);
    }

    #[test]
    fn body_tamper_rejects() {
        let secret = b"whsec_test_secret";
        let now = 1_700_000_000_i64;
        let header = sign(secret, br#"{"a":1}"#, now).expect("sign");
        let err = verify(secret, br#"{"a":2}"#, &header, now).unwrap_err();
        assert_eq!(err, SignError::Mismatch);
    }
}
