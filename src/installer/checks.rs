use std::env;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckResult {
    pub command: String,
    pub installed: bool,
    pub resolved_path: Option<String>,
}

pub trait InstallChecker: Send + Sync + 'static {
    fn check(&self, command: &str) -> Result<CheckResult>;
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
