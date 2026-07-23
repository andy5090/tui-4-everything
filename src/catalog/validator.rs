use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};

use crate::catalog::models::{Capability, CatalogRegistry, InstallMethod, Platform, ToolCategory};
use crate::mux::workspace::WorkspaceRegistry;

pub fn validate_catalog(catalog: &CatalogRegistry) -> Result<()> {
    let mut tool_ids = HashSet::new();
    for tool in &catalog.tools {
        if !tool_ids.insert(tool.id.clone()) {
            bail!("duplicate tool id: {}", tool.id);
        }

        let unique_capabilities = tool.capabilities.iter().collect::<HashSet<_>>();
        if unique_capabilities.len() != tool.capabilities.len() {
            bail!("tool {} has duplicate capabilities", tool.id);
        }
        if tool.category == ToolCategory::Agents
            && (!tool.capabilities.contains(&Capability::Commands)
                || !tool.capabilities.contains(&Capability::Autonomous))
        {
            bail!(
                "agent tool {} must declare COMMANDS and AUTONOMOUS",
                tool.id
            );
        }

        if tool.run.cmd.trim().is_empty()
            || tool
                .run
                .cmd
                .chars()
                .any(|ch| matches!(ch, ';' | '|' | '&' | '`' | '$' | '<' | '>' | '\n' | '\r'))
        {
            bail!("tool {} has an unsafe run command", tool.id);
        }
        if tool
            .install_timeout_sec
            .is_some_and(|timeout| !(60..=7_200).contains(&timeout))
        {
            bail!("tool {} has an invalid install timeout", tool.id);
        }
        if tool.launch_argument.as_ref().is_some_and(|argument| {
            argument.label.trim().is_empty() || argument.placeholder.trim().is_empty()
        }) {
            bail!("tool {} has an invalid launch argument", tool.id);
        }
        if tool.key_hints.iter().any(|hint| hint.trim().is_empty()) {
            bail!("tool {} has an empty key hint", tool.id);
        }
        let mut option_ids = HashSet::new();
        for option in &tool.run_options {
            if !option_ids.insert(option.id.as_str()) {
                bail!("tool {} has duplicate run option {}", tool.id, option.id);
            }
            if option.label.trim().is_empty()
                || !is_safe_argument(&option.flag)
                || !option.flag.starts_with('-')
                || option.values.iter().any(|value| !is_safe_argument(value))
            {
                bail!("tool {} has an unsafe run option {}", tool.id, option.id);
            }
            if let Some(default) = &option.default_value
                && !option.values.contains(default)
            {
                bail!(
                    "tool {} option {} has an unknown default value",
                    tool.id,
                    option.id
                );
            }
        }
        for platform in [Platform::Macos, Platform::Linux] {
            let count = tool
                .installers
                .iter()
                .filter(|installer| installer.platform == platform)
                .count();
            if count != 1 {
                bail!(
                    "tool {} must define exactly one {:?} installer, found {}",
                    tool.id,
                    platform,
                    count
                );
            }
        }

        for installer in &tool.installers {
            if installer.package_hints.is_empty()
                || installer
                    .package_hints
                    .iter()
                    .any(|hint| !is_safe_argument(hint))
            {
                bail!("tool {} installer has invalid package hints", tool.id);
            }
            if installer
                .system_packages
                .iter()
                .any(|package| !is_safe_argument(package))
            {
                bail!("tool {} installer has invalid system packages", tool.id);
            }
            if !installer.system_packages.is_empty() && installer.platform != Platform::Linux {
                bail!(
                    "tool {} has system packages on a non-Linux installer",
                    tool.id
                );
            }
            if matches!(installer.method, InstallMethod::Script) && !installer.requires_confirm {
                bail!(
                    "tool {} has script installer without explicit confirmation",
                    tool.id
                );
            }
            if matches!(installer.method, InstallMethod::Script)
                && installer.install_cmd.as_deref().is_none_or(str::is_empty)
            {
                bail!("tool {} has a script installer without a command", tool.id);
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

fn is_safe_argument(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|ch| {
            ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '`' | '$' | '<' | '>' | '\'' | '"')
        })
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

        if workspace.layout.panes.is_empty() {
            bail!("workspace {} has no apps", workspace.id);
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
