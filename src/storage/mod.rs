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
    #[serde(default)]
    pub theme: AppTheme,
    #[serde(default = "enabled_by_default")]
    pub mouse_enabled: bool,
    pub install_timeout_sec: u64,
    pub max_install_attempts: u32,
    pub confirm_all_installs: bool,
    #[serde(default)]
    pub ai_approval_mode: AiApprovalMode,
    #[serde(default)]
    pub preferred_ai_provider: String,
    #[serde(default = "default_api_provider_profiles")]
    pub api_providers: BTreeMap<String, ApiProviderProfile>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppTheme {
    #[default]
    Default,
    Amber,
    GreenScreen,
}

impl AppTheme {
    pub const ALL: [Self; 3] = [Self::Default, Self::Amber, Self::GreenScreen];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Amber => "Amber",
            Self::GreenScreen => "Green Screen",
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiProviderProfile {
    #[serde(default)]
    pub auth_mode: ProviderAuthMode,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub api_key_env: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthMode {
    Subscription,
    #[default]
    ApiKey,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiApprovalMode {
    #[serde(alias = "all_bounded")]
    Bypass,
    #[default]
    #[serde(alias = "safe_only")]
    Auto,
    Ask,
}

impl AiApprovalMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bypass => "Bypass",
            Self::Auto => "Auto",
            Self::Ask => "Ask",
        }
    }
}

impl ProviderAuthMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Subscription => "Subscription",
            Self::ApiKey => "API key",
        }
    }
}

pub fn default_api_provider_profiles() -> BTreeMap<String, ApiProviderProfile> {
    BTreeMap::from([
        (
            "codex".to_string(),
            ApiProviderProfile {
                auth_mode: ProviderAuthMode::Subscription,
                label: "OpenAI API".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-5.2".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
            },
        ),
        (
            "claude".to_string(),
            ApiProviderProfile {
                auth_mode: ProviderAuthMode::Subscription,
                label: "Anthropic API".to_string(),
                base_url: "https://api.anthropic.com/v1".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                api_key_env: "ANTHROPIC_API_KEY".to_string(),
            },
        ),
        (
            "gemini".to_string(),
            ApiProviderProfile {
                auth_mode: ProviderAuthMode::Subscription,
                label: "Gemini API".to_string(),
                base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
                model: "gemini-3.5-flash".to_string(),
                api_key_env: "GEMINI_API_KEY".to_string(),
            },
        ),
        (
            "custom".to_string(),
            ApiProviderProfile {
                auth_mode: ProviderAuthMode::ApiKey,
                label: "Custom".to_string(),
                base_url: String::new(),
                model: String::new(),
                api_key_env: "OPENAI_COMPAT_API_KEY".to_string(),
            },
        ),
        (
            "kimi".to_string(),
            ApiProviderProfile {
                auth_mode: ProviderAuthMode::ApiKey,
                label: "Kimi".to_string(),
                base_url: "https://api.moonshot.ai/v1".to_string(),
                model: "kimi-k3".to_string(),
                api_key_env: "MOONSHOT_API_KEY".to_string(),
            },
        ),
        (
            "zhipu".to_string(),
            ApiProviderProfile {
                auth_mode: ProviderAuthMode::ApiKey,
                label: "Zhipu AI".to_string(),
                base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
                model: "glm-5.2".to_string(),
                api_key_env: "ZHIPU_API_KEY".to_string(),
            },
        ),
    ])
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            default_mux: "tmux".to_string(),
            theme: AppTheme::Default,
            mouse_enabled: true,
            install_timeout_sec: 600,
            max_install_attempts: 2,
            confirm_all_installs: false,
            ai_approval_mode: AiApprovalMode::Auto,
            preferred_ai_provider: String::new(),
            api_providers: default_api_provider_profiles(),
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
