use std::collections::HashMap;
#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::mem::MaybeUninit;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
#[cfg(unix)]
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::catalog::models::OutputFilter;
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

    fn prepare_app_input(&self, _pane_id: &str) -> Result<()> {
        Ok(())
    }
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

    fn prepare_app_input(&self, pane_id: &str) -> Result<()> {
        // Spotatui can remain in its alternate screen after its terminal has
        // unexpectedly returned to canonical mode. Crossterm then receives
        // tmux key sequences as echoed text instead of input events.
        let output = self.run(&strings([
            "display-message",
            "-p",
            "-t",
            pane_id,
            "#{pane_current_command}\t#{alternate_on}\t#{pane_tty}",
        ]))?;
        ensure_success("inspect app input mode", &output)?;
        let Some(tty_path) = spotatui_raw_mode_target(output.stdout.trim_end_matches('\n'))? else {
            return Ok(());
        };

        #[cfg(unix)]
        repair_terminal_raw_mode(Path::new(tty_path))
            .with_context(|| format!("failed to restore Spotatui input mode on {tty_path}"))?;

        Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedPane {
    session_name: String,
    app_id: String,
}

pub struct TmuxRuntime<R = SystemTmuxRunner> {
    runner: R,
    managed_panes: Mutex<HashMap<String, ManagedPane>>,
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
            "sh",
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
        self.launch_app_at_size(session_name, workspace_id, app_id, command, 80, 24)
    }

    pub fn launch_app_at_size(
        &self,
        session_name: &str,
        workspace_id: &str,
        app_id: &str,
        command: &str,
        width: u16,
        height: u16,
    ) -> Result<LaunchOutcome> {
        self.launch_app_at_size_with_mode(
            session_name,
            workspace_id,
            app_id,
            command,
            width,
            height,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn launch_app_at_size_with_mode(
        &self,
        session_name: &str,
        workspace_id: &str,
        app_id: &str,
        command: &str,
        width: u16,
        height: u16,
        keep_open: bool,
    ) -> Result<LaunchOutcome> {
        self.launch_app_at_size_with_mode_and_filter(
            session_name,
            workspace_id,
            app_id,
            command,
            width,
            height,
            keep_open,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn launch_app_at_size_with_mode_and_filter(
        &self,
        session_name: &str,
        workspace_id: &str,
        app_id: &str,
        command: &str,
        width: u16,
        height: u16,
        keep_open: bool,
        output_filter: Option<OutputFilter>,
    ) -> Result<LaunchOutcome> {
        validate_identifier("session name", session_name)?;
        validate_identifier("workspace id", workspace_id)?;
        validate_identifier("app id", app_id)?;
        validate_command(command)?;
        validate_app_viewport(width, height)?;
        let command = match output_filter {
            Some(OutputFilter::Lolcat) => format!("{command} | lolcat"),
            None => command.to_string(),
        };
        self.launch_validated_app_at_size(
            session_name,
            workspace_id,
            app_id,
            &command,
            width,
            height,
            keep_open,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn launch_pipeline_at_size_with_mode(
        &self,
        session_name: &str,
        workspace_id: &str,
        app_id: &str,
        commands: &[&str],
        width: u16,
        height: u16,
        keep_open: bool,
    ) -> Result<LaunchOutcome> {
        validate_identifier("session name", session_name)?;
        validate_identifier("workspace id", workspace_id)?;
        validate_identifier("app id", app_id)?;
        if commands.len() < 2 {
            bail!("app pipeline requires at least two stages");
        }
        commands
            .iter()
            .try_for_each(|command| validate_command(command))?;
        validate_app_viewport(width, height)?;
        let command = commands.join(" | ");
        self.launch_validated_app_at_size(
            session_name,
            workspace_id,
            app_id,
            &command,
            width,
            height,
            keep_open,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn launch_validated_app_at_size(
        &self,
        session_name: &str,
        workspace_id: &str,
        app_id: &str,
        command: &str,
        width: u16,
        height: u16,
        keep_open: bool,
    ) -> Result<LaunchOutcome> {
        let managed_script = if keep_open {
            format!("PATH=\"$HOME/.local/bin:$PATH\"; export PATH; {command}")
        } else {
            format!("PATH=\"$HOME/.local/bin:$PATH\"; export PATH; exec {command}")
        };
        let app_command = if keep_open {
            persistent_output_command(&managed_script)
        } else {
            format!("exec sh -c {}", shell_quote(&managed_script))
        };

        if self.session_exists(session_name)? {
            self.ensure_managed_session(session_name)?;
            let listed_apps = self.list_apps_including_dead(session_name)?;
            if let Some((app, dead)) = listed_apps
                .iter()
                .find(|(app, _)| app.window_name == app_id)
            {
                if *dead {
                    self.resize_window(&app.pane_id, width, height)?;
                    self.disable_automatic_rename(&format!("{session_name}:{app_id}"))?;
                    self.spawn_app(&app.pane_id, &app_command, app_id)?;
                    return Ok(LaunchOutcome {
                        workspace_id: workspace_id.to_string(),
                        session_name: session_name.to_string(),
                        created: true,
                        pane_ids: vec![app.pane_id.clone()],
                    });
                }
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
                "sh",
            ]))?;
            ensure_success(&format!("create app window {app_id}"), &create)?;
            let pane_id = create.stdout.trim().to_string();
            if pane_id.is_empty() {
                bail!("tmux returned no pane id for app window {app_id}");
            }
            let setup = (|| {
                self.resize_window(&pane_id, width, height)?;
                self.disable_automatic_rename(&format!("{session_name}:{app_id}"))?;
                self.spawn_app(&pane_id, &app_command, app_id)
            })();
            if let Err(error) = setup {
                let _ = self.runner.run(&strings(["kill-pane", "-t", &pane_id]));
                return Err(error);
            }
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
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
            "-s",
            session_name,
            "-n",
            app_id,
            "-P",
            "-F",
            "#{pane_id}",
            "sh",
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
            self.disable_automatic_rename(&format!("{session_name}:{app_id}"))?;
            self.spawn_app(&pane_id, &app_command, app_id)
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
                "sh",
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

    fn spawn_app(&self, target: &str, command: &str, app_id: &str) -> Result<()> {
        let output = self
            .runner
            .run(&strings(["respawn-pane", "-k", "-t", target, command]))?;
        ensure_success(&format!("spawn app {app_id}"), &output)
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
                "sh",
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
            .retain(|_, pane| pane.session_name != session_name);
        Ok(())
    }

    pub fn list_apps(&self, session_name: &str) -> Result<Vec<ManagedApp>> {
        Ok(self
            .list_apps_including_dead(session_name)?
            .into_iter()
            .filter_map(|(app, dead)| (!dead).then_some(app))
            .collect())
    }

    fn list_apps_including_dead(&self, session_name: &str) -> Result<Vec<(ManagedApp, bool)>> {
        validate_identifier("session name", session_name)?;
        self.ensure_managed_session(session_name)?;
        let output = self.runner.run(&strings([
            "list-panes",
            "-s",
            "-t",
            session_name,
            "-F",
            "#{pane_id}\t#{window_index}\t#{window_name}\t#{pane_index}\t#{pane_current_command}\t#{pane_dead}",
        ]))?;
        ensure_success("list workspace apps", &output)?;
        let mut apps = output
            .stdout
            .lines()
            .map(parse_managed_app)
            .collect::<Result<Vec<_>>>()?;
        apps.sort_by_key(|(app, _)| (app.window_index, app.pane_index));
        let mut managed_panes = self
            .managed_panes
            .lock()
            .map_err(|_| anyhow::anyhow!("managed pane registry is unavailable"))?;
        managed_panes.retain(|_, pane| pane.session_name != session_name);
        managed_panes.extend(apps.iter().filter(|(_, dead)| !dead).map(|(app, _)| {
            (
                app.pane_id.clone(),
                ManagedPane {
                    session_name: session_name.to_string(),
                    app_id: app.window_name.clone(),
                },
            )
        }));
        Ok(apps)
    }

    pub fn capture_app(&self, pane_id: &str) -> Result<String> {
        self.capture_app_with_join(pane_id, false)
    }

    pub fn capture_app_joined(&self, pane_id: &str) -> Result<String> {
        self.capture_app_with_join(pane_id, true)
    }

    fn capture_app_with_join(&self, pane_id: &str, join_wrapped: bool) -> Result<String> {
        self.ensure_managed_pane(pane_id)?;
        let mut args = strings(["capture-pane", "-p", "-e"]);
        if join_wrapped {
            args.push("-J".to_string());
        }
        args.extend(strings(["-t", pane_id]));
        let output = self.runner.run(&args)?;
        ensure_success("capture app screen", &output)?;
        Ok(normalize_dec_graphics(output.stdout.trim_end_matches('\n')))
    }

    pub fn resize_app(&self, pane_id: &str, width: u16, height: u16) -> Result<()> {
        self.ensure_managed_pane(pane_id)?;
        self.resize_window(pane_id, width, height)
    }

    fn resize_window(&self, pane_id: &str, width: u16, height: u16) -> Result<()> {
        validate_app_viewport(width, height)?;
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
        let app_id = self.managed_app_id(pane_id)?;
        if text.chars().any(|ch| ch.is_control()) {
            bail!("app text contains a control character");
        }
        if app_id == "spotatui" {
            self.runner.prepare_app_input(pane_id)?;
        }
        let output = self
            .runner
            .run(&strings(["send-keys", "-l", "-t", pane_id, "--", text]))?;
        ensure_success("send app text", &output)
    }

    pub fn send_app_key(&self, pane_id: &str, key: &str) -> Result<()> {
        let app_id = self.managed_app_id(pane_id)?;
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            bail!("invalid app key: {key}");
        }
        if app_id == "spotatui" {
            self.runner.prepare_app_input(pane_id)?;
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
        self.managed_app_id(pane_id).map(|_| ())
    }

    fn managed_app_id(&self, pane_id: &str) -> Result<String> {
        validate_pane_id(pane_id)?;
        self.managed_panes
            .lock()
            .map_err(|_| anyhow::anyhow!("managed pane registry is unavailable"))?
            .get(pane_id)
            .map(|pane| pane.app_id.clone())
            .ok_or_else(|| anyhow::anyhow!("pane {pane_id} is not managed by T4E"))
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

fn parse_managed_app(line: &str) -> Result<(ManagedApp, bool)> {
    let fields = split_fields(line, 6, "managed app")?;
    validate_pane_id(fields[0])?;
    Ok((
        ManagedApp {
            pane_id: fields[0].to_string(),
            window_index: fields[1].parse()?,
            window_name: fields[2].to_string(),
            pane_index: fields[3].parse()?,
            process: fields[4].to_string(),
        },
        fields[5] == "1",
    ))
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

fn spotatui_raw_mode_target(line: &str) -> Result<Option<&str>> {
    let fields = split_fields(line, 3, "app input mode")?;
    if fields[0] != "spotatui" || fields[1] != "1" {
        return Ok(None);
    }
    if fields[2].is_empty() {
        bail!("Spotatui pane has no terminal device");
    }
    Ok(Some(fields[2]))
}

#[cfg(unix)]
fn repair_terminal_raw_mode(path: &Path) -> Result<bool> {
    let supported_path = path.to_str().is_some_and(|path| {
        path.strip_prefix("/dev/pts/").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
        }) || path.strip_prefix("/dev/ttys").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_alphanumeric())
        })
    });
    if !supported_path {
        bail!("unsupported terminal device: {}", path.display());
    }

    let terminal = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOCTTY | libc::O_NOFOLLOW)
        .open(path)?;
    if !terminal.metadata()?.file_type().is_char_device() {
        bail!(
            "terminal device is not a character device: {}",
            path.display()
        );
    }
    repair_terminal_fd(terminal.as_raw_fd()).map_err(Into::into)
}

#[cfg(unix)]
fn repair_terminal_fd(fd: std::os::fd::RawFd) -> io::Result<bool> {
    let mut terminal = MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `terminal` points to writable storage for one termios value and
    // is only assumed initialized after tcgetattr reports success.
    if unsafe { libc::tcgetattr(fd, terminal.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful tcgetattr call initialized the value.
    let mut terminal = unsafe { terminal.assume_init() };
    if terminal.c_lflag & (libc::ICANON | libc::ECHO) == 0 {
        return Ok(false);
    }

    // SAFETY: `terminal` was initialized by tcgetattr above.
    unsafe { libc::cfmakeraw(&mut terminal) };
    // SAFETY: `fd` remains open for this call and `terminal` is initialized.
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &terminal) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(true)
}

fn ensure_success(action: &str, output: &TmuxOutput) -> Result<()> {
    if output.success {
        Ok(())
    } else {
        bail!("{action} failed: {}", output.stderr.trim())
    }
}

fn normalize_dec_graphics(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut graphics = false;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x0e' => graphics = true,
            '\x0f' => graphics = false,
            '\x1b' => {
                output.push(ch);
                let Some(next) = chars.next() else {
                    break;
                };
                output.push(next);
                if next == '[' {
                    for sequence in chars.by_ref() {
                        output.push(sequence);
                        if ('@'..='~').contains(&sequence) {
                            break;
                        }
                    }
                } else if matches!(next, '(' | ')' | '*' | '+')
                    && let Some(designator) = chars.next()
                {
                    output.push(designator);
                }
            }
            _ if graphics => output.push(dec_graphic(ch)),
            _ => output.push(ch),
        }
    }
    output
}

fn dec_graphic(ch: char) -> char {
    match ch {
        '`' => '◆',
        'a' => '▒',
        'f' => '°',
        'g' => '±',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => ch,
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

fn persistent_output_command(command: &str) -> String {
    let script = format!(
        "{command}; status=$?; \
         if [ \"$status\" -ne 0 ]; then \
         printf '\\n[T4E] command exited with status %s\\n' \"$status\"; fi; \
         trap 'exit 0' INT TERM; while :; do sleep 86400; done"
    );
    format!("exec sh -c {}", shell_quote(&script))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn validate_app_viewport(width: u16, height: u16) -> Result<()> {
    if width < 20 || height < 5 {
        bail!("app viewport is too small: {width}x{height}");
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

#[cfg(test)]
mod tests {
    use super::{normalize_dec_graphics, persistent_output_command, spotatui_raw_mode_target};

    #[test]
    fn dec_special_graphics_are_converted_without_corrupting_ansi_styles() {
        let captured = "\x1b[37m\x0elqqk\x1b[31mx\x0fmqqj";
        assert_eq!(
            normalize_dec_graphics(captured),
            "\x1b[37m┌──┐\x1b[31m│mqqj"
        );
    }

    #[test]
    fn persistent_output_reports_command_failures() {
        let command = persistent_output_command("fortune");

        assert!(command.contains("[T4E] command exited"));
    }

    #[test]
    fn spotatui_raw_mode_repair_only_targets_its_alternate_screen() {
        assert_eq!(
            spotatui_raw_mode_target("spotatui\t1\t/dev/pts/7").expect("valid context"),
            Some("/dev/pts/7")
        );
        assert_eq!(
            spotatui_raw_mode_target("spotatui\t0\t/dev/pts/7").expect("valid context"),
            None
        );
        assert_eq!(
            spotatui_raw_mode_target("bash\t1\t/dev/pts/7").expect("valid context"),
            None
        );
        assert!(spotatui_raw_mode_target("spotatui\t1").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn canonical_pty_input_is_restored_to_raw_mode() {
        use std::mem::MaybeUninit;
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        let mut master_fd = -1;
        let mut slave_fd = -1;
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master_fd,
                    &mut slave_fd,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        let _master = unsafe { OwnedFd::from_raw_fd(master_fd) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave_fd) };

        let mut terminal = MaybeUninit::<libc::termios>::uninit();
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), terminal.as_mut_ptr()) },
            0
        );
        let mut terminal = unsafe { terminal.assume_init() };
        terminal.c_lflag |= libc::ICANON | libc::ECHO;
        assert_eq!(
            unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &terminal) },
            0
        );

        assert!(super::repair_terminal_fd(slave.as_raw_fd()).expect("raw mode repair"));
        assert!(!super::repair_terminal_fd(slave.as_raw_fd()).expect("raw mode is stable"));

        let mut repaired = MaybeUninit::<libc::termios>::uninit();
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), repaired.as_mut_ptr()) },
            0
        );
        let repaired = unsafe { repaired.assume_init() };
        assert_eq!(repaired.c_lflag & (libc::ICANON | libc::ECHO), 0);
    }
}
