use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};

use crate::catalog::models::{CatalogRegistry, InstallMethod};
use crate::mux::workspace::WorkspaceRegistry;

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
            if !matches!(installer.method, InstallMethod::Script) && installer.install_cmd.is_some()
            {
                bail!("tool {} has install_cmd on a non-script installer", tool.id);
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

pub fn validate_workspaces(catalog: &CatalogRegistry, registry: &WorkspaceRegistry) -> Result<()> {
    let tool_ids = catalog
        .tools
        .iter()
        .map(|tool| tool.id.as_str())
        .collect::<HashSet<_>>();
    let mut workspace_ids = HashSet::new();
    let mut session_names = HashSet::new();
    for workspace in &registry.workspaces {
        if !workspace_ids.insert(workspace.id.as_str()) {
            bail!("duplicate workspace id: {}", workspace.id);
        }
        if let Some(session) = &workspace.session_name {
            if !session_names.insert(session.as_str()) {
                bail!("duplicate workspace session name: {session}");
            }
            if session.is_empty()
                || !session
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
            {
                bail!("invalid workspace session name: {session}");
            }
        }
        for tool_id in &workspace.recommended_tools {
            if !tool_ids.contains(tool_id.as_str()) {
                bail!(
                    "workspace {} references unknown tool {}",
                    workspace.id,
                    tool_id
                );
            }
        }

        let mut pane_ids = HashSet::new();
        for pane in &workspace.layout.panes {
            if pane.split != "root" && !pane_ids.contains(pane.split.as_str()) {
                bail!(
                    "workspace {} pane {} references unavailable parent {}",
                    workspace.id,
                    pane.id,
                    pane.split
                );
            }
            if !pane_ids.insert(pane.id.as_str()) {
                bail!("workspace {} has duplicate pane {}", workspace.id, pane.id);
            }
            if pane.cmd.trim().is_empty() {
                bail!("workspace {} pane {} has no command", workspace.id, pane.id);
            }
            if pane
                .cmd
                .chars()
                .any(|ch| matches!(ch, ';' | '|' | '&' | '`' | '$' | '<' | '>' | '\n' | '\r'))
            {
                bail!(
                    "workspace {} pane {} contains forbidden shell syntax",
                    workspace.id,
                    pane.id
                );
            }
            let executable = pane.cmd.split_whitespace().next().unwrap_or_default();
            let recommended_executables = workspace
                .recommended_tools
                .iter()
                .filter_map(|tool_id| catalog.tools.iter().find(|tool| &tool.id == tool_id))
                .filter_map(|tool| tool.run.cmd.split_whitespace().next())
                .collect::<HashSet<_>>();
            if executable != "bash" && !recommended_executables.contains(executable) {
                bail!(
                    "workspace {} pane {} runs unapproved executable {}",
                    workspace.id,
                    pane.id,
                    executable
                );
            }
        }
    }
    Ok(())
}
