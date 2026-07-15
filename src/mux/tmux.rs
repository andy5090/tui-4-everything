use std::collections::HashMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mux::workspace::{Pane, SplitDirection, Workspace};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompileOutput {
    pub commands: Vec<String>,
    pub focus_target: String,
}

pub fn compile_workspace(
    workspace: &Workspace,
    session_name: &str,
    window_name: &str,
) -> Result<CompileOutput> {
    validate_identifier("session_name", session_name)?;
    validate_identifier("window_name", window_name)?;

    let mut commands = Vec::new();
    commands.push(format!(
        "tmux new-session -d -s {} -n {} \"bash\"",
        shell_quote(session_name),
        shell_quote(window_name)
    ));

    let mut pane_map = HashMap::new();
    pane_map.insert(
        "root".to_string(),
        format!("{}:{}.0", session_name, window_name),
    );

    let mut last_pane_var = String::new();
    for pane in &workspace.layout.panes {
        compile_pane(&mut commands, &mut pane_map, pane)?;
        last_pane_var = format!("${{PANE_{}}}", sanitize_var(&pane.id));
    }

    let focus_target = if last_pane_var.is_empty() {
        pane_map
            .get("root")
            .cloned()
            .unwrap_or_else(|| format!("{}:{}.0", session_name, window_name))
    } else {
        last_pane_var
    };

    commands.push(format!("tmux select-pane -t {}", focus_target));

    Ok(CompileOutput {
        commands,
        focus_target,
    })
}

fn compile_pane(
    commands: &mut Vec<String>,
    pane_map: &mut HashMap<String, String>,
    pane: &Pane,
) -> Result<()> {
    let parent_key = pane.split.as_str();
    let Some(parent_target) = pane_map.get(parent_key) else {
        bail!(
            "pane {} references unknown parent {}; parent must be created earlier",
            pane.id,
            parent_key
        );
    };

    let size_percent = parse_percent_size(&pane.size)
        .map_err(|_| anyhow::anyhow!("invalid pane size {}", pane.size))?;

    let split_flags = match pane.direction {
        SplitDirection::Left => "-h -b",
        SplitDirection::Right => "-h",
        SplitDirection::Up => "-v -b",
        SplitDirection::Down => "-v",
    };

    let pane_var = format!("PANE_{}", sanitize_var(&pane.id));
    commands.push(format!(
        "{}=$(tmux split-window {} -l {}% -P -F \"#{{pane_id}}\" -t {})",
        pane_var, split_flags, size_percent, parent_target
    ));
    commands.push(format!(
        "tmux send-keys -t ${{{}}} -- {} C-m",
        pane_var,
        shell_quote(&pane.cmd)
    ));

    pane_map.insert(pane.id.clone(), format!("${{{}}}", pane_var));
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub window_index: usize,
    pub window_name: String,
    pub window_layout: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub window_index: usize,
    pub pane_index: usize,
    pub pane_width: usize,
    pub pane_height: usize,
    pub pane_start_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandLog {
    pub window_index: usize,
    pub pane_index: usize,
    pub sequence: usize,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReproSnapshot {
    pub windows: Vec<WindowSnapshot>,
    pub panes: Vec<PaneSnapshot>,
    pub commands: Vec<CommandLog>,
}

pub fn reproducibility_hash(snapshot: &ReproSnapshot, workspace_root: &str) -> String {
    let mut canonical = Vec::new();

    let mut windows = snapshot.windows.clone();
    windows.sort_by_key(|w| w.window_index);
    for window in windows {
        canonical.push(format!(
            "W|{}|{}|{}",
            window.window_index,
            normalize_text(&window.window_name, workspace_root),
            normalize_tmux_layout(&window.window_layout, workspace_root)
        ));
    }

    let mut panes = snapshot.panes.clone();
    panes.sort_by_key(|p| (p.window_index, p.pane_index));
    for pane in panes {
        canonical.push(format!(
            "P|{}|{}|{}x{}|{}",
            pane.window_index,
            pane.pane_index,
            pane.pane_width,
            pane.pane_height,
            normalize_text(&pane.pane_start_command, workspace_root)
        ));
    }

    let mut commands = snapshot.commands.clone();
    commands.sort_by_key(|c| c.sequence);
    for command in commands {
        canonical.push(format!(
            "C|{}|{}|{}|{}",
            command.window_index,
            command.pane_index,
            command.sequence,
            normalize_text(&command.command, workspace_root)
        ));
    }

    let text = canonical.join("\n");
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn normalize_text(input: &str, workspace_root: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "<none>".to_string();
    }

    let normalized_ws = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized_ws.replace(workspace_root, "$WORKSPACE_ROOT")
}

fn normalize_tmux_layout(input: &str, workspace_root: &str) -> String {
    let normalized = normalize_text(input, workspace_root);
    let layout = normalized
        .split_once(',')
        .map_or(normalized.as_str(), |(_, layout)| layout);
    let chars = layout.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        let previous = start.checked_sub(1).and_then(|value| chars.get(value));
        let next = chars.get(index);
        if previous == Some(&'x') || next == Some(&'x') {
            output.extend(chars[start..index].iter());
        } else if !output.ends_with('#') {
            output.push('#');
        }
    }
    output
}

fn sanitize_var(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>()
        .to_ascii_uppercase()
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    let ok = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
    if !ok || value.is_empty() {
        bail!("invalid {}: {}", label, value);
    }
    Ok(())
}

fn parse_percent_size(value: &str) -> Result<u8> {
    if !value.ends_with('%') {
        bail!("size must end with %");
    }

    let number = &value[..value.len() - 1];
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("size must be numeric");
    }
    if number.len() > 1 && number.starts_with('0') {
        bail!("size cannot have leading zero");
    }

    let parsed = number.parse::<u8>()?;
    if parsed == 0 || parsed > 100 {
        bail!("size must be within 1..=100");
    }
    Ok(parsed)
}

fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{}'", escaped)
}
