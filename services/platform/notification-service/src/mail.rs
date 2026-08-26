//! Local email catcher (AUTH_MAIL_DIR / stdout) — mirrors core auth mail.

use std::fs;
use std::path::PathBuf;

pub async fn send_email(to: &str, subject: &str, body: &str) -> Result<(), String> {
    tracing::info!(
        to = %to,
        subject = %subject,
        body = %body,
        "NOTIFICATION EMAIL (local catcher)"
    );

    let root = std::env::var("AUTH_MAIL_DIR").unwrap_or_else(|_| ".tmp/mail".into());
    let dir = PathBuf::from(&root);
    let _ = fs::create_dir_all(&dir);
    let name = format!(
        "notif-{}-{}.txt",
        chrono::Utc::now().timestamp_millis(),
        to.replace('@', "_at_")
    );
    let path = dir.join(name);
    let _ = fs::write(&path, format!("To: {to}\nSubject: {subject}\n\n{body}\n"));
    Ok(())
}
