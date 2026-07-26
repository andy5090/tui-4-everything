use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::installer::execution::InstallJob;

pub fn gate_report_path(root: &Path, gate_id: &str) -> PathBuf {
    root.join("artifacts")
        .join("gates")
        .join(format!("{}-report.json", gate_id))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentState {
    #[serde(default)]
    pub queue: Vec<InstallJob>,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub recents: Vec<RecentItem>,
    #[serde(default)]
    pub settings: UserSettings,
    #[serde(default)]
    pub launch_preferences: BTreeMap<String, BTreeMap<String, LaunchOptionPreference>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchOptionPreference {
    pub enabled: bool,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentItem {
    pub id: String,
    pub kind: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserSettings {
    pub default_mux: String,
    #[serde(default = "enabled_by_default")]
    pub mouse_enabled: bool,
    pub install_timeout_sec: u64,
    pub max_install_attempts: u32,
    pub confirm_all_installs: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            default_mux: "tmux".to_string(),
            mouse_enabled: true,
            install_timeout_sec: 600,
            max_install_attempts: 2,
            confirm_all_installs: false,
        }
    }
}

pub fn default_state_path() -> PathBuf {
    if let Some(root) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(root).join("t4e").join("state.json");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("t4e")
            .join("state.json");
    }
    PathBuf::from(".t4e-state.json")
}

pub fn load_state(path: &Path) -> Result<PersistentState> {
    if !path.exists() {
        return Ok(PersistentState::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read state file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse state file {}", path.display()))
}

pub fn save_state(path: &Path, state: &PersistentState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state directory {}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state)?;
    fs::write(&temp, bytes)
        .with_context(|| format!("failed to write temporary state {}", temp.display()))?;
    fs::rename(&temp, path)
        .with_context(|| format!("failed to replace state file {}", path.display()))?;
    Ok(())
}

pub fn log_dir_for_state(path: &Path) -> PathBuf {
    path.parent().unwrap_or_else(|| Path::new(".")).join("logs")
}
