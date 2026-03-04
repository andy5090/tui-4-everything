use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::catalog::models::{InstallMethod, Installer, Risk, Tool};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallTask {
    pub tool_id: String,
    pub method: InstallMethod,
    pub command: String,
    pub requires_confirmation: bool,
    pub queued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallPolicy {
    pub enforce_script_confirmation: bool,
}

impl Default for InstallPolicy {
    fn default() -> Self {
        Self {
            enforce_script_confirmation: true,
        }
    }
}

pub fn build_install_task(tool: &Tool, installer: &Installer, policy: &InstallPolicy) -> Result<InstallTask> {
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

    Ok(InstallTask {
        tool_id: tool.id.clone(),
        method: installer.method.clone(),
        command,
        requires_confirmation,
        queued_at: Utc::now(),
    })
}

fn materialize_command(installer: &Installer) -> Result<String> {
    if let Some(cmd) = &installer.install_cmd {
        return Ok(cmd.clone());
    }

    let hint = installer
        .package_hints
        .first()
        .ok_or_else(|| anyhow::anyhow!("installer has no package hint"))?;
    validate_package_hint(hint)?;

    let command = match installer.method {
        InstallMethod::Brew => format!("brew install {}", hint),
        InstallMethod::BrewCask => format!("brew install --cask {}", hint),
        InstallMethod::Apt => format!("DEBIAN_FRONTEND=noninteractive apt-get install -y {}", hint),
        InstallMethod::Dnf => format!("dnf install -y {}", hint),
        InstallMethod::Pacman => format!("pacman -S --noconfirm {}", hint),
        InstallMethod::Pipx => format!("pipx install {}", hint),
        InstallMethod::NpmGlobal => format!("npm install -g {}", hint),
        InstallMethod::Cargo => format!("cargo install {}", hint),
        InstallMethod::Go => format!("go install {}", hint),
        InstallMethod::Script => {
            return Err(anyhow::anyhow!(
                "script installer requires explicit install_cmd in manifest"
            ));
        }
        InstallMethod::Other => {
            return Err(anyhow::anyhow!("unsupported install method"));
        }
    };

    Ok(command)
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
