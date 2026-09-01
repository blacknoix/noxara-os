//! Pure desktop shell helpers (no Tauri / GUI) — unit-tested in Linux CI.

pub mod deep_link;
pub mod offline_cache;

/// Global copilot hotkey chord (⌥Space / Alt+Space).
pub const COPILOT_HOTKEY: &str = "Alt+Space";

/// Default web app URL wrapped by the native shell.
pub fn default_web_url() -> String {
    std::env::var("COMPANYOS_WEB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into())
}
