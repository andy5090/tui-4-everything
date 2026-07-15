use std::collections::HashMap;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::installer::checks::{InstallChecker, SystemInstallChecker};

use super::tmux::{CommandLog, PaneSnapshot, ReproSnapshot, WindowSnapshot};
use super::workspace::{MuxBackend, SplitDirection, Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait TmuxRunner: Send + Sync + 'static {
    fn run(&self, args: &[String]) -> Result<TmuxOutput>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTmuxRunner;

impl TmuxRunner for SystemTmuxRunner {
    fn run(&self, args: &[String]) -> Result<TmuxOutput> {
        let output = Command::new("tmux")
            .args(args)
            .output()
            .with_context(|| format!("failed to run tmux {}", args.join(" ")))?;
        Ok(TmuxOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePreflight {
    pub tmux_available: bool,
    pub missing_commands: Vec<String>,
}

impl WorkspacePreflight {
    pub fn ready(&self) -> bool {
        self.tmux_available && self.missing_commands.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedSession {
    pub name: String,
    pub workspace_id: String,
    pub attached_clients: usize,
    pub windows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchOutcome {
    pub workspace_id: String,
    pub session_name: String,
    pub created: bool,
    pub pane_ids: Vec<String>,
}

pub fn attach_interactive(session_name: &str) -> Result<()> {
    validate_identifier("session name", session_name)?;
    let status = Command::new("tmux")
        .args(["attach-session", "-t", session_name])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to start interactive tmux attach")?;
    if !status.success() {
        bail!("tmux attach failed with status {status}");
    }
    Ok(())
}

pub struct TmuxRuntime<R = SystemTmuxRunner> {
    runner: R,
}

impl Default for TmuxRuntime<SystemTmuxRunner> {
    fn default() -> Self {
        Self::new(SystemTmuxRunner)
    }
}

impl<R: TmuxRunner> TmuxRuntime<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    pub fn preflight(&self, required_commands: &[String]) -> Result<WorkspacePreflight> {
        let checker = SystemInstallChecker;
        let tmux_available = checker.check("tmux")?.installed;
        let mut missing_commands = Vec::new();
        for command in required_commands {
            if !checker.check(command)?.installed {
                missing_commands.push(command.clone());
            }
        }
        missing_commands.sort();
        missing_commands.dedup();
        Ok(WorkspacePreflight {
            tmux_available,
            missing_commands,
        })
    }

    pub fn session_exists(&self, session_name: &str) -> Result<bool> {
        validate_identifier("session name", session_name)?;
        Ok(self
            .runner
            .run(&strings(["has-session", "-t", session_name]))?
            .success)
    }

    pub fn launch(&self, workspace: &Workspace) -> Result<LaunchOutcome> {
        if !matches!(workspace.mux, MuxBackend::Tmux) {
            bail!("workspace {} does not use tmux", workspace.id);
        }
        let session_name = workspace
            .session_name
            .as_deref()
            .unwrap_or(workspace.id.as_str());
        validate_identifier("session name", session_name)?;
        validate_identifier("workspace id", &workspace.id)?;
        if self.session_exists(session_name)? {
            return Ok(LaunchOutcome {
                workspace_id: workspace.id.clone(),
                session_name: session_name.to_string(),
                created: false,
                pane_ids: Vec::new(),
            });
        }

        let create = self.runner.run(&strings([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-n",
            "main",
            "bash",
        ]))?;
        ensure_success("create session", &create)?;

        match self.configure_workspace(workspace, session_name) {
            Ok(pane_ids) => Ok(LaunchOutcome {
                workspace_id: workspace.id.clone(),
                session_name: session_name.to_string(),
                created: true,
                pane_ids,
            }),
            Err(error) => {
                let _ = self
                    .runner
                    .run(&strings(["kill-session", "-t", session_name]));
                Err(error)
            }
        }
    }

    fn configure_workspace(&self, workspace: &Workspace, session: &str) -> Result<Vec<String>> {
        let marker = self.runner.run(&strings([
            "set-option",
            "-t",
            session,
            "@t4e_workspace",
            workspace.id.as_str(),
        ]))?;
        ensure_success("mark managed session", &marker)?;

        let mut targets = HashMap::from([("root".to_string(), format!("{session}:main.0"))]);
        let mut pane_ids = Vec::new();
        for pane in &workspace.layout.panes {
            let parent = targets.get(&pane.split).ok_or_else(|| {
                anyhow::anyhow!("pane {} references unknown parent {}", pane.id, pane.split)
            })?;
            let (orientation, before) = match pane.direction {
                SplitDirection::Left => ("-h", true),
                SplitDirection::Right => ("-h", false),
                SplitDirection::Up => ("-v", true),
                SplitDirection::Down => ("-v", false),
            };
            validate_percent(&pane.size)?;
            let mut args = strings(["split-window", orientation]);
            if before {
                args.push("-b".to_string());
            }
            args.extend(strings([
                "-l",
                pane.size.as_str(),
                "-P",
                "-F",
                "#{pane_id}",
                "-t",
                parent,
            ]));
            let split = self.runner.run(&args)?;
            ensure_success(&format!("split pane {}", pane.id), &split)?;
            let pane_id = split.stdout.trim().to_string();
            if pane_id.is_empty() {
                bail!("tmux returned no pane id for {}", pane.id);
            }
            let send = self.runner.run(&strings([
                "send-keys",
                "-t",
                pane_id.as_str(),
                "--",
                pane.cmd.as_str(),
                "C-m",
            ]))?;
            ensure_success(&format!("start pane {}", pane.id), &send)?;
            targets.insert(pane.id.clone(), pane_id.clone());
            pane_ids.push(pane_id);
        }

        if let Some(last) = pane_ids.last() {
            let select = self
                .runner
                .run(&strings(["select-pane", "-t", last.as_str()]))?;
            ensure_success("select final pane", &select)?;
        }
        Ok(pane_ids)
    }

    pub fn list_managed(&self) -> Result<Vec<ManagedSession>> {
        let output = self.runner.run(&strings([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_attached}\t#{session_windows}\t#{@t4e_workspace}",
        ]))?;
        if !output.success {
            if output.stderr.contains("no server running") {
                return Ok(Vec::new());
            }
            ensure_success("list sessions", &output)?;
        }
        output
            .stdout
            .lines()
            .filter_map(parse_managed_session)
            .collect()
    }

    pub fn stop(&self, session_name: &str) -> Result<()> {
        validate_identifier("session name", session_name)?;
        let output = self
            .runner
            .run(&strings(["kill-session", "-t", session_name]))?;
        ensure_success("stop session", &output)
    }

    pub fn attach(&self, session_name: &str) -> Result<()> {
        validate_identifier("session name", session_name)?;
        let output = self
            .runner
            .run(&strings(["attach-session", "-t", session_name]))?;
        ensure_success("attach session", &output)
    }

    pub fn snapshot(&self, workspace: &Workspace) -> Result<ReproSnapshot> {
        let session = workspace
            .session_name
            .as_deref()
            .unwrap_or(workspace.id.as_str());
        validate_identifier("session name", session)?;
        let windows_output = self.runner.run(&strings([
            "list-windows",
            "-t",
            session,
            "-F",
            "#{window_index}\t#{window_name}\t#{window_layout}",
        ]))?;
        ensure_success("inspect windows", &windows_output)?;
        let panes_output = self.runner.run(&strings([
            "list-panes",
            "-s",
            "-t",
            session,
            "-F",
            "#{window_index}\t#{pane_index}\t#{pane_width}\t#{pane_height}\t#{pane_start_command}",
        ]))?;
        ensure_success("inspect panes", &panes_output)?;

        let windows = windows_output
            .stdout
            .lines()
            .map(parse_window)
            .collect::<Result<Vec<_>>>()?;
        let panes = panes_output
            .stdout
            .lines()
            .map(parse_pane)
            .collect::<Result<Vec<_>>>()?;
        let commands = workspace
            .layout
            .panes
            .iter()
            .enumerate()
            .map(|(sequence, pane)| CommandLog {
                window_index: 0,
                pane_index: sequence + 1,
                sequence,
                command: pane.cmd.clone(),
            })
            .collect();
        Ok(ReproSnapshot {
            windows,
            panes,
            commands,
        })
    }
}

fn parse_managed_session(line: &str) -> Option<Result<ManagedSession>> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 4 || fields[3].is_empty() {
        return None;
    }
    Some((|| {
        Ok(ManagedSession {
            name: fields[0].to_string(),
            attached_clients: fields[1].parse()?,
            windows: fields[2].parse()?,
            workspace_id: fields[3].to_string(),
        })
    })())
}

fn parse_window(line: &str) -> Result<WindowSnapshot> {
    let fields = split_fields(line, 3, "window")?;
    Ok(WindowSnapshot {
        window_index: fields[0].parse()?,
        window_name: fields[1].to_string(),
        window_layout: fields[2].to_string(),
    })
}

fn parse_pane(line: &str) -> Result<PaneSnapshot> {
    let fields = split_fields(line, 5, "pane")?;
    Ok(PaneSnapshot {
        window_index: fields[0].parse()?,
        pane_index: fields[1].parse()?,
        pane_width: fields[2].parse()?,
        pane_height: fields[3].parse()?,
        pane_start_command: fields[4].to_string(),
    })
}

fn split_fields<'a>(line: &'a str, count: usize, label: &str) -> Result<Vec<&'a str>> {
    let fields = line.splitn(count, '\t').collect::<Vec<_>>();
    if fields.len() != count {
        bail!("invalid tmux {label} output: {line}");
    }
    Ok(fields)
}

fn ensure_success(action: &str, output: &TmuxOutput) -> Result<()> {
    if output.success {
        Ok(())
    } else {
        bail!("{action} failed: {}", output.stderr.trim())
    }
}

fn validate_percent(value: &str) -> Result<()> {
    let number = value
        .strip_suffix('%')
        .ok_or_else(|| anyhow::anyhow!("invalid pane size: {value}"))?;
    let parsed = number.parse::<u8>()?;
    if parsed == 0 || parsed > 100 || parsed.to_string() != number {
        bail!("invalid pane size: {value}");
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!("invalid {label}: {value}");
    }
    Ok(())
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(str::to_string).collect()
}
