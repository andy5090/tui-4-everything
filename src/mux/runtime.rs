use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::installer::checks::{InstallChecker, SystemInstallChecker};

use super::tmux::{CommandLog, PaneSnapshot, ReproSnapshot, WindowSnapshot};
use super::workspace::{MuxBackend, SplitDirection, TmuxView, Workspace};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedApp {
    pub pane_id: String,
    pub window_index: usize,
    pub window_name: String,
    pub pane_index: usize,
    pub process: String,
}

pub struct TmuxRuntime<R = SystemTmuxRunner> {
    runner: R,
    managed_panes: Mutex<HashMap<String, String>>,
}

impl Default for TmuxRuntime<SystemTmuxRunner> {
    fn default() -> Self {
        Self::new(SystemTmuxRunner)
    }
}

impl<R: TmuxRunner> TmuxRuntime<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            managed_panes: Mutex::new(HashMap::new()),
        }
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

    pub fn launch_app(
        &self,
        session_name: &str,
        workspace_id: &str,
        app_id: &str,
        command: &str,
    ) -> Result<LaunchOutcome> {
        validate_identifier("session name", session_name)?;
        validate_identifier("workspace id", workspace_id)?;
        validate_identifier("app id", app_id)?;
        validate_command(command)?;
        let app_command = format!("exec {command}");

        if self.session_exists(session_name)? {
            self.ensure_managed_session(session_name)?;
            if self
                .list_apps(session_name)?
                .iter()
                .any(|app| app.window_name == app_id)
            {
                return Ok(LaunchOutcome {
                    workspace_id: workspace_id.to_string(),
                    session_name: session_name.to_string(),
                    created: false,
                    pane_ids: Vec::new(),
                });
            }

            let create = self.runner.run(&strings([
                "new-window",
                "-d",
                "-t",
                session_name,
                "-n",
                app_id,
                "-P",
                "-F",
                "#{pane_id}",
                &app_command,
            ]))?;
            ensure_success(&format!("create app window {app_id}"), &create)?;
            let pane_id = create.stdout.trim().to_string();
            if pane_id.is_empty() {
                bail!("tmux returned no pane id for app window {app_id}");
            }
            self.disable_automatic_rename(&format!("{session_name}:{app_id}"))?;
            return Ok(LaunchOutcome {
                workspace_id: workspace_id.to_string(),
                session_name: session_name.to_string(),
                created: true,
                pane_ids: vec![pane_id],
            });
        }

        let create = self.runner.run(&strings([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-n",
            app_id,
            "-P",
            "-F",
            "#{pane_id}",
            &app_command,
        ]))?;
        ensure_success("create app session", &create)?;
        let pane_id = create.stdout.trim().to_string();
        if pane_id.is_empty() {
            let _ = self
                .runner
                .run(&strings(["kill-session", "-t", session_name]));
            bail!("tmux returned no pane id for app window {app_id}");
        }
        let setup = (|| {
            let marker = self.runner.run(&strings([
                "set-option",
                "-t",
                session_name,
                "@t4e_workspace",
                workspace_id,
            ]))?;
            ensure_success("mark app session", &marker)?;
            self.disable_automatic_rename(&format!("{session_name}:{app_id}"))
        })();
        if let Err(error) = setup {
            let _ = self
                .runner
                .run(&strings(["kill-session", "-t", session_name]));
            return Err(error);
        }
        Ok(LaunchOutcome {
            workspace_id: workspace_id.to_string(),
            session_name: session_name.to_string(),
            created: true,
            pane_ids: vec![pane_id],
        })
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

        match workspace.tmux_view {
            TmuxView::Windows => self.configure_app_windows(workspace, session),
            TmuxView::Panes => self.configure_split_panes(workspace, session),
        }
    }

    fn configure_app_windows(&self, workspace: &Workspace, session: &str) -> Result<Vec<String>> {
        let Some(first) = workspace.layout.panes.first() else {
            bail!("workspace {} has no apps", workspace.id);
        };
        validate_identifier("app window", &first.id)?;

        let initial = format!("{session}:main");
        let rename = self.runner.run(&strings([
            "rename-window",
            "-t",
            initial.as_str(),
            first.id.as_str(),
        ]))?;
        ensure_success("name first app window", &rename)?;

        let first_window = format!("{session}:{}", first.id);
        self.disable_automatic_rename(&first_window)?;
        let first_target = format!("{first_window}.0");
        self.start_app(&first_target, &first.cmd, &first.id)?;
        let mut targets = vec![first_target];

        for app in workspace.layout.panes.iter().skip(1) {
            validate_identifier("app window", &app.id)?;
            let create = self.runner.run(&strings([
                "new-window",
                "-d",
                "-t",
                session,
                "-n",
                app.id.as_str(),
                "-P",
                "-F",
                "#{pane_id}",
                "bash",
            ]))?;
            ensure_success(&format!("create app window {}", app.id), &create)?;
            let pane_id = create.stdout.trim().to_string();
            if pane_id.is_empty() {
                bail!("tmux returned no pane id for app window {}", app.id);
            }
            self.disable_automatic_rename(&format!("{session}:{}", app.id))?;
            self.start_app(&pane_id, &app.cmd, &app.id)?;
            targets.push(pane_id);
        }

        let select = self
            .runner
            .run(&strings(["select-window", "-t", first_window.as_str()]))?;
        ensure_success("select first app window", &select)?;
        Ok(targets)
    }

    fn disable_automatic_rename(&self, window: &str) -> Result<()> {
        let output = self.runner.run(&strings([
            "set-window-option",
            "-t",
            window,
            "automatic-rename",
            "off",
        ]))?;
        ensure_success("preserve app window name", &output)
    }

    fn start_app(&self, target: &str, command: &str, app_id: &str) -> Result<()> {
        let output =
            self.runner
                .run(&strings(["send-keys", "-t", target, "--", command, "C-m"]))?;
        ensure_success(&format!("start app {app_id}"), &output)
    }

    fn configure_split_panes(&self, workspace: &Workspace, session: &str) -> Result<Vec<String>> {
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
        ensure_success("stop session", &output)?;
        self.managed_panes
            .lock()
            .map_err(|_| anyhow::anyhow!("managed pane registry is unavailable"))?
            .retain(|_, session| session != session_name);
        Ok(())
    }

    pub fn list_apps(&self, session_name: &str) -> Result<Vec<ManagedApp>> {
        validate_identifier("session name", session_name)?;
        self.ensure_managed_session(session_name)?;
        let output = self.runner.run(&strings([
            "list-panes",
            "-s",
            "-t",
            session_name,
            "-F",
            "#{pane_id}\t#{window_index}\t#{window_name}\t#{pane_index}\t#{pane_current_command}",
        ]))?;
        ensure_success("list workspace apps", &output)?;
        let mut apps = output
            .stdout
            .lines()
            .map(parse_managed_app)
            .collect::<Result<Vec<_>>>()?;
        apps.sort_by_key(|app| (app.window_index, app.pane_index));
        let mut managed_panes = self
            .managed_panes
            .lock()
            .map_err(|_| anyhow::anyhow!("managed pane registry is unavailable"))?;
        managed_panes.retain(|_, session| session != session_name);
        managed_panes.extend(
            apps.iter()
                .map(|app| (app.pane_id.clone(), session_name.to_string())),
        );
        Ok(apps)
    }

    pub fn capture_app(&self, pane_id: &str) -> Result<String> {
        self.ensure_managed_pane(pane_id)?;
        let output =
            self.runner
                .run(&strings(["capture-pane", "-p", "-e", "-J", "-t", pane_id]))?;
        ensure_success("capture app screen", &output)?;
        Ok(output.stdout.trim_end_matches('\n').to_string())
    }

    pub fn resize_app(&self, pane_id: &str, width: u16, height: u16) -> Result<()> {
        self.ensure_managed_pane(pane_id)?;
        if width < 20 || height < 5 {
            bail!("app viewport is too small: {width}x{height}");
        }
        let width = width.to_string();
        let height = height.to_string();
        let output = self.runner.run(&strings([
            "resize-window",
            "-t",
            pane_id,
            "-x",
            width.as_str(),
            "-y",
            height.as_str(),
        ]))?;
        ensure_success("resize app viewport", &output)
    }

    pub fn send_app_text(&self, pane_id: &str, text: &str) -> Result<()> {
        self.ensure_managed_pane(pane_id)?;
        if text.chars().any(|ch| ch.is_control()) {
            bail!("app text contains a control character");
        }
        let output = self
            .runner
            .run(&strings(["send-keys", "-l", "-t", pane_id, "--", text]))?;
        ensure_success("send app text", &output)
    }

    pub fn send_app_key(&self, pane_id: &str, key: &str) -> Result<()> {
        self.ensure_managed_pane(pane_id)?;
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            bail!("invalid app key: {key}");
        }
        let output = self
            .runner
            .run(&strings(["send-keys", "-t", pane_id, key]))?;
        ensure_success("send app key", &output)
    }

    pub fn close_app(&self, pane_id: &str) -> Result<()> {
        self.ensure_managed_pane(pane_id)?;
        let output = self.runner.run(&strings(["kill-pane", "-t", pane_id]))?;
        ensure_success("close app", &output)?;
        self.managed_panes
            .lock()
            .map_err(|_| anyhow::anyhow!("managed pane registry is unavailable"))?
            .remove(pane_id);
        Ok(())
    }

    fn ensure_managed_session(&self, session_name: &str) -> Result<()> {
        if self
            .list_managed()?
            .iter()
            .any(|session| session.name == session_name)
        {
            Ok(())
        } else {
            bail!("session {session_name} is not managed by T4E")
        }
    }

    fn ensure_managed_pane(&self, pane_id: &str) -> Result<()> {
        validate_pane_id(pane_id)?;
        if self
            .managed_panes
            .lock()
            .map_err(|_| anyhow::anyhow!("managed pane registry is unavailable"))?
            .contains_key(pane_id)
        {
            Ok(())
        } else {
            bail!("pane {pane_id} is not managed by T4E")
        }
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
                window_index: if matches!(workspace.tmux_view, TmuxView::Windows) {
                    sequence
                } else {
                    0
                },
                pane_index: if matches!(workspace.tmux_view, TmuxView::Windows) {
                    0
                } else {
                    sequence + 1
                },
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

fn parse_managed_app(line: &str) -> Result<ManagedApp> {
    let fields = split_fields(line, 5, "managed app")?;
    validate_pane_id(fields[0])?;
    Ok(ManagedApp {
        pane_id: fields[0].to_string(),
        window_index: fields[1].parse()?,
        window_name: fields[2].to_string(),
        pane_index: fields[3].parse()?,
        process: fields[4].to_string(),
    })
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

fn validate_command(command: &str) -> Result<()> {
    if command.trim().is_empty()
        || command
            .chars()
            .any(|ch| matches!(ch, ';' | '|' | '&' | '`' | '$' | '<' | '>' | '\n' | '\r'))
    {
        bail!("app command contains forbidden shell syntax");
    }
    Ok(())
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

fn validate_pane_id(value: &str) -> Result<()> {
    let Some(number) = value.strip_prefix('%') else {
        bail!("invalid pane id: {value}");
    };
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("invalid pane id: {value}");
    }
    Ok(())
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(str::to_string).collect()
}
