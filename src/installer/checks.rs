use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::catalog::models::VersionProbe;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckResult {
    pub command: String,
    pub installed: bool,
    pub resolved_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionCheckResult {
    pub command: String,
    pub installed: bool,
    pub resolved_path: Option<String>,
    pub reported_version: Option<String>,
    pub normalized_version: Option<String>,
}

pub trait InstallChecker: Send + Sync + 'static {
    fn check(&self, command: &str) -> Result<CheckResult>;
    fn probe_version(&self, probe: &VersionProbe) -> Result<VersionCheckResult>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemInstallChecker;

impl InstallChecker for SystemInstallChecker {
    fn check(&self, command: &str) -> Result<CheckResult> {
        let resolved = resolve_in_path(command);
        Ok(CheckResult {
            command: command.to_string(),
            installed: resolved.is_some(),
            resolved_path: resolved.map(|path| path.display().to_string()),
        })
    }

    fn probe_version(&self, probe: &VersionProbe) -> Result<VersionCheckResult> {
        let command = probe_command(probe);
        let Some(resolved_path) = resolve_in_path(&probe.executable) else {
            return Ok(VersionCheckResult {
                command,
                installed: false,
                resolved_path: None,
                reported_version: None,
                normalized_version: None,
            });
        };

        let output = Command::new(&resolved_path)
            .args(&probe.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reported_version = output
            .status
            .success()
            .then(|| format!("{stdout}{stderr}"))
            .and_then(|raw| {
                let trimmed = raw.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            });
        let normalized_version = reported_version.as_deref().and_then(normalize_version);

        Ok(VersionCheckResult {
            command,
            installed: true,
            resolved_path: Some(resolved_path.display().to_string()),
            reported_version,
            normalized_version,
        })
    }
}

pub fn normalize_version(value: &str) -> Option<String> {
    value
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    ',' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\''
                )
            })
        })
        .map(
            |token| match token.strip_prefix('v').or_else(|| token.strip_prefix('V')) {
                Some(stripped)
                    if stripped
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_digit()) =>
                {
                    stripped
                }
                _ => token,
            },
        )
        .find(|token| {
            !token.is_empty()
                && token.chars().any(|ch| ch.is_ascii_digit())
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '+'))
        })
        .map(str::to_string)
}

fn probe_command(probe: &VersionProbe) -> String {
    std::iter::once(probe.executable.as_str())
        .chain(probe.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn resolve_in_path(command: &str) -> Option<PathBuf> {
    if command.contains('/') {
        let path = PathBuf::from(command);
        return is_executable(&path).then_some(path);
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(command))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_file()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
