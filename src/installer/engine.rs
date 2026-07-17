use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::catalog::models::{InstallMethod, Installer, Risk, Tool};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallTask {
    pub tool_id: String,
    pub method: InstallMethod,
    pub command: String,
    #[serde(default)]
    pub check_command: Option<String>,
    #[serde(default)]
    pub additional_check_commands: Vec<String>,
    #[serde(default)]
    pub install_timeout_sec: Option<u64>,
    #[serde(default)]
    pub requires_privileges: bool,
    pub requires_confirmation: bool,
    pub queued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallPolicy {
    pub enforce_script_confirmation: bool,
}

impl InstallTask {
    pub fn effective_timeout_sec(&self, configured_timeout_sec: u64) -> u64 {
        let method_default = if self.method == InstallMethod::Cargo {
            configured_timeout_sec.max(1_800)
        } else {
            configured_timeout_sec
        };
        self.install_timeout_sec
            .map_or(method_default, |timeout| method_default.max(timeout))
    }

    pub fn check_commands(&self) -> impl Iterator<Item = &str> {
        self.check_command
            .iter()
            .map(String::as_str)
            .chain(self.additional_check_commands.iter().map(String::as_str))
    }
}

impl Default for InstallPolicy {
    fn default() -> Self {
        Self {
            enforce_script_confirmation: true,
        }
    }
}

pub fn build_install_task(
    tool: &Tool,
    installer: &Installer,
    policy: &InstallPolicy,
) -> Result<InstallTask> {
    let command = materialize_command(installer)?;

    let requires_confirmation = match installer.method {
        InstallMethod::Script => {
            let _ = policy.enforce_script_confirmation;
            true
        }
        _ => installer.requires_confirm || matches!(tool.risk, Risk::Admin | Risk::High),
    };

    if matches!(installer.method, InstallMethod::Script) && !requires_confirmation {
        bail!("script installer for {} must require confirmation", tool.id);
    }

    let mut check_commands = tool.install_check_commands(installer.platform.clone());
    let check_command = (!check_commands.is_empty()).then(|| check_commands.remove(0));
    Ok(InstallTask {
        tool_id: tool.id.clone(),
        method: installer.method.clone(),
        command,
        check_command,
        additional_check_commands: check_commands,
        install_timeout_sec: tool.install_timeout_sec,
        requires_privileges: !installer.system_packages.is_empty(),
        requires_confirmation,
        queued_at: Utc::now(),
    })
}

fn materialize_command(installer: &Installer) -> Result<String> {
    if matches!(installer.method, InstallMethod::Script) {
        return installer
            .install_cmd
            .clone()
            .ok_or_else(|| anyhow::anyhow!("script installer requires explicit install_cmd"));
    }
    if installer.install_cmd.is_some() {
        bail!("install_cmd is only allowed for script installers");
    }

    if installer.package_hints.is_empty() {
        bail!("installer has no package hint");
    }
    for hint in &installer.package_hints {
        validate_package_hint(hint)?;
    }
    for package in &installer.system_packages {
        validate_package_hint(package)?;
    }
    let hint = &installer.package_hints[0];

    let command = match installer.method {
        InstallMethod::Brew => format!("brew install {}", hint),
        InstallMethod::BrewCask => format!("brew install --cask {}", hint),
        InstallMethod::Apt => format!(
            "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y {}",
            hint
        ),
        InstallMethod::Dnf => format!("sudo -n dnf install -y {}", hint),
        InstallMethod::Pacman => format!("sudo -n pacman -S --noconfirm {}", hint),
        InstallMethod::Snap => format!("sudo -n snap install {}", hint),
        InstallMethod::SnapClassic => format!("sudo -n snap install --classic {}", hint),
        InstallMethod::Pipx => format!(
            "command -v pipx >/dev/null 2>&1 || sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y pipx; pipx install {}",
            hint
        ),
        InstallMethod::NpmGlobal => format!("npm install -g {}", hint),
        InstallMethod::Cargo => format!(
            "cargo install --locked {}",
            installer.package_hints.join(" ")
        ),
        InstallMethod::Go => format!("go install {}", hint),
        InstallMethod::Script => unreachable!("script installers return before package handling"),
        InstallMethod::Other => {
            return Err(anyhow::anyhow!("unsupported install method"));
        }
    };

    if installer.system_packages.is_empty() {
        Ok(command)
    } else if installer.platform == crate::catalog::models::Platform::Linux {
        Ok(format!(
            "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y {} && {}",
            installer.system_packages.join(" "),
            command
        ))
    } else {
        bail!("system_packages are only supported by Linux installers")
    }
}

fn validate_package_hint(hint: &str) -> Result<()> {
    let is_valid = hint
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-._@+/".contains(ch));
    if !is_valid {
        bail!("unsafe package hint: {}", hint);
    }
    Ok(())
}
