use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::catalog::models::CatalogRegistry;
use crate::mux::workspace::WorkspaceRegistry;

pub fn load_catalog(path: &Path) -> Result<CatalogRegistry> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read catalog file: {}", path.display()))?;
    let parsed = serde_yaml::from_str::<CatalogRegistry>(&raw)
        .with_context(|| format!("failed to parse catalog yaml: {}", path.display()))?;
    Ok(parsed)
}

pub fn load_workspaces(path: &Path) -> Result<WorkspaceRegistry> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read workspace file: {}", path.display()))?;
    let parsed = serde_yaml::from_str::<WorkspaceRegistry>(&raw)
        .with_context(|| format!("failed to parse workspace yaml: {}", path.display()))?;
    Ok(parsed)
}
