use std::collections::HashMap;
use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::codex::service::{CodexCommand, CodexService};
use crate::installer::execution::{
    ExecutionPolicy, InstallExecutor, InstallJob, OutputChunk, SystemCommandRunner,
};
use crate::mux::runtime::{SystemTmuxRunner, TmuxRuntime, attach_interactive};
use crate::mux::tmux::reproducibility_hash;

use super::state::{AppEffect, AppState};
use super::ui::render;

type TuiTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn run(mut app: AppState) -> Result<()> {
    let mut session = TerminalSession::new()?;
    let (event_sender, event_receiver) = mpsc::channel();
    let mut active = HashMap::<String, ActiveInstall>::new();
    let tmux = TmuxRuntime::new(SystemTmuxRunner);
    let environment_context = format!(
        "Catalog tool IDs: {}\nWorkspace IDs: {}",
        app.catalog
            .tools
            .iter()
            .map(|tool| tool.id.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        app.workspaces
            .workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let codex = CodexService::spawn(
        std::env::current_dir()
            .unwrap_or_default()
            .display()
            .to_string(),
        environment_context,
    );
    match tmux.list_managed() {
        Ok(sessions) => app.apply_managed_sessions(sessions),
        Err(error) => app.apply_workspace_error("refresh", &error),
    }

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
        session.terminal.draw(|frame| render(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        {
            app.handle_key(key);
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
    Output { tool_id: String, chunk: OutputChunk },
    Complete(Box<InstallJob>),
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
                let cancel = Arc::new(AtomicBool::new(false));
                let thread_cancel = Arc::clone(&cancel);
                let sender = event_sender.clone();
                let output_tool_id = tool_id.clone();
                let policy = ExecutionPolicy {
                    timeout: Duration::from_secs(app.settings.install_timeout_sec),
                    max_attempts: app.settings.max_install_attempts,
                    log_dir: app.log_dir(),
                };
                app.mark_execution_started(&tool_id);
                let handle = thread::spawn(move || {
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
            AppEffect::AttachWorkspace(session_name) => {
                if let Err(error) = session.suspend_for(|| attach_interactive(&session_name)) {
                    app.apply_workspace_error("attach", &error);
                } else {
                    app.status = format!("Detached from {session_name}");
                }
                match tmux.list_managed() {
                    Ok(sessions) => app.apply_managed_sessions(sessions),
                    Err(error) => app.apply_workspace_error("refresh", &error),
                }
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
                if codex.send(CodexCommand::Prompt(prompt)).is_err() {
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
        Ok(Self { terminal })
    }

    fn suspend_for(&mut self, action: impl FnOnce() -> Result<()>) -> Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        let action_result = action();
        let resume_result = (|| -> Result<()> {
            enable_raw_mode()?;
            execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
            self.terminal.clear()?;
            Ok(())
        })();
        action_result.and(resume_result)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
