//! HTTP activity stubs — call domain APIs with on_behalf_of headers.

use tracing::info;

/// Call an HTTP API on behalf of a user (workflow activity stub).
pub async fn http_on_behalf_of(
    base_url: &str,
    path: &str,
    org_public: &str,
    actor_public: &str,
) -> anyhow::Result<u16> {
    let client = reqwest::Client::new();
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    info!(%url, %org_public, %actor_public, "workflow activity HTTP call (stub)");
    let resp = client
        .post(&url)
        .header("x-companyos-dev-org-id", org_public)
        .header("x-companyos-dev-user-id", actor_public)
        .header("x-companyos-on-behalf-of", actor_public)
        .send()
        .await?;
    Ok(resp.status().as_u16())
}
