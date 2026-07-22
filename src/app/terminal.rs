use std::collections::{BTreeSet, HashMap};
use std::io::{self, Stdout, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::catalog::models::InstallMethod;
use crate::codex::service::{CodexCommand, CodexService};
use crate::installer::checks::{InstallChecker, SystemInstallChecker};
use crate::installer::execution::{
    CommandRunner, ExecutionPolicy, InstallExecutor, InstallJob, OutputChunk, SystemCommandRunner,
};
use crate::mux::runtime::{SystemTmuxRunner, TmuxRuntime};
use crate::mux::tmux::reproducibility_hash;

use super::events::Screen;
use super::state::{AppEffect, AppInput, AppState};
use super::ui::render;

type TuiTerminal = Terminal<CrosstermBackend<Stdout>>;

static INSTALL_PROCESS_LOCK: Mutex<()> = Mutex::new(());

pub fn run(mut app: AppState) -> Result<()> {
    let mut session = TerminalSession::new()?;
    let (event_sender, event_receiver) = mpsc::channel();
    let mut active = HashMap::<String, ActiveInstall>::new();
    let mut app_sizes = HashMap::<String, (u16, u16)>::new();
    let tmux = TmuxRuntime::new(SystemTmuxRunner);
    let codex = CodexService::spawn(
        std::env::current_dir()
            .unwrap_or_default()
            .display()
            .to_string(),
    );
    let checker = SystemInstallChecker;
    let installed = app
        .catalog
        .tools
        .iter()
        .filter_map(|tool| {
            let platform = if cfg!(target_os = "macos") {
                crate::catalog::models::Platform::Macos
            } else {
                crate::catalog::models::Platform::Linux
            };
            tool.install_check_commands(platform)
                .iter()
                .all(|command| checker.check(command).is_ok_and(|result| result.installed))
                .then(|| tool.id.clone())
        })
        .collect::<BTreeSet<_>>();
    app.apply_installed_tools(installed);
    match tmux.list_managed() {
        Ok(sessions) => {
            let has_app_session = sessions.iter().any(|session| session.name == "t4e-apps");
            app.apply_managed_sessions(sessions);
            if has_app_session {
                match tmux.list_apps("t4e-apps") {
                    Ok(apps) => app.remember_app_view("t4e-apps".to_string(), apps),
                    Err(error) => app.apply_workspace_error("restore apps", &error),
                }
            }
        }
        Err(error) => app.apply_workspace_error("refresh", &error),
    }
    persist_or_report(&mut app);

    while !app.should_quit {
        drain_runtime_events(&mut app, &event_receiver, &mut active);
        drain_codex_events(&mut app, &codex);
        process_effects(
            &mut app,
            &event_sender,
            &mut active,
            &tmux,
            &codex,
            &mut session,
        );
        sync_app_viewport(&mut app, &tmux, &mut app_sizes, &session);
        refresh_app_view(&mut app, &tmux);
        session.terminal.draw(|frame| render(frame, &mut app))?;
        if event::poll(frame_poll_interval(app.screen))? {
            let handled = match event::read()? {
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    app.handle_key(key);
                    true
                }
                Event::Mouse(mouse) => {
                    let height = session.terminal.size().map(|size| size.height).unwrap_or(0);
                    app.handle_mouse(mouse, height);
                    true
                }
                _ => false,
            };
            if !handled {
                continue;
            }
            process_effects(
                &mut app,
                &event_sender,
                &mut active,
                &tmux,
                &codex,
                &mut session,
            );
            persist_or_report(&mut app);
        }
    }

    for install in active.values() {
        install.cancel.store(true, Ordering::Relaxed);
    }
    for (_, install) in active.drain() {
        let _ = install.handle.join();
    }
    drain_runtime_events(&mut app, &event_receiver, &mut active);
    persist_or_report(&mut app);
    Ok(())
}

enum RuntimeEvent {
    Output {
        tool_id: String,
        chunk: OutputChunk,
    },
    Complete(Box<InstallJob>),
    UninstallComplete {
        tool_id: String,
        success: bool,
        error: String,
    },
}

struct ActiveInstall {
    cancel: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

fn process_effects(
    app: &mut AppState,
    event_sender: &mpsc::Sender<RuntimeEvent>,
    active: &mut HashMap<String, ActiveInstall>,
    tmux: &TmuxRuntime<SystemTmuxRunner>,
    codex: &CodexService,
    session: &mut TerminalSession,
) {
    while let Some(effect) = app.take_effect() {
        match effect {
            AppEffect::Execute(job) => {
                let tool_id = job.item.tool_id.clone();
                if active.contains_key(&tool_id) {
                    continue;
                }
                if (job.task.requires_privileges
                    || install_method_requires_privileges(&job.task.method))
                    && let Err(error) = session.suspend_for(|| acquire_install_privileges(&tool_id))
                {
                    app.apply_install_authorization_error(&tool_id, &error);
                    continue;
                }
                let cancel = Arc::new(AtomicBool::new(false));
                let thread_cancel = Arc::clone(&cancel);
                let sender = event_sender.clone();
                let output_tool_id = tool_id.clone();
                let policy = ExecutionPolicy {
                    timeout: Duration::from_secs(
                        job.task
                            .effective_timeout_sec(app.settings.install_timeout_sec),
                    ),
                    max_attempts: if job.task.method == InstallMethod::Cargo {
                        1
                    } else {
                        app.settings.max_install_attempts
                    },
                    log_dir: app.log_dir(),
                };
                app.mark_execution_started(&tool_id);
                let handle = thread::spawn(move || {
                    let _install_guard = INSTALL_PROCESS_LOCK
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let executor = InstallExecutor::new(SystemCommandRunner, policy);
                    let completed = executor.execute(*job, thread_cancel, |chunk| {
                        let _ = sender.send(RuntimeEvent::Output {
                            tool_id: output_tool_id.clone(),
                            chunk,
                        });
                    });
                    let _ = sender.send(RuntimeEvent::Complete(Box::new(completed)));
                });
                active.insert(tool_id, ActiveInstall { cancel, handle });
            }
            AppEffect::Cancel(tool_id) => {
                if let Some(install) = active.get(&tool_id) {
                    install.cancel.store(true, Ordering::Relaxed);
                }
            }
            AppEffect::LaunchTool(request) => {
                let executable = request
                    .command
                    .split_whitespace()
                    .next()
                    .unwrap_or(request.tool_id.as_str())
                    .to_string();
                match tmux.preflight(std::slice::from_ref(&executable)) {
                    Ok(preflight) if !preflight.tmux_available => app.apply_workspace_error(
                        "app launch",
                        &anyhow::anyhow!("tmux is not installed"),
                    ),
                    Ok(preflight) if !preflight.missing_commands.is_empty() => {
                        app.install_then_launch(request);
                    }
                    Ok(_) => {
                        let (width, height) = app_viewport_size(session).unwrap_or((80, 17));
                        match tmux.launch_app_at_size(
                            "t4e-apps",
                            "app-launcher",
                            &request.tool_id,
                            &request.command,
                            width,
                            height,
                        ) {
                            Ok(_) => match tmux.list_apps("t4e-apps") {
                                Ok(apps) => {
                                    app.open_app_view("t4e-apps".to_string(), apps);
                                    app.focus_app(&request.tool_id);
                                }
                                Err(error) => app.apply_workspace_error("open app", &error),
                            },
                            Err(error) => app.apply_workspace_error("app launch", &error),
                        }
                    }
                    Err(error) => app.apply_workspace_error("app preflight", &error),
                }
            }
            AppEffect::LaunchWorkspace(request) => {
                let commands = request
                    .required_tools
                    .iter()
                    .map(|(_, command)| command.clone())
                    .collect::<Vec<_>>();
                match tmux.preflight(&commands) {
                    Ok(preflight) if !preflight.tmux_available => app.apply_workspace_error(
                        "preflight",
                        &anyhow::anyhow!("tmux is not installed"),
                    ),
                    Ok(preflight) if !preflight.missing_commands.is_empty() => {
                        let missing = request
                            .required_tools
                            .iter()
                            .filter(|(_, command)| preflight.missing_commands.contains(command))
                            .map(|(id, _)| id.clone())
                            .collect();
                        app.apply_workspace_preflight_failure(missing);
                    }
                    Ok(_) => match tmux.launch(&request.workspace) {
                        Ok(outcome) => app.apply_workspace_launch(outcome),
                        Err(error) => app.apply_workspace_error("launch", &error),
                    },
                    Err(error) => app.apply_workspace_error("preflight", &error),
                }
            }
            AppEffect::OpenAppView(session_name) => match tmux.list_apps(&session_name) {
                Ok(apps) => app.open_app_view(session_name, apps),
                Err(error) => app.apply_workspace_error("open apps", &error),
            },
            AppEffect::SendAppInput { pane_id, input } => {
                let result = match input {
                    AppInput::Text(text) => tmux.send_app_text(&pane_id, &text),
                    AppInput::Key(key) => tmux.send_app_key(&pane_id, &key),
                };
                if let Err(error) = result {
                    app.apply_app_view_error(&error);
                }
            }
            AppEffect::CloseApp(pane_id) => {
                if let Err(error) = tmux.close_app(&pane_id) {
                    app.apply_app_view_error(&error);
                } else {
                    reload_app_view(app, tmux);
                }
            }
            AppEffect::SetMouseCapture(enabled) => {
                if let Err(error) = session.set_mouse_capture(enabled) {
                    app.status = format!("Mouse mode failed: {error}");
                }
            }
            AppEffect::CopyUrl(url) => match copy_to_clipboard(&url) {
                Ok(method) => app.status = format!("Copied link via {method}"),
                Err(error) => app.status = format!("Could not copy link: {error}"),
            },
            AppEffect::OpenUrl(url) => match open_url(&url) {
                Ok(()) => app.status = "Opened link in the default browser".to_string(),
                Err(error) => app.status = format!("Could not open link: {error}"),
            },
            AppEffect::Uninstall(request) => {
                let tool_id = request.tool_id.clone();
                if active.contains_key(&tool_id) {
                    continue;
                }
                if uninstall_method_requires_privileges(&request.method)
                    && let Err(error) = session.suspend_for(|| acquire_install_privileges(&tool_id))
                {
                    app.apply_uninstall_result(&tool_id, false, &error.to_string());
                    continue;
                }
                let cancel = Arc::new(AtomicBool::new(false));
                let thread_cancel = Arc::clone(&cancel);
                let sender = event_sender.clone();
                let timeout = Duration::from_secs(app.settings.install_timeout_sec);
                let active_tool_id = tool_id.clone();
                app.mark_uninstall_started(&tool_id);
                let handle = thread::spawn(move || {
                    let _install_guard = INSTALL_PROCESS_LOCK
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let runner = SystemCommandRunner;
                    let output_tool_id = tool_id.clone();
                    let result =
                        runner.run(&request.command, timeout, &thread_cancel, &mut |chunk| {
                            let _ = sender.send(RuntimeEvent::Output {
                                tool_id: output_tool_id.clone(),
                                chunk,
                            });
                        });
                    let (success, error) = match result {
                        Ok(output) if output.exit_code == Some(0) => {
                            match SystemInstallChecker.check(&request.check_command) {
                                Ok(check) if !check.installed => (true, String::new()),
                                Ok(_) => (
                                    false,
                                    "remove command succeeded but executable is still present"
                                        .to_string(),
                                ),
                                Err(error) => (false, error.to_string()),
                            }
                        }
                        Ok(output) => (
                            false,
                            format!("remove command exited with {:?}", output.exit_code),
                        ),
                        Err(error) => (false, error.to_string()),
                    };
                    let _ = sender.send(RuntimeEvent::UninstallComplete {
                        tool_id,
                        success,
                        error,
                    });
                });
                active.insert(active_tool_id, ActiveInstall { cancel, handle });
            }
            AppEffect::StopWorkspace(session_name) => match tmux.stop(&session_name) {
                Ok(()) => {
                    app.status = format!("Stopped tmux session {session_name}");
                    app.logs.push(format!("workspace: stopped {session_name}"));
                    match tmux.list_managed() {
                        Ok(sessions) => app.apply_managed_sessions(sessions),
                        Err(error) => app.apply_workspace_error("refresh", &error),
                    }
                }
                Err(error) => app.apply_workspace_error("stop", &error),
            },
            AppEffect::RefreshWorkspaces => match tmux.list_managed() {
                Ok(sessions) => app.apply_managed_sessions(sessions),
                Err(error) => app.apply_workspace_error("refresh", &error),
            },
            AppEffect::SnapshotWorkspace(workspace) => match tmux.snapshot(&workspace) {
                Ok(snapshot) => {
                    let root = std::env::current_dir()
                        .unwrap_or_default()
                        .display()
                        .to_string();
                    let hash = reproducibility_hash(&snapshot, &root);
                    app.apply_workspace_hash(&workspace.id, &hash);
                }
                Err(error) => app.apply_workspace_error("snapshot", &error),
            },
            AppEffect::CodexPrompt(prompt) => {
                let environment_context = app.ai_environment_context();
                if codex
                    .send(CodexCommand::Prompt {
                        text: prompt,
                        environment_context,
                    })
                    .is_err()
                {
                    app.apply_codex_event(crate::codex::service::CodexEvent::Error(
                        "Codex service is not running".to_string(),
                    ));
                }
            }
            AppEffect::CodexInterrupt => {
                if codex.send(CodexCommand::Interrupt).is_err() {
                    app.apply_codex_event(crate::codex::service::CodexEvent::Error(
                        "Codex service is not running".to_string(),
                    ));
                }
            }
        }
    }
}

fn copy_to_clipboard(text: &str) -> Result<&'static str> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (program, args) in candidates {
        let mut child = match Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => continue,
        };
        if let Some(stdin) = &mut child.stdin {
            stdin.write_all(text.as_bytes())?;
        }
        drop(child.stdin.take());
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            match child.try_wait()? {
                Some(status) if status.success() => return Ok(program),
                Some(_) => break,
                None if Instant::now() >= deadline => {
                    thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Ok(program);
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    let encoded = BASE64_STANDARD.encode(text);
    let mut stdout = io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()?;
    Ok("OSC 52")
}

fn open_url(url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        anyhow::bail!("only HTTP(S) links can be opened");
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| anyhow::anyhow!("could not start {program}: {error}"))?;
    Ok(())
}

fn refresh_app_view(app: &mut AppState, tmux: &TmuxRuntime<SystemTmuxRunner>) {
    if app.screen != Screen::AppView {
        return;
    }
    let Some((apps, selected)) = app
        .app_view
        .as_ref()
        .map(|view| (view.apps.clone(), view.selected))
    else {
        return;
    };
    let Some(current) = apps.get(selected.min(apps.len().saturating_sub(1))) else {
        app.update_app_view(apps, String::new());
        return;
    };
    match tmux.capture_app(&current.pane_id) {
        Ok(content) => app.update_app_view(apps, content),
        Err(_) => reload_app_view(app, tmux),
    }
}

fn frame_poll_interval(screen: Screen) -> Duration {
    if screen == Screen::AppView {
        Duration::from_millis(33)
    } else {
        Duration::from_millis(100)
    }
}

fn sync_app_viewport(
    app: &mut AppState,
    tmux: &TmuxRuntime<SystemTmuxRunner>,
    app_sizes: &mut HashMap<String, (u16, u16)>,
    session: &TerminalSession,
) {
    let Some(pane_id) = app
        .app_view
        .as_ref()
        .and_then(|view| view.apps.get(view.selected))
        .map(|managed_app| managed_app.pane_id.clone())
    else {
        return;
    };
    let Some(viewport) = app_viewport_size(session) else {
        return;
    };
    if app_sizes.get(&pane_id) == Some(&viewport) {
        return;
    }
    match tmux.resize_app(&pane_id, viewport.0, viewport.1) {
        Ok(()) => {
            app_sizes.insert(pane_id, viewport);
        }
        Err(error) => app.apply_app_view_error(&error),
    }
}

fn app_viewport_size(session: &TerminalSession) -> Option<(u16, u16)> {
    let size = session.terminal.size().ok()?;
    Some((size.width.saturating_sub(2), size.height.saturating_sub(7)))
}

fn reload_app_view(app: &mut AppState, tmux: &TmuxRuntime<SystemTmuxRunner>) {
    let Some(session_name) = app.app_view.as_ref().map(|view| view.session_name.clone()) else {
        return;
    };
    match tmux.list_managed() {
        Ok(sessions) if !sessions.iter().any(|session| session.name == session_name) => {
            app.update_app_view(Vec::new(), String::new());
            return;
        }
        Ok(_) => {}
        Err(error) => {
            app.apply_app_view_error(&error);
            return;
        }
    }
    match tmux.list_apps(&session_name) {
        Ok(apps) => {
            app.update_app_view(apps, String::new());
            refresh_app_view(app, tmux);
        }
        Err(error) => app.apply_app_view_error(&error),
    }
}

fn install_method_requires_privileges(method: &InstallMethod) -> bool {
    matches!(
        method,
        InstallMethod::Apt
            | InstallMethod::Dnf
            | InstallMethod::Pacman
            | InstallMethod::Snap
            | InstallMethod::SnapClassic
            | InstallMethod::Pipx
            | InstallMethod::LazyVim
            | InstallMethod::Tplay
    )
}

fn uninstall_method_requires_privileges(method: &InstallMethod) -> bool {
    matches!(
        method,
        InstallMethod::Apt
            | InstallMethod::Dnf
            | InstallMethod::Pacman
            | InstallMethod::Snap
            | InstallMethod::SnapClassic
            | InstallMethod::Pipx
    )
}

fn acquire_install_privileges(tool_id: &str) -> Result<()> {
    #[cfg(unix)]
    if unsafe { libc::geteuid() } == 0 {
        return Ok(());
    }

    println!("T4E needs administrator access to install {tool_id}.");
    println!("Authenticate with sudo, or press Ctrl+C to cancel.\n");
    let status = Command::new("sudo")
        .arg("-v")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| anyhow::anyhow!("could not start sudo: {error}"))?;
    if !status.success() {
        anyhow::bail!("sudo authentication did not complete");
    }
    Ok(())
}

fn drain_codex_events(app: &mut AppState, codex: &CodexService) {
    while let Ok(event) = codex.try_recv() {
        app.apply_codex_event(event);
    }
}

fn drain_runtime_events(
    app: &mut AppState,
    receiver: &mpsc::Receiver<RuntimeEvent>,
    active: &mut HashMap<String, ActiveInstall>,
) {
    while let Ok(event) = receiver.try_recv() {
        match event {
            RuntimeEvent::Output { tool_id, chunk } => app.record_output(&tool_id, chunk),
            RuntimeEvent::Complete(job) => {
                let tool_id = job.item.tool_id.clone();
                app.apply_execution(*job);
                if let Some(install) = active.remove(&tool_id) {
                    let _ = install.handle.join();
                }
                persist_or_report(app);
            }
            RuntimeEvent::UninstallComplete {
                tool_id,
                success,
                error,
            } => {
                app.apply_uninstall_result(&tool_id, success, &error);
                if let Some(uninstall) = active.remove(&tool_id) {
                    let _ = uninstall.handle.join();
                }
                persist_or_report(app);
            }
        }
    }
}

fn persist_or_report(app: &mut AppState) {
    if let Err(error) = app.persist() {
        app.status = format!("State save failed: {error}");
    }
}

struct TerminalSession {
    terminal: TuiTerminal,
    mouse_capture: bool,
}

impl TerminalSession {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self {
            terminal,
            mouse_capture: false,
        })
    }

    fn set_mouse_capture(&mut self, enabled: bool) -> Result<()> {
        if enabled == self.mouse_capture {
            return Ok(());
        }
        if enabled {
            execute!(self.terminal.backend_mut(), EnableMouseCapture)?;
        } else {
            execute!(self.terminal.backend_mut(), DisableMouseCapture)?;
        }
        self.mouse_capture = enabled;
        Ok(())
    }

    fn suspend_for(&mut self, action: impl FnOnce() -> Result<()>) -> Result<()> {
        disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        self.terminal.show_cursor()?;
        let action_result = action();
        let resume_result = (|| -> Result<()> {
            enable_raw_mode()?;
            execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
            if self.mouse_capture {
                execute!(self.terminal.backend_mut(), EnableMouseCapture)?;
            }
            self.terminal.clear()?;
            Ok(())
        })();
        action_result.and(resume_result)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        frame_poll_interval, install_method_requires_privileges,
        uninstall_method_requires_privileges,
    };
    use crate::app::events::Screen;
    use crate::catalog::models::InstallMethod;
    use std::time::Duration;

    #[test]
    fn app_view_targets_thirty_frames_per_second() {
        assert_eq!(
            frame_poll_interval(Screen::AppView),
            Duration::from_millis(33)
        );
        assert_eq!(
            frame_poll_interval(Screen::Catalog),
            Duration::from_millis(100)
        );
    }

    #[test]
    fn system_package_managers_request_privileges() {
        assert!(install_method_requires_privileges(&InstallMethod::Apt));
        assert!(install_method_requires_privileges(&InstallMethod::Dnf));
        assert!(install_method_requires_privileges(&InstallMethod::Pacman));
        assert!(install_method_requires_privileges(&InstallMethod::Snap));
        assert!(install_method_requires_privileges(
            &InstallMethod::SnapClassic
        ));
        assert!(install_method_requires_privileges(&InstallMethod::Pipx));
        assert!(install_method_requires_privileges(&InstallMethod::LazyVim));
        assert!(install_method_requires_privileges(&InstallMethod::Tplay));
        assert!(!install_method_requires_privileges(&InstallMethod::Brew));
        assert!(!install_method_requires_privileges(&InstallMethod::Cargo));
        assert!(!uninstall_method_requires_privileges(
            &InstallMethod::LazyVim
        ));
    }
}
