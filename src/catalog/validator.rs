use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};

use crate::catalog::models::{CatalogRegistry, InstallMethod};

pub fn validate_catalog(catalog: &CatalogRegistry) -> Result<()> {
    let mut tool_ids = HashSet::new();
    for tool in &catalog.tools {
        if !tool_ids.insert(tool.id.clone()) {
            bail!("duplicate tool id: {}", tool.id);
        }

        for installer in &tool.installers {
            if matches!(installer.method, InstallMethod::Script) && !installer.requires_confirm {
                bail!(
                    "tool {} has script installer without explicit confirmation",
                    tool.id
                );
            }
        }
    }

    let tool_index: HashMap<&str, _> = catalog.tools.iter().map(|t| (t.id.as_str(), t)).collect();
    for pack in &catalog.packs {
        for tool_id in &pack.tool_ids {
            if !tool_index.contains_key(tool_id.as_str()) {
                bail!("pack {} references unknown tool {}", pack.id, tool_id);
            }
        }
    }

    Ok(())
}
