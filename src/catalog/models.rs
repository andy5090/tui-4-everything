use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogRegistry {
    pub packs: Vec<Pack>,
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pack {
    pub id: String,
    pub title: String,
    #[serde(default = "default_exposure")]
    pub exposure: Exposure,
    pub tool_ids: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tool {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub category: ToolCategory,
    #[serde(default)]
    pub tags: Vec<String>,
    pub audience: Audience,
    pub risk: Risk,
    #[serde(default = "default_exposure")]
    pub exposure: Exposure,
    pub run: RunSpec,
    #[serde(default)]
    pub run_options: Vec<RunOption>,
    #[serde(default)]
    pub installers: Vec<Installer>,
    #[serde(default)]
    pub checks: Vec<Check>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunOption {
    pub id: String,
    pub label: String,
    pub flag: String,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub default_enabled: bool,
    #[serde(default)]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunSpec {
    pub cmd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Installer {
    pub platform: Platform,
    pub method: InstallMethod,
    #[serde(default)]
    pub package_hints: Vec<String>,
    #[serde(default)]
    pub system_packages: Vec<String>,
    #[serde(default)]
    pub executable: Option<String>,
    pub install_cmd: Option<String>,
    #[serde(default)]
    pub requires_confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Check {
    pub which: Option<String>,
    pub version: Option<String>,
    pub custom: Option<String>,
}

impl Tool {
    pub fn is_launchable_app(&self) -> bool {
        !self.tags.iter().any(|tag| tag == "support")
    }

    pub fn run_command_for(&self, platform: Platform) -> &str {
        self.installers
            .iter()
            .find(|installer| installer.platform == platform)
            .and_then(|installer| installer.executable.as_deref())
            .unwrap_or(&self.run.cmd)
    }

    pub fn run_command_for_current_platform(&self) -> &str {
        self.run_command_for(if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Linux
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Entertainment,
    Files,
    Fun,
    Edit,
    Utility,
    Agents,
    Reading,
    Ide,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    General,
    Prosumer,
    Developer,
    Ops,
    Security,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Exposure {
    #[serde(rename = "starter")]
    Starter,
    #[serde(rename = "search_only")]
    SearchOnly,
    #[serde(rename = "labs")]
    Labs,
}

fn default_exposure() -> Exposure {
    Exposure::Starter
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Risk {
    #[serde(rename = "SAFE")]
    Safe,
    #[serde(rename = "CAUTION")]
    Caution,
    #[serde(rename = "ADMIN")]
    Admin,
    #[serde(rename = "HIGH")]
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    #[serde(rename = "macos")]
    Macos,
    #[serde(rename = "linux")]
    Linux,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstallMethod {
    #[serde(rename = "brew")]
    Brew,
    #[serde(rename = "brew_cask")]
    BrewCask,
    #[serde(rename = "apt")]
    Apt,
    #[serde(rename = "dnf")]
    Dnf,
    #[serde(rename = "pacman")]
    Pacman,
    #[serde(rename = "snap")]
    Snap,
    #[serde(rename = "snap_classic")]
    SnapClassic,
    #[serde(rename = "pipx")]
    Pipx,
    #[serde(rename = "npm_global")]
    NpmGlobal,
    #[serde(rename = "cargo")]
    Cargo,
    #[serde(rename = "go")]
    Go,
    #[serde(rename = "script")]
    Script,
    #[serde(other)]
    Other,
}

impl InstallMethod {
    pub fn channel_name(&self) -> &'static str {
        match self {
            Self::Brew => "brew",
            Self::BrewCask => "brew_cask",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Snap => "snap",
            Self::SnapClassic => "snap_classic",
            Self::Pipx => "pipx",
            Self::NpmGlobal => "npm_global",
            Self::Cargo => "cargo",
            Self::Go => "go",
            Self::Script => "script",
            Self::Other => "other",
        }
    }
}
