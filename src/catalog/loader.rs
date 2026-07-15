use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::catalog::models::CatalogRegistry;
use crate::mux::workspace::WorkspaceRegistry;

pub const DEFAULT_CATALOG_PATH: &str = "registry/catalog.yaml";
pub const DEFAULT_WORKSPACES_PATH: &str = "registry/workspaces.yaml";

const EMBEDDED_CATALOG: &str = include_str!("../../registry/catalog.yaml");
const EMBEDDED_WORKSPACES: &str = include_str!("../../registry/workspaces.yaml");

pub fn load_catalog(path: &Path) -> Result<CatalogRegistry> {
    let (raw, source) = read_or_embedded(path, DEFAULT_CATALOG_PATH, EMBEDDED_CATALOG)?;
    serde_yaml::from_str::<CatalogRegistry>(&raw)
        .with_context(|| format!("failed to parse catalog yaml: {source}"))
}

pub fn load_workspaces(path: &Path) -> Result<WorkspaceRegistry> {
    let (raw, source) = read_or_embedded(path, DEFAULT_WORKSPACES_PATH, EMBEDDED_WORKSPACES)?;
    serde_yaml::from_str::<WorkspaceRegistry>(&raw)
        .with_context(|| format!("failed to parse workspace yaml: {source}"))
}

fn read_or_embedded(path: &Path, default_path: &str, embedded: &str) -> Result<(String, String)> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok((raw, path.display().to_string())),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound && path == Path::new(default_path) =>
        {
            Ok((embedded.to_string(), format!("embedded:{default_path}")))
        }
        Err(error) => {
            Err(error).with_context(|| format!("failed to read registry file: {}", path.display()))
        }
    }
}
