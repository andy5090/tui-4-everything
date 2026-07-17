use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::catalog::models::{CatalogRegistry, InstallMethod, Platform, Risk, Tool, ToolCategory};
use crate::codex::service::CodexEvent;
use crate::installer::diagnostics::FailureDiagnostics;
use crate::installer::engine::{InstallPolicy, build_install_task};
use crate::installer::execution::{InstallJob, OutputChunk, OutputStream};
use crate::installer::queue::QueueState;
use crate::mux::runtime::{LaunchOutcome, ManagedApp, ManagedSession};
use crate::mux::tmux::compile_workspace;
use crate::mux::workspace::{Workspace, WorkspaceRegistry};
use crate::storage::{
    PersistentState, RecentItem, UserSettings, load_state, log_dir_for_state, save_state,
};

use super::events::Screen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallConfirmation {
    pub tool_id: String,
    pub command: String,
    pub typed: bool,
    pub expected: String,
    pub input: String,
}

#[derive(Debug, Clone)]
pub enum AppEffect {
    Execute(Box<InstallJob>),
    Cancel(String),
    LaunchTool(ToolLaunchRequest),
    LaunchWorkspace(Box<WorkspaceLaunchRequest>),
    OpenAppView(String),
    SendAppInput { pane_id: String, input: AppInput },
    CloseApp(String),
    SetMouseCapture(bool),
    CopyUrl(String),
    OpenUrl(String),
    Uninstall(UninstallRequest),
    StopWorkspace(String),
    RefreshWorkspaces,
    SnapshotWorkspace(Box<Workspace>),
    CodexPrompt(String),
    CodexInterrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallRequest {
    pub tool_id: String,
    pub command: String,
    pub check_command: String,
    pub method: InstallMethod,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLaunchRequest {
    pub tool_id: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptionSelection {
    pub enabled: bool,
    pub value_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptionsState {
    pub tool_id: String,
    pub tool_name: String,
    pub selected: usize,
    pub selections: Vec<LaunchOptionSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchArgumentState {
    pub tool_id: String,
    pub tool_name: String,
    pub label: String,
    pub placeholder: String,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppInput {
    Text(String),
    Key(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppViewState {
    pub session_name: String,
    pub workspace_title: String,
    pub return_screen: Screen,
    pub apps: Vec<ManagedApp>,
    pub selected: usize,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkAction {
    Open,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPickerState {
    pub urls: Vec<String>,
    pub selected: usize,
    pub action: LinkAction,
}

#[derive(Debug, Clone)]
pub struct WorkspaceLaunchRequest {
    pub workspace: Workspace,
    pub required_tools: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiMessage {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiAction {
    pub kind: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiActionConfirmation {
    pub action: AiAction,
    pub expected: String,
    pub input: String,
}

#[derive(Debug)]
pub struct AppState {
    pub catalog: CatalogRegistry,
    pub workspaces: WorkspaceRegistry,
    pub screen: Screen,
    pub catalog_index: usize,
    pub workspace_index: usize,
    pub agent_index: usize,
    pub install_index: usize,
    pub pack_index: usize,
    pub settings_index: usize,
    pub search_query: String,
    pub search_mode: bool,
    pub show_help: bool,
    pub should_quit: bool,
    pub mouse_enabled: bool,
    pub queue: Vec<InstallJob>,
    pub logs: Vec<String>,
    pub status: String,
    pub confirmation: Option<InstallConfirmation>,
    pub active_pack: Option<String>,
    pub favorites: BTreeSet<String>,
    pub installed_tools: BTreeSet<String>,
    pub uninstalling_tools: BTreeSet<String>,
    pub uninstall_confirmation: Option<UninstallRequest>,
    pub recents: Vec<RecentItem>,
    pub settings: UserSettings,
    pub queue_running: bool,
    pub managed_sessions: Vec<ManagedSession>,
    pub workspace_missing_tools: Vec<String>,
    pub app_view: Option<AppViewState>,
    pub link_picker: Option<LinkPickerState>,
    pub launch_argument: Option<LaunchArgumentState>,
    pub launch_options: Option<LaunchOptionsState>,
    pending_tool_launch: Option<ToolLaunchRequest>,
    return_after_app_close: bool,
    pub ai_account: String,
    pub ai_status: String,
    pub ai_input: String,
    pub ai_input_mode: bool,
    pub ai_messages: Vec<AiMessage>,
    pub ai_streaming: String,
    pub ai_usage: String,
    pub pending_ai_action: Option<AiAction>,
    pub ai_confirmation: Option<AiActionConfirmation>,
    effects: VecDeque<AppEffect>,
    state_path: Option<PathBuf>,
}

impl AppState {
    pub fn new(catalog: CatalogRegistry, workspaces: WorkspaceRegistry) -> Self {
        Self::from_saved(catalog, workspaces, PersistentState::default(), None)
    }

    pub fn persistent(
        catalog: CatalogRegistry,
        workspaces: WorkspaceRegistry,
        state_path: PathBuf,
    ) -> Result<Self> {
        let saved = load_state(&state_path)?;
        Ok(Self::from_saved(
            catalog,
            workspaces,
            saved,
            Some(state_path),
        ))
    }

    fn from_saved(
        catalog: CatalogRegistry,
        workspaces: WorkspaceRegistry,
        mut saved: PersistentState,
        state_path: Option<PathBuf>,
    ) -> Self {
        for job in &mut saved.queue {
            if job.item.state == QueueState::Installing {
                let _ = job.item.transition(QueueState::Failed);
                job.diagnostics = Some(FailureDiagnostics::from_stderr(
                    None,
                    "installation was interrupted before T4E exited",
                    "",
                ));
            }
        }
        reconcile_saved_queue(&catalog, &mut saved);
        saved.logs.push("T4E dashboard started".to_string());
        Self {
            catalog,
            workspaces,
            screen: Screen::Home,
            catalog_index: 0,
            workspace_index: 0,
            agent_index: 0,
            install_index: 0,
            pack_index: 0,
            settings_index: 0,
            search_query: String::new(),
            search_mode: false,
            show_help: false,
            should_quit: false,
            mouse_enabled: false,
            queue: saved.queue,
            logs: saved.logs,
            status: "Ready".to_string(),
            confirmation: None,
            active_pack: None,
            favorites: saved.favorites.into_iter().collect(),
            installed_tools: BTreeSet::new(),
            uninstalling_tools: BTreeSet::new(),
            uninstall_confirmation: None,
            recents: saved.recents,
            settings: saved.settings,
            queue_running: false,
            managed_sessions: Vec::new(),
            workspace_missing_tools: Vec::new(),
            app_view: None,
            link_picker: None,
            launch_argument: None,
            launch_options: None,
            pending_tool_launch: None,
            return_after_app_close: false,
            ai_account: "connecting".to_string(),
            ai_status: "Starting local Codex app-server".to_string(),
            ai_input: String::new(),
            ai_input_mode: false,
            ai_messages: Vec::new(),
            ai_streaming: String::new(),
            ai_usage: "waiting for usage data".to_string(),
            pending_ai_action: None,
            ai_confirmation: None,
            effects: VecDeque::new(),
            state_path,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('m') {
            self.mouse_enabled = !self.mouse_enabled;
            self.effects
                .push_back(AppEffect::SetMouseCapture(self.mouse_enabled));
            self.status = format!(
                "Mouse controls {}; Alt+M toggles",
                if self.mouse_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            return;
        }
        if self.screen == Screen::AppView {
            self.handle_app_view_key(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.launch_argument.is_some() {
            self.handle_launch_argument_key(key.code);
            return;
        }

        if self.launch_options.is_some() {
            self.handle_launch_options_key(key.code);
            return;
        }

        if self.uninstall_confirmation.is_some() {
            self.handle_uninstall_confirmation_key(key.code);
            return;
        }

        if self.confirmation.is_some() {
            self.handle_confirmation_key(key.code);
            return;
        }

        if self.ai_confirmation.is_some() {
            self.handle_ai_confirmation_key(key.code);
            return;
        }

        if self.show_help {
            self.show_help = false;
            return;
        }

        if self.search_mode {
            self.handle_search_key(key.code);
            return;
        }

        if self.ai_input_mode {
            self.handle_ai_input_key(key.code);
            return;
        }

        match key.code {
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Backspace | KeyCode::Esc => self.return_to_main(),
            KeyCode::Char('q') if self.screen == Screen::Home => self.should_quit = true,
            KeyCode::Char('q') => self.screen = Screen::Home,
            KeyCode::Char('1') => self.screen = Screen::Home,
            KeyCode::Char('2') => self.open_all_catalog(),
            KeyCode::Char('3') => self.screen = Screen::Install,
            KeyCode::Char('4') => self.screen = Screen::Workspace,
            KeyCode::Char('5') => self.screen = Screen::Agents,
            KeyCode::Char('6') => self.screen = Screen::Logs,
            KeyCode::Char('7') => self.screen = Screen::Settings,
            _ => self.handle_screen_key(key.code),
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent, terminal_height: u16) {
        if !self.mouse_enabled
            || self.confirmation.is_some()
            || self.ai_confirmation.is_some()
            || self.launch_argument.is_some()
            || self.launch_options.is_some()
            || self.uninstall_confirmation.is_some()
            || self.search_mode
            || self.ai_input_mode
        {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_current_selection(-1),
            MouseEventKind::ScrollDown => self.move_current_selection(1),
            MouseEventKind::Down(MouseButton::Left) if self.screen == Screen::AppView => {
                self.handle_app_view_click(mouse.column, mouse.row, terminal_height);
            }
            MouseEventKind::Down(MouseButton::Left) => self.select_list_row(mouse.row),
            _ => {}
        }
    }

    pub fn visible_catalog_tools(&self) -> Vec<&Tool> {
        let query = self.search_query.to_ascii_lowercase();
        self.catalog
            .tools
            .iter()
            .filter(|tool| tool.category != ToolCategory::Agents || self.active_pack.is_some())
            .filter(|tool| self.active_pack.is_none() || tool.is_launchable_app())
            .filter(|tool| {
                self.active_pack.as_ref().is_none_or(|pack_id| {
                    self.catalog
                        .packs
                        .iter()
                        .find(|pack| &pack.id == pack_id)
                        .is_some_and(|pack| pack.tool_ids.contains(&tool.id))
                })
            })
            .filter(|tool| {
                query.is_empty()
                    || tool.name.to_ascii_lowercase().contains(&query)
                    || tool.id.to_ascii_lowercase().contains(&query)
                    || tool
                        .tags
                        .iter()
                        .any(|tag| tag.to_ascii_lowercase().contains(&query))
            })
            .collect()
    }

    pub fn agent_tools(&self) -> Vec<&Tool> {
        self.catalog
            .tools
            .iter()
            .filter(|tool| tool.category == ToolCategory::Agents)
            .collect()
    }

    pub fn selected_catalog_tool(&self) -> Option<&Tool> {
        self.visible_catalog_tools()
            .get(self.catalog_index)
            .copied()
    }

    pub fn selected_workspace(&self) -> Option<&Workspace> {
        self.workspaces.workspaces.get(self.workspace_index)
    }

    pub fn selected_agent(&self) -> Option<&Tool> {
        self.agent_tools().get(self.agent_index).copied()
    }

    pub fn ai_environment_context(&self) -> String {
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let apps = self
            .catalog
            .tools
            .iter()
            .map(|tool| {
                format!(
                    "{}={} (run: {})",
                    tool.id,
                    tool.name,
                    tool.run_command_for_current_platform()
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let workspaces = self
            .workspaces
            .workspaces
            .iter()
            .map(|workspace| {
                let session = self
                    .managed_sessions
                    .iter()
                    .find(|session| session.workspace_id == workspace.id);
                let runtime = session.map_or_else(
                    || "stopped".to_string(),
                    |session| format!("running as {}", session.name),
                );
                format!(
                    "{}={} (mux: {:?}, apps: {}, state: {})",
                    workspace.id,
                    workspace.title,
                    workspace.mux,
                    workspace.recommended_tools.join(","),
                    runtime
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let queue = if self.queue.is_empty() {
            "empty".to_string()
        } else {
            self.queue
                .iter()
                .map(|job| format!("{}:{:?}", job.item.tool_id, job.item.state))
                .collect::<Vec<_>>()
                .join(", ")
        };

        format!(
            "platform: {platform}\ninstall queue: {queue}\ncatalog apps: {apps}\nworkspaces: {workspaces}"
        )
    }

    pub fn risk_label(risk: &Risk) -> &'static str {
        match risk {
            Risk::Safe => "SAFE",
            Risk::Caution => "CAUTION",
            Risk::Admin => "ADMIN",
            Risk::High => "HIGH",
        }
    }

    pub fn take_effect(&mut self) -> Option<AppEffect> {
        self.effects.pop_front()
    }

    pub fn mark_execution_started(&mut self, tool_id: &str) {
        if let Some(job) = self
            .queue
            .iter_mut()
            .find(|job| job.item.tool_id == tool_id)
        {
            let _ = job.item.transition(QueueState::Installing);
            self.status = format!("Installing {tool_id}");
            self.logs.push(format!("install: started {tool_id}"));
            self.trim_logs();
        }
    }

    pub fn apply_execution(&mut self, mut completed: InstallJob) {
        let tool_id = completed.item.tool_id.clone();
        if self.should_quit
            && let Some(attempt) = completed.attempts.last()
            && attempt.cancelled
        {
            completed.diagnostics = Some(FailureDiagnostics::from_stderr(
                attempt.exit_code,
                "installation interrupted because T4E exited or restarted",
                &attempt.log_path,
            ));
        }
        let state = completed.item.state.clone();
        let already_installed = state == QueueState::Success
            && completed.attempts.is_empty()
            && completed
                .preflight
                .as_ref()
                .is_some_and(|check| check.installed);
        if let Some(job) = self
            .queue
            .iter_mut()
            .find(|job| job.item.tool_id == tool_id)
        {
            *job = completed;
        }
        self.status = match state {
            QueueState::Success if already_installed => {
                format!("{tool_id} is already installed and ready")
            }
            QueueState::Success => format!("Installed {tool_id}"),
            QueueState::Failed => format!("Installation failed for {tool_id}"),
            _ => format!("Installation updated for {tool_id}"),
        };
        if already_installed {
            self.logs
                .push(format!("install: {tool_id} already installed"));
        } else {
            self.logs
                .push(format!("install: {} -> {:?}", tool_id, state));
        }
        if state == QueueState::Success {
            self.installed_tools.insert(tool_id.clone());
            self.push_recent(tool_id.clone(), "tool");
            if self
                .pending_tool_launch
                .as_ref()
                .is_some_and(|request| request.tool_id == tool_id)
                && let Some(request) = self.pending_tool_launch.take()
            {
                self.effects.push_back(AppEffect::LaunchTool(request));
            }
        } else if state == QueueState::Failed
            && self
                .pending_tool_launch
                .as_ref()
                .is_some_and(|request| request.tool_id == tool_id)
        {
            self.pending_tool_launch = None;
        }
        self.trim_logs();
        if self.queue_running {
            self.request_next_queued();
        }
    }

    pub fn apply_install_authorization_error(&mut self, tool_id: &str, error: &anyhow::Error) {
        if self
            .pending_tool_launch
            .as_ref()
            .is_some_and(|request| request.tool_id == tool_id)
        {
            self.pending_tool_launch = None;
        }
        self.queue_running = false;
        self.status = format!("Authorization cancelled for {tool_id}: {error}");
        self.logs
            .push(format!("install: {tool_id} authorization failed: {error}"));
        self.trim_logs();
    }

    pub fn record_output(&mut self, tool_id: &str, chunk: OutputChunk) {
        let stream = match chunk.stream {
            OutputStream::Stdout => "out",
            OutputStream::Stderr => "err",
        };
        for line in chunk.text.lines().filter(|line| !line.trim().is_empty()) {
            self.logs
                .push(format!("{tool_id} [{stream}]: {}", line.trim_end()));
        }
        self.trim_logs();
    }

    pub fn persist(&self) -> Result<()> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        save_state(
            path,
            &PersistentState {
                queue: self.queue.clone(),
                logs: self.logs.clone(),
                favorites: self.favorites.iter().cloned().collect(),
                recents: self.recents.clone(),
                settings: self.settings.clone(),
            },
        )
    }

    pub fn log_dir(&self) -> PathBuf {
        self.state_path
            .as_deref()
            .map(log_dir_for_state)
            .unwrap_or_else(|| Path::new("artifacts").join("install-logs"))
    }

    pub fn apply_managed_sessions(&mut self, sessions: Vec<ManagedSession>) {
        self.managed_sessions = sessions;
        self.status = format!("{} running workspaces", self.managed_sessions.len());
    }

    pub fn apply_installed_tools(&mut self, tool_ids: BTreeSet<String>) {
        self.installed_tools = tool_ids;
    }

    pub fn mark_uninstall_started(&mut self, tool_id: &str) {
        self.uninstalling_tools.insert(tool_id.to_string());
        self.status = format!("Uninstalling {tool_id}");
        self.logs.push(format!("uninstall: started {tool_id}"));
        self.trim_logs();
    }

    pub fn apply_uninstall_result(&mut self, tool_id: &str, success: bool, error: &str) {
        self.uninstalling_tools.remove(tool_id);
        if success {
            self.installed_tools.remove(tool_id);
            self.queue.retain(|job| job.item.tool_id != tool_id);
            self.install_index = self.install_index.min(self.queue.len().saturating_sub(1));
            self.status = format!("Uninstalled {tool_id}");
            self.logs.push(format!("uninstall: completed {tool_id}"));
        } else {
            self.status = format!("Uninstall failed for {tool_id}: {error}");
            self.logs
                .push(format!("uninstall: failed {tool_id}: {error}"));
        }
        self.trim_logs();
    }

    pub fn apply_workspace_launch(&mut self, outcome: LaunchOutcome) {
        self.workspace_missing_tools.clear();
        self.status = if outcome.created {
            format!("Launched workspace {}", outcome.workspace_id)
        } else {
            format!("Session {} is already running", outcome.session_name)
        };
        self.logs.push(format!(
            "workspace: {} {}",
            if outcome.created {
                "launched"
            } else {
                "reused"
            },
            outcome.session_name
        ));
        self.push_recent(outcome.workspace_id, "workspace");
        self.effects.push_back(AppEffect::RefreshWorkspaces);
        self.effects
            .push_back(AppEffect::OpenAppView(outcome.session_name));
        self.trim_logs();
    }

    pub fn open_app_view(&mut self, session_name: String, apps: Vec<ManagedApp>) {
        if apps.is_empty() {
            self.status = "Workspace has no running apps".to_string();
            return;
        }
        let workspace_title = self
            .workspaces
            .workspaces
            .iter()
            .find(|workspace| workspace.session_name.as_deref() == Some(&session_name))
            .map(|workspace| workspace.title.clone())
            .unwrap_or_else(|| "Running apps".to_string());
        let return_screen = if self.screen == Screen::AppView {
            self.app_view
                .as_ref()
                .map_or(Screen::Home, |view| view.return_screen)
        } else {
            self.screen
        };
        self.app_view = Some(AppViewState {
            session_name,
            workspace_title,
            return_screen,
            apps,
            selected: 0,
            content: String::new(),
        });
        self.link_picker = None;
        self.screen = Screen::AppView;
        self.return_after_app_close = false;
        self.status = "App controls are available in the T4E toolbar".to_string();
    }

    pub fn remember_app_view(&mut self, session_name: String, apps: Vec<ManagedApp>) {
        if apps.is_empty() {
            return;
        }
        self.app_view = Some(AppViewState {
            session_name,
            workspace_title: "Running apps".to_string(),
            return_screen: self.screen,
            apps,
            selected: 0,
            content: String::new(),
        });
        self.link_picker = None;
    }

    pub fn focus_app(&mut self, app_id: &str) {
        let Some(view) = &mut self.app_view else {
            return;
        };
        if let Some(index) = view.apps.iter().position(|app| app.window_name == app_id) {
            view.selected = index;
            view.content.clear();
        }
    }

    pub fn update_app_view(&mut self, apps: Vec<ManagedApp>, content: String) {
        let Some(view) = &mut self.app_view else {
            return;
        };
        view.apps = apps;
        if self.return_after_app_close {
            self.return_after_app_close = false;
            self.leave_app_view("App closed");
            return;
        }
        if view.apps.is_empty() {
            self.leave_app_view("App closed");
            return;
        }
        view.selected = view.selected.min(view.apps.len() - 1);
        view.content = content;
    }

    pub fn apply_app_view_error(&mut self, error: &anyhow::Error) {
        self.return_after_app_close = false;
        self.leave_app_view(&format!("App view closed: {error}"));
    }

    pub fn apply_workspace_preflight_failure(&mut self, missing_tools: Vec<String>) {
        self.workspace_missing_tools = missing_tools;
        self.status = format!(
            "Workspace needs {} tools; press I to queue them",
            self.workspace_missing_tools.len()
        );
    }

    pub fn apply_workspace_error(&mut self, action: &str, error: &anyhow::Error) {
        self.status = format!("Workspace {action} failed: {error}");
        self.logs
            .push(format!("workspace: {action} failed: {error}"));
        self.trim_logs();
    }

    pub fn apply_workspace_hash(&mut self, workspace_id: &str, hash: &str) {
        self.status = format!("{workspace_id} snapshot {}", &hash[..12.min(hash.len())]);
        self.logs
            .push(format!("workspace: snapshot {workspace_id} {hash}"));
        self.trim_logs();
    }

    pub fn apply_codex_event(&mut self, event: CodexEvent) {
        match event {
            CodexEvent::Ready { account } => {
                self.ai_account = account;
                self.ai_status = "Ready".to_string();
            }
            CodexEvent::ThreadStarted(id) => {
                self.ai_status = format!("Thread {}", short_id(&id));
            }
            CodexEvent::TurnStarted(id) => {
                self.ai_streaming.clear();
                self.ai_status = format!("Working {}", short_id(&id));
            }
            CodexEvent::Delta(delta) => self.ai_streaming.push_str(&delta),
            CodexEvent::Message(text) => {
                self.ai_streaming.clear();
                self.ai_messages.push(AiMessage {
                    role: "Codex".to_string(),
                    text,
                });
                if self.ai_messages.len() > 50 {
                    self.ai_messages.remove(0);
                }
            }
            CodexEvent::ActionProposed { kind, target } => match kind.as_str() {
                "catalog_search" => {
                    self.active_pack = None;
                    self.search_query = target.clone();
                    self.catalog_index = 0;
                    self.screen = Screen::Catalog;
                    self.status = format!("AI searched the catalog for {target}");
                }
                "install_plan" if self.catalog.tools.iter().any(|tool| tool.id == target) => {
                    self.active_pack = None;
                    self.search_query = target.clone();
                    self.catalog_index = 0;
                    self.screen = Screen::Catalog;
                    self.status = format!("Review the install plan for {target}");
                }
                "workspace_launch"
                    if self
                        .workspaces
                        .workspaces
                        .iter()
                        .any(|workspace| workspace.id == target) =>
                {
                    self.pending_ai_action = Some(AiAction { kind, target });
                    self.ai_status = "Workspace launch proposed; press A to approve".to_string();
                }
                _ => {
                    self.ai_status = format!("Rejected unsupported AI action {kind}:{target}");
                }
            },
            CodexEvent::Usage(usage) => self.ai_usage = usage,
            CodexEvent::TurnCompleted(status) => {
                self.ai_status = self.pending_ai_action.as_ref().map_or_else(
                    || format!("Turn {status}"),
                    |action| {
                        format!(
                            "Approval required for {} {}; press A to review",
                            action.kind, action.target
                        )
                    },
                );
            }
            CodexEvent::ApprovalDenied(method) => {
                self.ai_status = format!("Denied app-server request {method}");
                self.logs.push(format!("codex: denied {method}"));
            }
            CodexEvent::Diagnostic(diagnostic) => {
                self.logs.push(format!("codex diagnostic: {diagnostic}"));
            }
            CodexEvent::Error(error) => {
                self.ai_status = format!("Error: {error}");
                self.logs.push(format!("codex: {error}"));
            }
        }
        self.trim_logs();
    }

    fn handle_search_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Enter => self.search_mode = false,
            KeyCode::Backspace => {
                self.search_query.pop();
                self.catalog_index = 0;
            }
            KeyCode::Char(ch) => {
                self.search_query.push(ch);
                self.catalog_index = 0;
            }
            _ => {}
        }
    }

    fn handle_ai_input_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.ai_input_mode = false;
                self.ai_input.clear();
                self.ai_status = "Input cancelled".to_string();
            }
            KeyCode::Backspace => {
                self.ai_input.pop();
            }
            KeyCode::Enter => {
                let prompt = self.ai_input.trim().to_string();
                if !prompt.is_empty() {
                    self.ai_messages.push(AiMessage {
                        role: "You".to_string(),
                        text: prompt.clone(),
                    });
                    self.effects.push_back(AppEffect::CodexPrompt(prompt));
                }
                self.ai_input.clear();
                self.ai_input_mode = false;
            }
            KeyCode::Char(ch) => self.ai_input.push(ch),
            _ => {}
        }
    }

    fn begin_ai_action_confirmation(&mut self) {
        let Some(action) = self.pending_ai_action.take() else {
            self.ai_status = "No AI action is awaiting approval".to_string();
            return;
        };
        self.ai_confirmation = Some(AiActionConfirmation {
            expected: format!("APPROVE {} {}", action.kind, action.target),
            action,
            input: String::new(),
        });
        self.ai_status = "Type the approval phrase exactly".to_string();
    }

    fn handle_ai_confirmation_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.ai_confirmation = None;
                self.ai_status = "AI action approval cancelled".to_string();
            }
            KeyCode::Backspace => {
                if let Some(confirmation) = &mut self.ai_confirmation {
                    confirmation.input.pop();
                }
            }
            KeyCode::Char(ch) => {
                if let Some(confirmation) = &mut self.ai_confirmation {
                    confirmation.input.push(ch);
                }
            }
            KeyCode::Enter => {
                let Some(confirmation) = self.ai_confirmation.take() else {
                    return;
                };
                if confirmation.input != confirmation.expected {
                    self.ai_status = "AI action approval phrase did not match".to_string();
                    return;
                }
                if confirmation.action.kind == "workspace_launch"
                    && let Some(index) = self
                        .workspaces
                        .workspaces
                        .iter()
                        .position(|workspace| workspace.id == confirmation.action.target)
                {
                    self.workspace_index = index;
                    self.screen = Screen::Workspace;
                    self.request_workspace_launch();
                    self.logs.push(format!(
                        "codex: approved workspace_launch {}",
                        confirmation.action.target
                    ));
                }
            }
            _ => {}
        }
    }

    fn handle_screen_key(&mut self, code: KeyCode) {
        match self.screen {
            Screen::Home => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_pack(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_pack(-1),
                KeyCode::Enter => self.open_selected_pack(),
                KeyCode::Char('I') => self.queue_selected_pack(),
                KeyCode::Char('c') => self.open_all_catalog(),
                KeyCode::Char('i') => self.screen = Screen::Install,
                KeyCode::Char('w') => self.screen = Screen::Workspace,
                KeyCode::Char('a') => self.screen = Screen::Agents,
                KeyCode::Char('l') => self.screen = Screen::Logs,
                KeyCode::Char('s') => self.screen = Screen::Settings,
                _ => {}
            },
            Screen::Catalog => match code {
                KeyCode::Char('/') => self.search_mode = true,
                KeyCode::Down | KeyCode::Char('j') => self.move_catalog(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_catalog(-1),
                KeyCode::Enter => self.request_selected_tool_launch(),
                KeyCode::Char('I') => self.queue_selected_tool(),
                KeyCode::Char('U') => self.request_selected_uninstall(),
                KeyCode::Char('f') => self.toggle_favorite(),
                KeyCode::Char('p') => {
                    self.active_pack = None;
                    self.catalog_index = 0;
                    self.status = "Showing all catalog tools".to_string();
                }
                _ => {}
            },
            Screen::Install => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_install(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_install(-1),
                KeyCode::Char('r') => self.retry_selected(),
                KeyCode::Char('x') => self.request_execute_selected(),
                KeyCode::Char('X') => self.request_execute_queue(),
                KeyCode::Char('c') => self.request_cancel_selected(),
                KeyCode::Char('d') => self.remove_selected_job(),
                _ => {}
            },
            Screen::Workspace => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_workspace(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_workspace(-1),
                KeyCode::Enter => self.request_workspace_launch(),
                KeyCode::Char('a') => self.request_workspace_attach(),
                KeyCode::Char('x') => self.request_workspace_stop(),
                KeyCode::Char('r') => self.effects.push_back(AppEffect::RefreshWorkspaces),
                KeyCode::Char('h') => self.request_workspace_snapshot(),
                KeyCode::Char('I') => self.queue_workspace_requirements(),
                _ => {}
            },
            Screen::AppView => {}
            Screen::Agents => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_agent(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_agent(-1),
                KeyCode::Enter | KeyCode::Char('i') => {
                    self.ai_input_mode = true;
                    self.ai_status = "Compose request".to_string();
                }
                KeyCode::Char('x') => self.effects.push_back(AppEffect::CodexInterrupt),
                KeyCode::Char('A') => self.begin_ai_action_confirmation(),
                _ => {}
            },
            Screen::Logs => {
                if matches!(code, KeyCode::Char('c')) {
                    self.logs.clear();
                    self.status = "Activity log cleared".to_string();
                }
            }
            Screen::Settings => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_setting(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_setting(-1),
                KeyCode::Left | KeyCode::Char('h') => self.adjust_setting(-1),
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => self.adjust_setting(1),
                _ => {}
            },
        }
    }

    fn handle_app_view_key(&mut self, key: KeyEvent) {
        if self.link_picker.is_some() {
            self.handle_link_picker_key(key.code);
            return;
        }
        match key.code {
            KeyCode::BackTab | KeyCode::F(6) => self.move_app_view(-1),
            KeyCode::Tab | KeyCode::F(7) => self.move_app_view(1),
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.request_close_current_app();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.request_app_url(false);
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.request_app_url(true);
            }
            KeyCode::Backspace if key.modifiers.contains(KeyModifiers::ALT) => {
                self.background_app_view();
            }
            _ => {
                let Some(input) = app_input_from_key(key) else {
                    return;
                };
                if let Some(pane_id) = self
                    .app_view
                    .as_ref()
                    .and_then(|view| view.apps.get(view.selected))
                    .map(|app| app.pane_id.clone())
                {
                    self.effects
                        .push_back(AppEffect::SendAppInput { pane_id, input });
                }
            }
        }
    }

    fn request_close_current_app(&mut self) {
        if let Some(pane_id) = self
            .app_view
            .as_ref()
            .and_then(|view| view.apps.get(view.selected))
            .map(|app| app.pane_id.clone())
        {
            self.return_after_app_close = true;
            self.effects.push_back(AppEffect::CloseApp(pane_id));
        }
    }

    fn request_app_url(&mut self, open: bool) {
        let Some(content) = self.app_view.as_ref().map(|view| view.content.as_str()) else {
            return;
        };
        let mut urls = extract_urls(content);
        urls.reverse();
        let mut seen = BTreeSet::new();
        urls.retain(|url| seen.insert(url.clone()));
        if urls.is_empty() {
            self.status = "No HTTP(S) link found in the current app".to_string();
            return;
        }
        let action = if open {
            LinkAction::Open
        } else {
            LinkAction::Copy
        };
        if urls.len() == 1 {
            self.apply_link_action(action, urls.remove(0));
        } else {
            self.link_picker = Some(LinkPickerState {
                urls,
                selected: 0,
                action,
            });
            self.status = "Select a link from the current app".to_string();
        }
    }

    fn handle_link_picker_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(picker) = &mut self.link_picker {
                    picker.selected = picker.selected.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(picker) = &mut self.link_picker {
                    picker.selected = (picker.selected + 1).min(picker.urls.len() - 1);
                }
            }
            KeyCode::Enter => {
                let Some(picker) = self.link_picker.take() else {
                    return;
                };
                if let Some(url) = picker.urls.get(picker.selected).cloned() {
                    self.apply_link_action(picker.action, url);
                }
            }
            KeyCode::Esc => {
                self.link_picker = None;
                self.status = "Link selection cancelled".to_string();
            }
            _ => {}
        }
    }

    fn apply_link_action(&mut self, action: LinkAction, url: String) {
        self.effects.push_back(match action {
            LinkAction::Open => AppEffect::OpenUrl(url),
            LinkAction::Copy => AppEffect::CopyUrl(url),
        });
    }

    fn leave_app_view(&mut self, status: &str) {
        let return_screen = self
            .app_view
            .as_ref()
            .map_or(Screen::Home, |view| view.return_screen);
        self.app_view = None;
        self.link_picker = None;
        self.return_after_app_close = false;
        self.screen = return_screen;
        self.status = status.to_string();
    }

    fn background_app_view(&mut self) {
        let return_screen = self
            .app_view
            .as_ref()
            .map_or(Screen::Home, |view| view.return_screen);
        self.return_after_app_close = false;
        self.screen = return_screen;
        self.status = "Returned; apps remain running".to_string();
    }

    fn handle_app_view_click(&mut self, column: u16, row: u16, terminal_height: u16) {
        if row <= 2 {
            let Some(view) = &mut self.app_view else {
                return;
            };
            let mut start = 1_u16;
            for (index, app) in view.apps.iter().enumerate() {
                let width = app.window_name.chars().count() as u16 + 2;
                if column >= start && column < start.saturating_add(width) {
                    view.selected = index;
                    view.content.clear();
                    return;
                }
                start = start.saturating_add(width + 3);
            }
        } else if row >= terminal_height.saturating_sub(2) {
            match column {
                0..=11 => self.move_app_view(1),
                12..=33 => self.move_app_view(-1),
                34..=51 => {
                    self.background_app_view();
                }
                _ => self.request_close_current_app(),
            }
        }
    }

    fn move_current_selection(&mut self, delta: isize) {
        match self.screen {
            Screen::Home => self.move_pack(delta),
            Screen::Catalog => self.move_catalog(delta),
            Screen::Install => self.move_install(delta),
            Screen::Workspace => self.move_workspace(delta),
            Screen::AppView => self.move_app_view(delta),
            Screen::Agents => self.move_agent(delta),
            Screen::Settings => self.move_setting(delta),
            Screen::Logs => {}
        }
    }

    fn select_list_row(&mut self, row: u16) {
        let Some(index) = row.checked_sub(4).map(usize::from) else {
            return;
        };
        match self.screen {
            Screen::Home if index < self.catalog.packs.len() => self.pack_index = index,
            Screen::Catalog if index < self.visible_catalog_tools().len() => {
                self.catalog_index = index;
            }
            Screen::Install if index < self.queue.len() => self.install_index = index,
            Screen::Workspace if index < self.workspaces.workspaces.len() => {
                self.workspace_index = index;
            }
            Screen::Agents if index < self.agent_tools().len() => self.agent_index = index,
            Screen::Settings if index < 4 => self.settings_index = index,
            Screen::AppView
            | Screen::Logs
            | Screen::Home
            | Screen::Catalog
            | Screen::Install
            | Screen::Workspace
            | Screen::Agents
            | Screen::Settings => {}
        }
    }

    fn move_app_view(&mut self, delta: isize) {
        let Some(view) = &mut self.app_view else {
            return;
        };
        view.selected = move_index(view.selected, view.apps.len(), delta);
        view.content.clear();
    }

    fn move_catalog(&mut self, delta: isize) {
        self.catalog_index = move_index(
            self.catalog_index,
            self.visible_catalog_tools().len(),
            delta,
        );
    }

    fn move_workspace(&mut self, delta: isize) {
        self.workspace_index = move_index(
            self.workspace_index,
            self.workspaces.workspaces.len(),
            delta,
        );
    }

    fn move_agent(&mut self, delta: isize) {
        self.agent_index = move_index(self.agent_index, self.agent_tools().len(), delta);
    }

    fn move_install(&mut self, delta: isize) {
        self.install_index = move_index(self.install_index, self.queue.len(), delta);
    }

    fn move_pack(&mut self, delta: isize) {
        self.pack_index = move_index(self.pack_index, self.catalog.packs.len(), delta);
    }

    fn move_setting(&mut self, delta: isize) {
        self.settings_index = move_index(self.settings_index, 4, delta);
    }

    fn queue_selected_tool(&mut self) {
        let Some(tool) = self.selected_catalog_tool() else {
            self.status = "No catalog tool selected".to_string();
            return;
        };
        self.queue_tool_ids(vec![tool.id.clone()]);
    }

    fn request_selected_uninstall(&mut self) {
        let Some(tool) = self.selected_catalog_tool() else {
            return;
        };
        let tool_id = tool.id.clone();
        if !self.installed_tools.contains(&tool_id) {
            self.status = format!("{tool_id} is not installed");
            return;
        }
        let platform = if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Linux
        };
        let Some(installer) = tool
            .installers
            .iter()
            .find(|installer| installer.platform == platform)
        else {
            self.status = format!("No uninstall method for {tool_id}");
            return;
        };
        let packages = if installer.method == InstallMethod::Cargo {
            installer.package_hints.join(" ")
        } else {
            installer.package_hints[0].clone()
        };
        let Some(command) = uninstall_command(&installer.method, &packages) else {
            self.status = format!("Uninstall is unavailable for {tool_id}");
            return;
        };
        let check_command = installer
            .executable
            .clone()
            .or_else(|| tool.checks.iter().find_map(|check| check.which.clone()))
            .or_else(|| {
                tool.run_command_for_current_platform()
                    .split_whitespace()
                    .next()
                    .map(str::to_string)
            })
            .unwrap_or_else(|| tool_id.clone());
        self.uninstall_confirmation = Some(UninstallRequest {
            tool_id,
            command,
            check_command,
            method: installer.method.clone(),
        });
        self.status = "Confirm app uninstall".to_string();
    }

    fn handle_uninstall_confirmation_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.uninstall_confirmation = None;
                self.status = "Uninstall cancelled".to_string();
            }
            KeyCode::Enter => {
                if let Some(request) = self.uninstall_confirmation.take() {
                    self.effects.push_back(AppEffect::Uninstall(request));
                }
            }
            _ => {}
        }
    }

    fn request_selected_tool_launch(&mut self) {
        let Some(tool) = self.selected_catalog_tool() else {
            self.status = "No app selected".to_string();
            return;
        };
        let tool_id = tool.id.clone();
        let tool_name = tool.name.clone();
        let command = tool.run_command_for_current_platform().to_string();
        let launch_argument = tool.launch_argument.clone();
        let options = tool.run_options.clone();
        if let Some(index) = self
            .app_view
            .as_ref()
            .and_then(|view| view.apps.iter().position(|app| app.window_name == tool_id))
        {
            if let Some(view) = &mut self.app_view {
                view.return_screen = self.screen;
                view.selected = index;
                view.content.clear();
            }
            self.screen = Screen::AppView;
            self.status = format!("Returned to {tool_name}");
            return;
        }
        if let Some(argument) = launch_argument {
            self.launch_argument = Some(LaunchArgumentState {
                tool_id,
                tool_name,
                label: argument.label,
                placeholder: argument.placeholder,
                input: String::new(),
            });
            self.status = "Enter the required launch value".to_string();
            return;
        }
        if options.is_empty() {
            self.effects
                .push_back(AppEffect::LaunchTool(ToolLaunchRequest {
                    tool_id,
                    command,
                }));
            self.status = format!("Opening {tool_name}");
            return;
        }
        let selections = options
            .iter()
            .map(|option| LaunchOptionSelection {
                enabled: option.default_enabled,
                value_index: option
                    .default_value
                    .as_ref()
                    .and_then(|default| option.values.iter().position(|value| value == default))
                    .unwrap_or_default(),
            })
            .collect();
        self.launch_options = Some(LaunchOptionsState {
            tool_id,
            tool_name,
            selected: 0,
            selections,
        });
        self.status = "Configure launch options".to_string();
    }

    fn handle_launch_argument_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.launch_argument = None;
                self.status = "Launch cancelled".to_string();
            }
            KeyCode::Backspace => {
                if let Some(argument) = &mut self.launch_argument {
                    argument.input.pop();
                }
            }
            KeyCode::Char(ch) => {
                if let Some(argument) = &mut self.launch_argument {
                    argument.input.push(ch);
                }
            }
            KeyCode::Enter => {
                let Some(argument) = self.launch_argument.take() else {
                    return;
                };
                let value = argument.input.trim();
                if value.is_empty() {
                    self.launch_argument = Some(argument);
                    self.status = "A launch value is required".to_string();
                    return;
                }
                let Some(tool) = self
                    .catalog
                    .tools
                    .iter()
                    .find(|tool| tool.id == argument.tool_id)
                else {
                    self.status = "Launch tool is no longer available".to_string();
                    return;
                };
                self.effects
                    .push_back(AppEffect::LaunchTool(ToolLaunchRequest {
                        tool_id: tool.id.clone(),
                        command: format!(
                            "{} {}",
                            tool.run_command_for_current_platform(),
                            shell_quote(value)
                        ),
                    }));
                self.status = format!("Opening {}", tool.name);
            }
            _ => {}
        }
    }

    fn handle_launch_options_key(&mut self, code: KeyCode) {
        let option_count = self
            .launch_options
            .as_ref()
            .map_or(0, |options| options.selections.len());
        match code {
            KeyCode::Esc => {
                self.launch_options = None;
                self.status = "Launch cancelled".to_string();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(options) = &mut self.launch_options {
                    options.selected = move_index(options.selected, option_count, -1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(options) = &mut self.launch_options {
                    options.selected = move_index(options.selected, option_count, 1);
                }
            }
            KeyCode::Char(' ') => {
                if let Some(options) = &mut self.launch_options
                    && let Some(selection) = options.selections.get_mut(options.selected)
                {
                    selection.enabled = !selection.enabled;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.move_launch_option_value(-1),
            KeyCode::Right | KeyCode::Char('l') => self.move_launch_option_value(1),
            KeyCode::Enter => self.confirm_launch_options(),
            _ => {}
        }
    }

    fn move_launch_option_value(&mut self, delta: isize) {
        let Some(options) = &mut self.launch_options else {
            return;
        };
        let Some(tool) = self
            .catalog
            .tools
            .iter()
            .find(|tool| tool.id == options.tool_id)
        else {
            return;
        };
        let Some(option) = tool.run_options.get(options.selected) else {
            return;
        };
        let Some(selection) = options.selections.get_mut(options.selected) else {
            return;
        };
        if !option.values.is_empty() {
            selection.value_index = move_index(selection.value_index, option.values.len(), delta);
        }
    }

    fn confirm_launch_options(&mut self) {
        let Some(options) = self.launch_options.take() else {
            return;
        };
        let Some(tool) = self
            .catalog
            .tools
            .iter()
            .find(|tool| tool.id == options.tool_id)
        else {
            self.status = "Launch option tool is no longer available".to_string();
            return;
        };
        let mut command = tool.run_command_for_current_platform().to_string();
        for (option, selection) in tool.run_options.iter().zip(&options.selections) {
            if !selection.enabled {
                continue;
            }
            command.push(' ');
            command.push_str(&option.flag);
            if let Some(value) = option.values.get(selection.value_index) {
                command.push(' ');
                command.push_str(value);
            }
        }
        self.effects
            .push_back(AppEffect::LaunchTool(ToolLaunchRequest {
                tool_id: tool.id.clone(),
                command,
            }));
        self.status = format!("Opening {}", tool.name);
    }

    pub fn install_then_launch(&mut self, request: ToolLaunchRequest) {
        let tool_id = request.tool_id.clone();
        self.pending_tool_launch = Some(request);

        if let Some(index) = self
            .queue
            .iter()
            .position(|job| job.item.tool_id == tool_id)
        {
            self.install_index = index;
            match self.queue[index].item.state {
                QueueState::Failed => self.retry_selected(),
                QueueState::Success => {
                    self.queue.remove(index);
                    self.queue_tool_ids(vec![tool_id.clone()]);
                }
                QueueState::Installing => {
                    self.status = format!("Installing {tool_id}; it will open when ready");
                    return;
                }
                QueueState::Queued | QueueState::Idle => {}
            }
        } else {
            self.queue_tool_ids(vec![tool_id.clone()]);
        }

        self.status = format!("{tool_id} is not installed; installing before launch");
        self.request_execute_selected();
    }

    fn return_to_main(&mut self) {
        if self.screen == Screen::Catalog && self.active_pack.is_some() {
            self.active_pack = None;
            self.search_query.clear();
        }
        self.screen = Screen::Home;
    }

    fn open_selected_pack(&mut self) {
        let Some(pack) = self.catalog.packs.get(self.pack_index) else {
            return;
        };
        self.active_pack = Some(pack.id.clone());
        self.catalog_index = 0;
        self.screen = Screen::Catalog;
        self.status = format!("Viewing {}", pack.title);
    }

    fn open_all_catalog(&mut self) {
        self.active_pack = None;
        self.search_query.clear();
        self.search_mode = false;
        self.catalog_index = 0;
        self.screen = Screen::Catalog;
        self.status = "Showing all catalog tools".to_string();
    }

    fn queue_selected_pack(&mut self) {
        let Some(pack) = self.catalog.packs.get(self.pack_index) else {
            return;
        };
        let ids = pack.tool_ids.clone();
        self.queue_tool_ids(ids);
    }

    fn queue_tool_ids(&mut self, tool_ids: Vec<String>) {
        let mut added = 0_usize;
        let mut skipped = 0_usize;
        for tool_id in tool_ids {
            if self.queue.iter().any(|job| job.item.tool_id == tool_id) {
                skipped += 1;
                continue;
            }
            let Some(task) = self.build_current_task(&tool_id) else {
                skipped += 1;
                continue;
            };
            let channel = task.method.channel_name().to_string();
            self.queue.push(InstallJob::new(task, channel));
            self.logs.push(format!("queue: added {tool_id}"));
            added += 1;
        }
        self.install_index = self.queue.len().saturating_sub(1);
        self.status = format!("Queued {added} tools; skipped {skipped}");
        self.trim_logs();
    }

    fn toggle_favorite(&mut self) {
        let Some(tool_id) = self.selected_catalog_tool().map(|tool| tool.id.clone()) else {
            return;
        };
        let favorite = self.favorites.insert(tool_id.clone());
        if !favorite {
            self.favorites.remove(&tool_id);
        }
        self.status = if favorite {
            format!("Added {tool_id} to favorites")
        } else {
            format!("Removed {tool_id} from favorites")
        };
    }

    fn retry_selected(&mut self) {
        let Some(job) = self.queue.get_mut(self.install_index) else {
            self.status = "Install queue is empty".to_string();
            return;
        };

        if job.item.state != QueueState::Failed {
            self.status = format!("{} is not in a failed state", job.item.tool_id);
            return;
        }

        let tool_id = job.item.tool_id.clone();
        if job.item.transition(QueueState::Queued).is_ok() {
            self.status = format!("Queued {} for retry", tool_id);
            self.logs.push(format!("queue: retry {}", tool_id));
        }
    }

    fn request_execute_selected(&mut self) {
        let Some(job) = self.queue.get(self.install_index) else {
            self.status = "Install queue is empty".to_string();
            return;
        };
        if job.item.state != QueueState::Queued {
            self.status = format!("{} is not queued", job.item.tool_id);
            return;
        }
        let Some(current_task) = self.build_current_task(&job.item.tool_id) else {
            self.status = format!(
                "Cannot verify the saved install plan for {}",
                job.item.tool_id
            );
            return;
        };
        if install_plan_changed(&job.task, &current_task) {
            self.status = format!(
                "Saved plan for {} is stale; remove and queue it again",
                job.item.tool_id
            );
            return;
        }

        if job.task.requires_confirmation || self.settings.confirm_all_installs {
            let tool_id = job.item.tool_id.clone();
            let typed = job.task.requires_confirmation;
            self.confirmation = Some(InstallConfirmation {
                command: job.task.command.clone(),
                expected: format!("INSTALL {tool_id}"),
                input: String::new(),
                typed,
                tool_id,
            });
            self.status = if typed {
                "Type the confirmation phrase exactly".to_string()
            } else {
                "Press Enter to approve this SAFE installation".to_string()
            };
        } else {
            self.effects
                .push_back(AppEffect::Execute(Box::new(job.clone())));
        }
    }

    fn request_execute_queue(&mut self) {
        if self.queue_running {
            self.status = "Install queue is already running".to_string();
            return;
        }
        self.queue_running = true;
        self.request_next_queued();
    }

    fn request_next_queued(&mut self) {
        let Some(index) = self
            .queue
            .iter()
            .position(|job| job.item.state == QueueState::Queued)
        else {
            self.queue_running = false;
            self.status = "Install queue run completed".to_string();
            return;
        };
        self.install_index = index;
        let pending_effects = self.effects.len();
        self.request_execute_selected();
        if self.confirmation.is_some() {
            self.status = format!(
                "Queue paused for approval of {}",
                self.queue[index].item.tool_id
            );
        } else if self.effects.len() == pending_effects {
            self.queue_running = false;
        }
    }

    fn request_cancel_selected(&mut self) {
        let Some(job) = self.queue.get(self.install_index) else {
            return;
        };
        if job.item.state == QueueState::Installing {
            self.queue_running = false;
            self.effects
                .push_back(AppEffect::Cancel(job.item.tool_id.clone()));
            self.status = format!("Cancelling {}", job.item.tool_id);
        } else {
            self.status = format!("{} is not running", job.item.tool_id);
        }
    }

    fn remove_selected_job(&mut self) {
        let Some(job) = self.queue.get(self.install_index) else {
            return;
        };
        if job.item.state == QueueState::Installing {
            self.status = "Cancel the running job before removing it".to_string();
            return;
        }
        let tool_id = job.item.tool_id.clone();
        self.queue.remove(self.install_index);
        self.install_index = self.install_index.min(self.queue.len().saturating_sub(1));
        self.status = format!("Removed {tool_id} from the install queue");
        self.logs.push(format!("queue: removed {tool_id}"));
    }

    fn build_current_task(&self, tool_id: &str) -> Option<crate::installer::engine::InstallTask> {
        build_current_task(&self.catalog, tool_id)
    }

    fn handle_confirmation_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.confirmation = None;
                self.queue_running = false;
                self.pending_tool_launch = None;
                self.status = "Installation confirmation cancelled".to_string();
            }
            KeyCode::Backspace => {
                if let Some(confirmation) = &mut self.confirmation
                    && confirmation.typed
                {
                    confirmation.input.pop();
                }
            }
            KeyCode::Enter => {
                let Some(confirmation) = self.confirmation.take() else {
                    return;
                };
                if !confirmation.typed || confirmation.input == confirmation.expected {
                    if let Some(job) = self
                        .queue
                        .iter()
                        .find(|job| job.item.tool_id == confirmation.tool_id)
                    {
                        self.effects
                            .push_back(AppEffect::Execute(Box::new(job.clone())));
                    }
                } else {
                    self.queue_running = false;
                    self.status = "Confirmation phrase did not match".to_string();
                }
            }
            KeyCode::Char(ch) => {
                if let Some(confirmation) = &mut self.confirmation
                    && confirmation.typed
                {
                    confirmation.input.push(ch);
                }
            }
            _ => {}
        }
    }

    fn request_workspace_launch(&mut self) {
        let Some(workspace) = self.selected_workspace().cloned() else {
            return;
        };
        if !matches!(workspace.mux, crate::mux::workspace::MuxBackend::Tmux) {
            self.status = "Zellij runtime is not enabled yet".to_string();
            return;
        }
        if let Err(error) = compile_workspace(
            &workspace,
            workspace.session_name.as_deref().unwrap_or(&workspace.id),
            "main",
        ) {
            self.apply_workspace_error("validation", &error);
            return;
        }
        let required_tools = workspace
            .recommended_tools
            .iter()
            .filter_map(|id| {
                self.catalog
                    .tools
                    .iter()
                    .find(|tool| &tool.id == id)
                    .map(|tool| {
                        (
                            id.clone(),
                            tool.run_command_for_current_platform()
                                .split_whitespace()
                                .next()
                                .unwrap_or(tool.id.as_str())
                                .to_string(),
                        )
                    })
            })
            .collect();
        self.effects.push_back(AppEffect::LaunchWorkspace(Box::new(
            WorkspaceLaunchRequest {
                workspace,
                required_tools,
            },
        )));
    }

    fn request_workspace_attach(&mut self) {
        let Some(session) = self.selected_workspace_session() else {
            self.status = "Selected workspace has no running session".to_string();
            return;
        };
        self.effects.push_back(AppEffect::OpenAppView(session));
    }

    fn request_workspace_stop(&mut self) {
        let Some(session) = self.selected_workspace_session() else {
            self.status = "Selected workspace has no running session".to_string();
            return;
        };
        self.effects.push_back(AppEffect::StopWorkspace(session));
    }

    fn request_workspace_snapshot(&mut self) {
        let Some(workspace) = self.selected_workspace().cloned() else {
            return;
        };
        if self.selected_workspace_session().is_none() {
            self.status = "Launch the workspace before taking a snapshot".to_string();
            return;
        }
        self.effects
            .push_back(AppEffect::SnapshotWorkspace(Box::new(workspace)));
    }

    fn queue_workspace_requirements(&mut self) {
        let ids = if self.workspace_missing_tools.is_empty() {
            self.selected_workspace()
                .map(|workspace| workspace.recommended_tools.clone())
                .unwrap_or_default()
        } else {
            self.workspace_missing_tools.clone()
        };
        self.queue_tool_ids(ids);
    }

    fn selected_workspace_session(&self) -> Option<String> {
        let workspace = self.selected_workspace()?;
        self.managed_sessions
            .iter()
            .find(|session| session.workspace_id == workspace.id)
            .map(|session| session.name.clone())
    }

    fn adjust_setting(&mut self, delta: isize) {
        match self.settings_index {
            0 => {
                const OPTIONS: [&str; 3] = ["tmux", "zellij", "none"];
                let current = OPTIONS
                    .iter()
                    .position(|value| *value == self.settings.default_mux)
                    .unwrap_or_default();
                let next = (current as isize + delta).rem_euclid(OPTIONS.len() as isize) as usize;
                self.settings.default_mux = OPTIONS[next].to_string();
            }
            1 => {
                let next = self.settings.install_timeout_sec as i64 + delta as i64 * 60;
                self.settings.install_timeout_sec = next.clamp(60, 3600) as u64;
            }
            2 => {
                let next = self.settings.max_install_attempts as isize + delta;
                self.settings.max_install_attempts = next.clamp(1, 5) as u32;
            }
            3 => {
                self.settings.confirm_all_installs = !self.settings.confirm_all_installs;
            }
            _ => {}
        }
        self.status = "Settings updated".to_string();
    }

    fn push_recent(&mut self, id: String, kind: &str) {
        self.recents
            .retain(|item| !(item.id == id && item.kind == kind));
        self.recents.insert(
            0,
            RecentItem {
                id,
                kind: kind.to_string(),
                timestamp: chrono::Utc::now(),
            },
        );
        self.recents.truncate(20);
    }

    fn trim_logs(&mut self) {
        const MAX_LOG_LINES: usize = 500;
        if self.logs.len() > MAX_LOG_LINES {
            self.logs.drain(0..self.logs.len() - MAX_LOG_LINES);
        }
    }
}

fn reconcile_saved_queue(catalog: &CatalogRegistry, saved: &mut PersistentState) {
    let mut refreshed = Vec::new();
    for job in &mut saved.queue {
        if !matches!(job.item.state, QueueState::Queued | QueueState::Failed) {
            continue;
        }
        let Some(current_task) = build_current_task(catalog, &job.item.tool_id) else {
            continue;
        };
        if !install_plan_changed(&job.task, &current_task) {
            continue;
        }

        job.item.channel = current_task.method.channel_name().to_string();
        if job.item.state == QueueState::Failed {
            let _ = job.item.transition(QueueState::Queued);
        }
        job.item.attempts = 0;
        job.task = current_task;
        job.attempts.clear();
        job.preflight = None;
        job.postflight = None;
        job.diagnostics = None;
        refreshed.push(job.item.tool_id.clone());
    }
    saved.logs.extend(
        refreshed
            .into_iter()
            .map(|tool_id| format!("queue: refreshed stale install plan for {tool_id}")),
    );
}

fn build_current_task(
    catalog: &CatalogRegistry,
    tool_id: &str,
) -> Option<crate::installer::engine::InstallTask> {
    let platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    };
    let tool = catalog.tools.iter().find(|tool| tool.id == tool_id)?;
    let installer = tool
        .installers
        .iter()
        .find(|installer| installer.platform == platform)?;
    build_install_task(tool, installer, &InstallPolicy::default()).ok()
}

fn install_plan_changed(
    saved: &crate::installer::engine::InstallTask,
    current: &crate::installer::engine::InstallTask,
) -> bool {
    saved.command != current.command
        || saved.method != current.method
        || saved.check_command != current.check_command
        || saved.additional_check_commands != current.additional_check_commands
        || saved.install_timeout_sec != current.install_timeout_sec
        || saved.requires_privileges != current.requires_privileges
        || saved.requires_confirmation != current.requires_confirmation
}

fn uninstall_command(method: &InstallMethod, package: &str) -> Option<String> {
    let command = match method {
        InstallMethod::Brew => format!("brew uninstall {package}"),
        InstallMethod::BrewCask => format!("brew uninstall --cask {package}"),
        InstallMethod::Apt => {
            format!(
                "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 remove -y {package}"
            )
        }
        InstallMethod::Dnf => format!("sudo -n dnf remove -y {package}"),
        InstallMethod::Pacman => format!("sudo -n pacman -Rns --noconfirm {package}"),
        InstallMethod::Snap | InstallMethod::SnapClassic => {
            format!("sudo -n snap remove {package}")
        }
        InstallMethod::Pipx => format!("pipx uninstall {package}"),
        InstallMethod::NpmGlobal => format!("npm uninstall --global {package}"),
        InstallMethod::Cargo => format!("cargo uninstall {package}"),
        InstallMethod::LazyVim => "rm -f \"$HOME/.local/bin/t4e-lazyvim\" && rm -rf \"${XDG_CONFIG_HOME:-$HOME/.config}/t4e-lazyvim\" \"${XDG_DATA_HOME:-$HOME/.local/share}/t4e-lazyvim\" \"${XDG_STATE_HOME:-$HOME/.local/state}/t4e-lazyvim\" \"${XDG_CACHE_HOME:-$HOME/.cache}/t4e-lazyvim\"".to_string(),
        InstallMethod::Go | InstallMethod::Script | InstallMethod::Other => return None,
    };
    Some(command)
}

fn app_input_from_key(key: KeyEvent) -> Option<AppInput> {
    let modifier = if key.modifiers.contains(KeyModifiers::CONTROL) {
        Some("C")
    } else if key.modifiers.contains(KeyModifiers::ALT) {
        Some("M")
    } else if key.modifiers.contains(KeyModifiers::SHIFT) {
        Some("S")
    } else {
        None
    };

    if let KeyCode::Char(ch) = key.code {
        return match modifier {
            None | Some("S") => Some(AppInput::Text(ch.to_string())),
            Some(prefix) => {
                let name = if ch == ' ' {
                    "Space".to_string()
                } else {
                    ch.to_ascii_lowercase().to_string()
                };
                Some(AppInput::Key(format!("{prefix}-{name}")))
            }
        };
    }

    let name = match key.code {
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Escape".to_string(),
        KeyCode::Backspace => "BSpace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BTab".to_string(),
        KeyCode::Delete => "DC".to_string(),
        KeyCode::Insert => "IC".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PPage".to_string(),
        KeyCode::PageDown => "NPage".to_string(),
        KeyCode::F(number) => format!("F{number}"),
        _ => return None,
    };
    Some(AppInput::Key(
        modifier.map_or(name.clone(), |prefix| format!("{prefix}-{name}")),
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn extract_urls(content: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut offset = 0;
    while offset < content.len() {
        let remaining = &content[offset..];
        let next = [remaining.find("https://"), remaining.find("http://")]
            .into_iter()
            .flatten()
            .min();
        let Some(start) = next else {
            break;
        };
        let absolute_start = offset + start;
        let tail = &content[absolute_start..];
        let end = tail
            .char_indices()
            .find(|(_, ch)| {
                ch.is_whitespace()
                    || ch.is_control()
                    || matches!(ch, '"' | '\'' | '<' | '>' | '[' | ']')
            })
            .map_or(tail.len(), |(index, _)| index);
        let url = tail[..end]
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '}'])
            .to_string();
        if !url.is_empty() {
            urls.push(url);
        }
        offset = absolute_start + end.max(1);
    }
    urls
}

fn move_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(len as isize) as usize
}

fn short_id(value: &str) -> &str {
    &value[..value.len().min(12)]
}
