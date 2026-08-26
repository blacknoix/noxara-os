//! Content-type / size allowlist.

pub const MAX_SIZE_BYTES: i64 = 10 * 1024 * 1024; // 10 MiB

pub fn allowed_content_type(ct: &str) -> bool {
    matches!(
        ct.to_ascii_lowercase().as_str(),
        "application/pdf" | "image/png" | "image/jpeg" | "image/webp"
    )
}

pub fn validate_upload(content_type: &str, size_bytes: i64) -> Result<(), String> {
    if size_bytes <= 0 || size_bytes > MAX_SIZE_BYTES {
        return Err(format!(
            "size_bytes must be 1..={MAX_SIZE_BYTES} (10MB max)"
        ));
    }
    if !allowed_content_type(content_type) {
        return Err(format!(
            "content_type '{content_type}' not allowed (pdf, png, jpeg, webp)"
        ));
    }
    Ok(())
}
