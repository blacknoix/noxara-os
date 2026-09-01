//! Offline shell cache for the last dashboard snapshot.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedDashboard {
    pub as_of_ms: u64,
    pub org_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn cache_path(base_dir: &Path) -> PathBuf {
    base_dir.join("companyos-desktop-dashboard-cache.json")
}

pub fn save_dashboard(base_dir: &Path, dash: &CachedDashboard) -> Result<PathBuf, CacheError> {
    fs::create_dir_all(base_dir)?;
    let path = cache_path(base_dir);
    fs::write(&path, serde_json::to_vec_pretty(dash)?)?;
    Ok(path)
}

pub fn load_dashboard(base_dir: &Path) -> Result<Option<CachedDashboard>, CacheError> {
    let path = cache_path(base_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_cached_dashboard() {
        let dir = tempfile::tempdir().unwrap();
        let dash = CachedDashboard {
            as_of_ms: 1_700_000_000_000,
            org_id: Some("org_acme".into()),
            payload: json!({"widgets":[{"id":"revenue"}]}),
        };
        save_dashboard(dir.path(), &dash).unwrap();
        let loaded = load_dashboard(dir.path()).unwrap().unwrap();
        assert_eq!(loaded, dash);
    }

    #[test]
    fn missing_cache_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_dashboard(dir.path()).unwrap().is_none());
    }
}
