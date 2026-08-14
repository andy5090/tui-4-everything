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
    #[serde(default)]
    pub key_hints: Vec<String>,
    #[serde(default)]
    pub install_timeout_sec: Option<u64>,
    pub category: ToolCategory,
    #[serde(default)]
    pub tags: Vec<String>,
    pub audience: Audience,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default = "default_exposure")]
    pub exposure: Exposure,
    pub run: RunSpec,
    #[serde(default)]
    pub launch_argument: Option<LaunchArgument>,
    #[serde(default)]
    pub run_options: Vec<RunOption>,
    #[serde(default)]
    pub installers: Vec<Installer>,
    #[serde(default)]
    pub checks: Vec<Check>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchArgument {
    pub label: String,
    pub placeholder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub flag: String,
    #[serde(default)]
    pub output_filter: Option<OutputFilter>,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub default_enabled: bool,
    #[serde(default)]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputFilter {
    Lolcat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunSpec {
    pub cmd: String,
    #[serde(default)]
    pub keep_open: bool,
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
    #[serde(default)]
    pub verified_update: Option<VerifiedUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedUpdate {
    pub version: String,
    pub version_probe: VersionProbe,
    pub command: String,
    pub verified_at: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionProbe {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Check {
    pub which: Option<String>,
    pub version: Option<String>,
    pub custom: Option<String>,
}

impl Tool {
    /// Prefer a native Termux installer while retaining the legacy Linux plan
    /// until each catalog application has been migrated explicitly.
    pub fn installer_for(&self, platform: Platform) -> Option<&Installer> {
        self.installers
            .iter()
            .find(|installer| installer.platform == platform)
            .or_else(|| {
                if platform == Platform::Termux {
                    self.installers
                        .iter()
                        .find(|installer| installer.platform == Platform::Linux)
                } else {
                    None
                }
            })
    }

    pub fn is_launchable_app(&self) -> bool {
        !self.tags.iter().any(|tag| tag == "support")
    }

    /// Builtin apps ship inside the t4e executable and need no installation.
    pub fn is_builtin(&self) -> bool {
        self.installers
            .iter()
            .any(|installer| installer.method == InstallMethod::Builtin)
    }

    pub fn risk_level(&self) -> RiskLevel {
        self.capabilities
            .iter()
            .copied()
            .map(Capability::risk_level)
            .max()
            .unwrap_or(RiskLevel::Safe)
    }

    pub fn app_category(&self) -> AppCategory {
        match self.id.as_str() {
            "ascii-camera" | "btop" | "fastfetch" => AppCategory::System,
            "shellcast" | "spotatui" | "spotify-player" | "ncspot" | "cava" | "termusic"
            | "yewtube" | "youtube-tui" | "tplay" => AppCategory::Media,
            "newsboat" | "lynx" => AppCategory::Internet,
            "yazi" | "ncdu" | "broot" => AppCategory::Files,
            "termleaf" | "micro" | "helix" | "lazyvim" => AppCategory::Editors,
            "claude-code" | "codex-cli" | "opencode" => AppCategory::Ai,
            "bastet" | "ninvaders" | "nudoku" => AppCategory::Games,
            "cmatrix" | "asciiquarium" | "sl" | "lolcat" | "cowsay" | "fortune" | "tty-clock"
            | "big-clock" | "nyancat" | "pipes-sh" => AppCategory::Entertainment,
            _ => AppCategory::Utilities,
        }
    }

    pub fn run_command_for(&self, platform: Platform) -> String {
        if self.is_builtin() {
            return crate::builtin::launch_command(&self.run.cmd);
        }
        self.installer_for(platform)
            .and_then(|installer| installer.executable.clone())
            .unwrap_or_else(|| self.run.cmd.clone())
    }

    pub fn run_command_for_current_platform(&self) -> String {
        self.run_command_for(Platform::current())
    }

    pub fn install_check_commands(&self, platform: Platform) -> Vec<String> {
        let checks = self
            .checks
            .iter()
            .filter_map(|check| check.which.clone())
            .collect::<Vec<_>>();
        if !checks.is_empty() {
            return checks;
        }
        self.installer_for(platform)
            .and_then(|installer| installer.executable.clone())
            .or_else(|| {
                self.run_command_for(platform)
                    .split_whitespace()
                    .next()
                    .map(str::to_string)
            })
            .into_iter()
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    Internet,
    Media,
    Files,
    Editors,
    Ai,
    System,
    Utilities,
    Games,
    Entertainment,
}

impl AppCategory {
    pub const ALL: [Self; 9] = [
        Self::Internet,
        Self::Media,
        Self::Files,
        Self::Editors,
        Self::Ai,
        Self::System,
        Self::Utilities,
        Self::Games,
        Self::Entertainment,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Internet => "Internet",
            Self::Media => "Media",
            Self::Files => "Files",
            Self::Editors => "Editors",
            Self::Ai => "AI",
            Self::System => "System",
            Self::Utilities => "Utilities",
            Self::Games => "Games",
            Self::Entertainment => "Entertainment",
        }
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
    #[serde(rename = "labs")]
    Labs,
}

fn default_exposure() -> Exposure {
    Exposure::Starter
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Capability {
    Network,
    Account,
    CameraCapture,
    FileRead,
    FileWrite,
    Delete,
    System,
    Commands,
    Autonomous,
}

impl Capability {
    pub fn risk_level(self) -> RiskLevel {
        match self {
            Self::Network | Self::Account | Self::FileRead => RiskLevel::Low,
            Self::CameraCapture | Self::FileWrite | Self::Delete => RiskLevel::High,
            Self::System | Self::Commands | Self::Autonomous => RiskLevel::Danger,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Network => "NETWORK",
            Self::Account => "ACCOUNT",
            Self::CameraCapture => "CAMERA_CAPTURE",
            Self::FileRead => "FILE_READ",
            Self::FileWrite => "FILE_WRITE",
            Self::Delete => "DELETE",
            Self::System => "SYSTEM",
            Self::Commands => "COMMANDS",
            Self::Autonomous => "AUTONOMOUS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Safe,
    Low,
    High,
    Danger,
}

impl RiskLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Safe => "SAFE",
            Self::Low => "LOW",
            Self::High => "HIGH",
            Self::Danger => "DANGER",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    #[serde(rename = "macos")]
    Macos,
    #[serde(rename = "linux")]
    Linux,
    #[serde(rename = "termux")]
    Termux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "android") {
            Self::Termux
        } else {
            Self::Linux
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Termux => "termux",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstallMethod {
    #[serde(rename = "builtin")]
    Builtin,
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
    #[serde(rename = "xbps")]
    Xbps,
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
    #[serde(rename = "lazyvim")]
    LazyVim,
    #[serde(rename = "tplay")]
    Tplay,
    #[serde(rename = "youtube_tui")]
    YoutubeTui,
    #[serde(rename = "yewtube")]
    Yewtube,
    #[serde(rename = "ascii_camera")]
    AsciiCamera,
    #[serde(rename = "newsboat")]
    Newsboat,
    #[serde(rename = "fastfetch")]
    Fastfetch,
    #[serde(rename = "script")]
    Script,
    #[serde(other)]
    Other,
}

impl InstallMethod {
    pub fn channel_name(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Brew => "brew",
            Self::BrewCask => "brew_cask",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Xbps => "xbps",
            Self::Snap => "snap",
            Self::SnapClassic => "snap_classic",
            Self::Pipx => "pipx",
            Self::NpmGlobal => "npm_global",
            Self::Cargo => "cargo",
            Self::Go => "go",
            Self::LazyVim => "lazyvim",
            Self::Tplay => "tplay",
            Self::YoutubeTui => "youtube_tui",
            Self::Yewtube => "yewtube",
            Self::AsciiCamera => "ascii_camera",
            Self::Newsboat => "newsboat",
            Self::Fastfetch => "fastfetch",
            Self::Script => "script",
            Self::Other => "other",
        }
    }
}
