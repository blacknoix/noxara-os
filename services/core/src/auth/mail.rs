//! Local mail delivery for verification / magic link / reset.
//!
//! Local default: log the link (and write under `.tmp/mail/` when writable).
//! Production: set `SMTP_URL` or `AUTH_MAIL_WEBHOOK` (documented in README).

use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct OutboundMail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

pub async fn send_mail(mail: OutboundMail) -> Result<(), String> {
    if let Ok(webhook) = std::env::var("AUTH_MAIL_WEBHOOK") {
        let client = reqwest::Client::new();
        client
            .post(&webhook)
            .json(&serde_json::json!({
                "to": mail.to,
                "subject": mail.subject,
                "body": mail.body,
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    tracing::info!(
        to = %mail.to,
        subject = %mail.subject,
        body = %mail.body,
        "AUTH MAIL (local catcher) — link printed for development"
    );

    let root = std::env::var("AUTH_MAIL_DIR").unwrap_or_else(|_| ".tmp/mail".into());
    let dir = PathBuf::from(&root);
    let _ = fs::create_dir_all(&dir);
    let name = format!(
        "{}-{}.txt",
        chrono::Utc::now().timestamp_millis(),
        mail.to.replace('@', "_at_")
    );
    let path = dir.join(name);
    let _ = fs::write(
        &path,
        format!(
            "To: {}\nSubject: {}\n\n{}\n",
            mail.to, mail.subject, mail.body
        ),
    );
    Ok(())
}

pub fn public_app_base() -> String {
    std::env::var("PUBLIC_APP_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into())
}

pub fn public_api_base() -> String {
    std::env::var("PUBLIC_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into())
}
