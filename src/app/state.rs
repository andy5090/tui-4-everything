use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::catalog::models::{CatalogRegistry, Platform, Risk, Tool, ToolCategory};
use crate::codex::service::CodexEvent;
use crate::installer::diagnostics::FailureDiagnostics;
use crate::installer::engine::{InstallPolicy, build_install_task};
use crate::installer::execution::{InstallJob, OutputChunk, OutputStream};
use crate::installer::queue::QueueState;
use crate::mux::runtime::{LaunchOutcome, ManagedSession};
use crate::mux::tmux::compile_workspace;
use crate::mux::workspace::{Workspace, WorkspaceRegistry};
use crate::storage::{
    PersistentState, RecentItem, UserSettings, load_state, log_dir_for_state, save_state,
};

use super::events::Screen;

const SCREENS: [Screen; 7] = [
    Screen::Home,
    Screen::Catalog,
    Screen::Install,
    Screen::Workspace,
    Screen::Agents,
    Screen::Logs,
    Screen::Settings,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallConfirmation {
    pub tool_id: String,
    pub command: String,
    pub expected: String,
    pub input: String,
}

#[derive(Debug, Clone)]
pub enum AppEffect {
    Execute(Box<InstallJob>),
    Cancel(String),
    LaunchWorkspace(Box<WorkspaceLaunchRequest>),
    AttachWorkspace(String),
    StopWorkspace(String),
    RefreshWorkspaces,
    SnapshotWorkspace(Box<Workspace>),
    CodexPrompt(String),
    CodexInterrupt,
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
    pub queue: Vec<InstallJob>,
    pub logs: Vec<String>,
    pub status: String,
    pub confirmation: Option<InstallConfirmation>,
    pub selected_tools: BTreeSet<String>,
    pub active_pack: Option<String>,
    pub favorites: BTreeSet<String>,
    pub recents: Vec<RecentItem>,
    pub settings: UserSettings,
    pub queue_running: bool,
    pub managed_sessions: Vec<ManagedSession>,
    pub workspace_missing_tools: Vec<String>,
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
                    "installation was interrupted before t4e exited",
                    "",
                ));
            }
        }
        saved.logs.push("t4e dashboard started".to_string());
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
            queue: saved.queue,
            logs: saved.logs,
            status: "Ready".to_string(),
            confirmation: None,
            selected_tools: BTreeSet::new(),
            active_pack: None,
            favorites: saved.favorites.into_iter().collect(),
            recents: saved.recents,
            settings: saved.settings,
            queue_running: false,
            managed_sessions: Vec::new(),
            workspace_missing_tools: Vec::new(),
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
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
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
            KeyCode::Tab => self.cycle_screen(1),
            KeyCode::BackTab => self.cycle_screen(-1),
            KeyCode::Esc => self.screen = Screen::Home,
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

    pub fn visible_catalog_tools(&self) -> Vec<&Tool> {
        let query = self.search_query.to_ascii_lowercase();
        self.catalog
            .tools
            .iter()
            .filter(|tool| tool.category != ToolCategory::Agents)
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

    pub fn apply_execution(&mut self, completed: InstallJob) {
        let tool_id = completed.item.tool_id.clone();
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
            self.push_recent(tool_id, "tool");
        }
        self.trim_logs();
        if self.queue_running {
            self.request_next_queued();
        }
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
        self.status = format!("{} managed tmux sessions", self.managed_sessions.len());
    }

    pub fn apply_workspace_launch(&mut self, outcome: LaunchOutcome) {
        self.workspace_missing_tools.clear();
        self.status = if outcome.created {
            format!("Launched tmux session {}", outcome.session_name)
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
        self.trim_logs();
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
                KeyCode::Enter => self.queue_selected_tool(),
                KeyCode::Char(' ') => self.toggle_selected_tool(),
                KeyCode::Char('a') => self.toggle_all_visible_tools(),
                KeyCode::Char('I') => self.queue_selected_tools(),
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

    fn cycle_screen(&mut self, delta: isize) {
        let current = SCREENS
            .iter()
            .position(|screen| *screen == self.screen)
            .unwrap_or_default();
        let next = (current as isize + delta).rem_euclid(SCREENS.len() as isize) as usize;
        self.screen = SCREENS[next];
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

    fn toggle_selected_tool(&mut self) {
        let Some(tool_id) = self.selected_catalog_tool().map(|tool| tool.id.clone()) else {
            return;
        };
        if !self.selected_tools.insert(tool_id.clone()) {
            self.selected_tools.remove(&tool_id);
        }
        self.status = format!("{} tools selected", self.selected_tools.len());
    }

    fn toggle_all_visible_tools(&mut self) {
        let visible = self
            .visible_catalog_tools()
            .into_iter()
            .map(|tool| tool.id.clone())
            .collect::<Vec<_>>();
        let all_selected = visible
            .iter()
            .all(|tool_id| self.selected_tools.contains(tool_id));
        if all_selected {
            for tool_id in visible {
                self.selected_tools.remove(&tool_id);
            }
        } else {
            self.selected_tools.extend(visible);
        }
        self.status = format!("{} tools selected", self.selected_tools.len());
    }

    fn queue_selected_tools(&mut self) {
        if self.selected_tools.is_empty() {
            self.queue_selected_tool();
            return;
        }
        self.queue_tool_ids(self.selected_tools.iter().cloned().collect());
        self.selected_tools.clear();
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
            let channel = format!("{:?}", task.method).to_ascii_lowercase();
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
        if current_task.command != job.task.command
            || current_task.method != job.task.method
            || current_task.check_command != job.task.check_command
            || current_task.requires_confirmation != job.task.requires_confirmation
        {
            self.status = format!(
                "Saved plan for {} is stale; remove and queue it again",
                job.item.tool_id
            );
            return;
        }

        if job.task.requires_confirmation || self.settings.confirm_all_installs {
            let tool_id = job.item.tool_id.clone();
            self.confirmation = Some(InstallConfirmation {
                command: job.task.command.clone(),
                expected: format!("INSTALL {tool_id}"),
                input: String::new(),
                tool_id,
            });
            self.status = "Type the confirmation phrase exactly".to_string();
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
        let platform = if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Linux
        };
        let tool = self.catalog.tools.iter().find(|tool| tool.id == tool_id)?;
        let installer = tool
            .installers
            .iter()
            .find(|installer| installer.platform == platform)?;
        build_install_task(tool, installer, &InstallPolicy::default()).ok()
    }

    fn handle_confirmation_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.confirmation = None;
                self.queue_running = false;
                self.status = "Installation confirmation cancelled".to_string();
            }
            KeyCode::Backspace => {
                if let Some(confirmation) = &mut self.confirmation {
                    confirmation.input.pop();
                }
            }
            KeyCode::Enter => {
                let Some(confirmation) = self.confirmation.take() else {
                    return;
                };
                if confirmation.input == confirmation.expected {
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
                if let Some(confirmation) = &mut self.confirmation {
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
                            tool.run
                                .cmd
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
        self.effects.push_back(AppEffect::AttachWorkspace(session));
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

fn move_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(len as isize) as usize
}

fn short_id(value: &str) -> &str {
    &value[..value.len().min(12)]
}
