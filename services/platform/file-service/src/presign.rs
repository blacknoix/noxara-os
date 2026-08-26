//! AWS Signature Version 4 query-string presign for MinIO path-style PutObject.
//!
//! Used when `MINIO_ENDPOINT` is set. When MinIO is unset, handlers return the
//! local-upload URL instead (see `handlers/local_upload.rs`).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// Build a path-style pre-signed PUT URL for MinIO / S3-compatible storage.
pub fn presign_put_object(
    endpoint: &str,
    bucket: &str,
    object_key: &str,
    access_key: &str,
    secret_key: &str,
    content_type: &str,
    expires_secs: u32,
) -> Result<String, String> {
    let endpoint = endpoint.trim_end_matches('/');
    let host = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint);
    let region = std::env::var("MINIO_REGION").unwrap_or_else(|_| "us-east-1".into());
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let credential_scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let credential = format!("{access_key}/{credential_scope}");

    let canonical_uri = format!(
        "/{}/{}",
        bucket,
        object_key
            .split('/')
            .map(urlencoding::encode)
            .collect::<Vec<_>>()
            .join("/")
    );

    let mut query = [
        (
            "X-Amz-Algorithm".to_string(),
            "AWS4-HMAC-SHA256".to_string(),
        ),
        ("X-Amz-Credential".to_string(), credential),
        ("X-Amz-Date".to_string(), amz_date.clone()),
        ("X-Amz-Expires".to_string(), expires_secs.to_string()),
        (
            "X-Amz-SignedHeaders".to_string(),
            "content-type;host".to_string(),
        ),
    ];
    query.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query = query
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!("content-type:{content_type}\nhost:{host}\n");
    let signed_headers = "content-type;host";
    let payload_hash = "UNSIGNED-PAYLOAD";
    let canonical_request = format!(
        "PUT\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let key = signing_key(secret_key, &date_stamp, &region, "s3");
    let signature = hex::encode(hmac_sha256(&key, string_to_sign.as_bytes()));

    Ok(format!(
        "{endpoint}{canonical_uri}?{canonical_query}&X-Amz-Signature={signature}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_url_with_signature() {
        let url = presign_put_object(
            "http://127.0.0.1:9000",
            "companyos-files",
            "org/uuid/file.pdf",
            "minioadmin",
            "minioadmin",
            "application/pdf",
            900,
        )
        .unwrap();
        assert!(url.contains("X-Amz-Signature="));
        assert!(url.contains("companyos-files"));
        assert!(url.starts_with("http://127.0.0.1:9000/"));
    }
}
