//! Tauri commands bridging to `companyos-desktop-shell`.

use companyos_desktop_shell::deep_link::{open_in_org, DeepLink, DeepLinkNavigation};
use companyos_desktop_shell::offline_cache::{load_dashboard, save_dashboard, CachedDashboard};
use companyos_desktop_shell::{default_web_url, COPILOT_HOTKEY};
use serde_json::Value;
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("companyos-desktop")
}

#[tauri::command]
pub fn get_web_url() -> String {
    default_web_url()
}

#[tauri::command]
pub fn copilot_hotkey() -> String {
    COPILOT_HOTKEY.to_string()
}

#[tauri::command]
pub fn load_cached_dashboard() -> Option<CachedDashboard> {
    load_dashboard(&cache_dir()).ok().flatten()
}

#[tauri::command]
pub fn save_cached_dashboard(org_id: Option<String>, payload: Value) -> Result<(), String> {
    let dash = CachedDashboard {
        as_of_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        org_id,
        payload,
    };
    save_dashboard(&cache_dir(), &dash).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn parse_deep_link(uri: String) -> Result<DeepLink, String> {
    DeepLink::parse(&uri).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_deep_link(
    uri: String,
    current_org_id: Option<String>,
) -> Result<DeepLinkNavigation, String> {
    resolve_url(&uri, current_org_id.as_deref()).map_err(|e| e.to_string())
}

pub fn resolve_url(
    uri: &str,
    current_org_id: Option<&str>,
) -> Result<DeepLinkNavigation, companyos_desktop_shell::deep_link::DeepLinkError> {
    let link = DeepLink::parse(uri)?;
    open_in_org(&link, current_org_id)
}
