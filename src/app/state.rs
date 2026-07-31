use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::ai::service::{AiEvent, AiProvider, ProviderReadiness};
use crate::catalog::models::{
    AppCategory, CatalogRegistry, InstallMethod, OutputFilter, Platform, RiskLevel, RunOption,
    Tool, ToolCategory,
};
use crate::codex::service::CodexEvent;
use crate::installer::diagnostics::FailureDiagnostics;
use crate::installer::engine::{InstallPolicy, build_install_task, build_verified_update_task};
use crate::installer::execution::{InstallJob, OutputChunk, OutputStream};
use crate::installer::queue::QueueState;
use crate::mux::runtime::{LaunchOutcome, ManagedApp, ManagedSession};
use crate::mux::tmux::compile_workspace;
use crate::mux::workspace::{Workspace, WorkspaceRegistry};
use crate::storage::{
    LaunchOptionPreference, PersistentState, RecentItem, UserSettings, load_state,
    log_dir_for_state, save_state,
};
use crate::system_info::{SystemOverview, cached_system_overview};

use super::events::Screen;

pub const NAVIGATION_TAB_LABELS: [&str; 4] = ["HOME", "Activity", "Settings", "Help"];

fn navigation_tab_label(index: usize) -> &'static str {
    NAVIGATION_TAB_LABELS.get(index).copied().unwrap_or("HOME")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeFilter {
    AllApps,
    Installed,
    Favorites,
    Recent,
    Running,
    Category(AppCategory),
}

impl HomeFilter {
    pub const ALL: [Self; 14] = [
        Self::Running,
        Self::Favorites,
        Self::Recent,
        Self::AllApps,
        Self::Installed,
        Self::Category(AppCategory::Internet),
        Self::Category(AppCategory::Media),
        Self::Category(AppCategory::Files),
        Self::Category(AppCategory::Editors),
        Self::Category(AppCategory::Ai),
        Self::Category(AppCategory::System),
        Self::Category(AppCategory::Utilities),
        Self::Category(AppCategory::Games),
        Self::Category(AppCategory::Entertainment),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AllApps => "All Apps",
            Self::Installed => "Installed",
            Self::Favorites => "Favorites",
            Self::Recent => "Recent",
            Self::Running => "Running",
            Self::Category(category) => category.label(),
        }
    }

    pub fn is_category(self) -> bool {
        matches!(self, Self::Category(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeFocus {
    Views,
    AppList,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolUpdateState {
    Current { installed: String, verified: String },
    Drift { installed: String, verified: String },
    Error(String),
}

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
    CopySelection { start: (u16, u16), end: (u16, u16) },
    ReadAppLinks { pane_id: String, action: LinkAction },
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
    pub reinstall: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLaunchRequest {
    pub tool_id: String,
    pub command: String,
    pub keep_open: bool,
    pub output_filter: Option<OutputFilter>,
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
    pub command: String,
    pub argument: Option<String>,
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

#[derive(Debug, Clone)]
pub struct LaunchApproval {
    pub tool_name: String,
    pub request: ToolLaunchRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseSelection {
    pub start: (u16, u16),
    pub end: (u16, u16),
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
    pub activity_scroll: usize,
    pub home_filter_index: usize,
    pub home_app_index: usize,
    pub home_focus: HomeFocus,
    pub settings_index: usize,
    pub system_overview: SystemOverview,
    pub search_query: String,
    pub search_mode: bool,
    pub should_quit: bool,
    pub mouse_enabled: bool,
    pub mouse_selection: Option<MouseSelection>,
    pub queue: Vec<InstallJob>,
    pub logs: Vec<String>,
    pub status: String,
    pub confirmation: Option<InstallConfirmation>,
    pub favorites: BTreeSet<String>,
    pub installed_tools: BTreeSet<String>,
    pub tool_updates: BTreeMap<String, ToolUpdateState>,
    pub uninstalling_tools: BTreeSet<String>,
    pub uninstall_confirmation: Option<UninstallRequest>,
    pub recents: Vec<RecentItem>,
    pub settings: UserSettings,
    pub launch_preferences: BTreeMap<String, BTreeMap<String, LaunchOptionPreference>>,
    pub queue_running: bool,
    pub managed_sessions: Vec<ManagedSession>,
    pub workspace_missing_tools: Vec<String>,
    pub app_view: Option<AppViewState>,
    pub link_picker: Option<LinkPickerState>,
    pub launch_argument: Option<LaunchArgumentState>,
    pub launch_options: Option<LaunchOptionsState>,
    pub launch_approval: Option<LaunchApproval>,
    pending_tool_launch: Option<ToolLaunchRequest>,
    pending_tool_install: Option<String>,
    pending_tool_configuration: Option<String>,
    installed_scan_complete: bool,
    approved_camera_tools: BTreeSet<String>,
    return_after_app_close: bool,
    pub ai_account: String,
    pub ai_provider: AiProvider,
    pub ai_ready_providers: BTreeMap<AiProvider, String>,
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
        saved.logs.push(timestamp_log("T4E dashboard started"));
        let mouse_enabled = saved.settings.mouse_enabled;
        Self {
            catalog,
            workspaces,
            screen: Screen::Home,
            catalog_index: 0,
            workspace_index: 0,
            agent_index: 0,
            install_index: 0,
            activity_scroll: 0,
            home_filter_index: 3,
            home_app_index: 0,
            home_focus: HomeFocus::Views,
            settings_index: 0,
            system_overview: cached_system_overview(),
            search_query: String::new(),
            search_mode: false,
            should_quit: false,
            mouse_enabled,
            mouse_selection: None,
            queue: saved.queue,
            logs: saved.logs,
            status: "Ready".to_string(),
            confirmation: None,
            favorites: saved.favorites.into_iter().collect(),
            installed_tools: BTreeSet::new(),
            tool_updates: BTreeMap::new(),
            uninstalling_tools: BTreeSet::new(),
            uninstall_confirmation: None,
            recents: saved.recents,
            settings: saved.settings,
            launch_preferences: saved.launch_preferences,
            queue_running: false,
            managed_sessions: Vec::new(),
            workspace_missing_tools: Vec::new(),
            app_view: None,
            link_picker: None,
            launch_argument: None,
            launch_options: None,
            launch_approval: None,
            pending_tool_launch: None,
            pending_tool_install: None,
            pending_tool_configuration: None,
            installed_scan_complete: false,
            approved_camera_tools: BTreeSet::new(),
            return_after_app_close: false,
            ai_account: "not connected".to_string(),
            ai_provider: AiProvider::Codex,
            ai_ready_providers: BTreeMap::new(),
            ai_status: "AI unavailable · connect a provider in Settings".to_string(),
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
            self.settings.mouse_enabled = self.mouse_enabled;
            self.mouse_selection = None;
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
        if self.screen == Screen::Home
            && key.modifiers.contains(KeyModifiers::ALT)
            && key.code == KeyCode::Char('q')
        {
            self.search_mode = false;
            self.should_quit = true;
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

        if self.launch_approval.is_some() {
            self.handle_launch_approval_key(key.code);
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

        if self.search_mode {
            self.handle_search_key(key.code);
            return;
        }

        if self.ai_input_mode {
            self.handle_ai_input_key(key.code);
            return;
        }

        match key.code {
            KeyCode::Char('?') => self.screen = Screen::Help,
            KeyCode::Backspace | KeyCode::Esc => self.return_to_main(),
            KeyCode::Tab if self.screen == Screen::Home => self.move_home_focus(1),
            KeyCode::BackTab if self.screen == Screen::Home => self.move_home_focus(-1),
            KeyCode::Tab => self.move_navigation_tab(1),
            KeyCode::BackTab => self.move_navigation_tab(-1),
            KeyCode::Char('q') if self.screen == Screen::Home => self.should_quit = true,
            KeyCode::Char('q') => self.screen = Screen::Home,
            KeyCode::Char('1') => self.screen = Screen::Home,
            KeyCode::Char('2') => self.open_all_catalog(),
            KeyCode::Char('3') => self.screen = Screen::Install,
            KeyCode::Char('4') => {
                self.screen = Screen::Home;
                self.focus_ai();
            }
            KeyCode::Char('5') | KeyCode::Char('6') => self.screen = Screen::Logs,
            KeyCode::Char('7') => self.screen = Screen::Settings,
            KeyCode::Char('8') => self.screen = Screen::Help,
            _ => self.handle_screen_key(key.code),
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent, terminal_height: u16) {
        if !self.mouse_enabled
            || self.confirmation.is_some()
            || self.ai_confirmation.is_some()
            || self.launch_argument.is_some()
            || self.launch_options.is_some()
            || self.launch_approval.is_some()
            || self.uninstall_confirmation.is_some()
            || self.search_mode
            || self.ai_input_mode
        {
            self.mouse_selection = None;
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.mouse_selection = Some(MouseSelection {
                    start: (mouse.column, mouse.row),
                    end: (mouse.column, mouse.row),
                });
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(selection) = &mut self.mouse_selection {
                    selection.end = (mouse.column, mouse.row);
                }
                return;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(mut selection) = self.mouse_selection.take() {
                    selection.end = (mouse.column, mouse.row);
                    if selection.start != selection.end {
                        self.effects.push_back(AppEffect::CopySelection {
                            start: selection.start,
                            end: selection.end,
                        });
                    }
                }
                return;
            }
            _ => {}
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_current_selection(-1),
            MouseEventKind::ScrollDown => self.move_current_selection(1),
            MouseEventKind::Down(MouseButton::Left) if self.screen == Screen::AppView => {
                self.handle_app_view_click(mouse.column, mouse.row, terminal_height);
            }
            MouseEventKind::Down(MouseButton::Left) if mouse.row < 3 => {
                self.select_navigation_tab_at(mouse.column);
            }
            MouseEventKind::Down(MouseButton::Left)
                if self.screen == Screen::Home
                    && mouse.column < 24
                    && (3..=5).contains(&mouse.row) =>
            {
                self.mouse_selection = None;
                self.begin_home_search();
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.select_list_row(mouse.column, mouse.row)
            }
            _ => {}
        }
    }

    pub fn navigation_tab_index(&self) -> usize {
        match self.screen {
            Screen::Home | Screen::Catalog | Screen::Install => 0,
            Screen::Workspace => 0,
            Screen::Agents => 0,
            Screen::Logs => 1,
            Screen::Settings => 2,
            Screen::Help => 3,
            Screen::AppView => 0,
        }
    }

    fn move_navigation_tab(&mut self, delta: isize) {
        let index = move_index(
            self.navigation_tab_index(),
            NAVIGATION_TAB_LABELS.len(),
            delta,
        );
        self.open_navigation_tab(index);
    }

    fn open_navigation_tab(&mut self, index: usize) {
        self.screen = match index {
            0 => Screen::Home,
            1 => Screen::Logs,
            2 => Screen::Settings,
            3 => Screen::Help,
            _ => return,
        };
        self.status = format!("Opened {}", navigation_tab_label(index));
    }

    fn select_navigation_tab_at(&mut self, column: u16) {
        let mut start = 1_u16;
        for (index, label) in NAVIGATION_TAB_LABELS.iter().enumerate() {
            let end = start.saturating_add(label.len() as u16);
            if column >= start && column < end {
                self.open_navigation_tab(index);
                return;
            }
            start = end.saturating_add(3);
        }
    }

    pub fn visible_catalog_tools(&self) -> Vec<&Tool> {
        let query = self.search_query.to_ascii_lowercase();
        self.catalog
            .tools
            .iter()
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

    pub fn selected_home_filter(&self) -> HomeFilter {
        HomeFilter::ALL
            .get(self.home_filter_index)
            .copied()
            .unwrap_or(HomeFilter::AllApps)
    }

    pub fn home_tools(&self) -> Vec<&Tool> {
        let filter = self.selected_home_filter();
        let query = self.search_query.to_ascii_lowercase();
        let matches_query = |tool: &&Tool| {
            query.is_empty()
                || tool.name.to_ascii_lowercase().contains(&query)
                || tool.id.to_ascii_lowercase().contains(&query)
                || tool
                    .tags
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(&query))
        };

        if self.search_mode || !query.is_empty() {
            return self
                .catalog
                .tools
                .iter()
                .filter(|tool| tool.is_launchable_app())
                .filter(matches_query)
                .collect();
        }

        match filter {
            HomeFilter::Recent => self
                .recents
                .iter()
                .filter(|recent| recent.kind == "tool")
                .filter_map(|recent| {
                    self.catalog
                        .tools
                        .iter()
                        .find(|tool| tool.id == recent.id && tool.is_launchable_app())
                })
                .filter(matches_query)
                .collect(),
            HomeFilter::Running => self
                .app_view
                .as_ref()
                .into_iter()
                .flat_map(|view| &view.apps)
                .filter_map(|running| {
                    self.catalog
                        .tools
                        .iter()
                        .find(|tool| tool.id == running.window_name && tool.is_launchable_app())
                })
                .filter(matches_query)
                .collect(),
            _ => self
                .catalog
                .tools
                .iter()
                .filter(|tool| tool.is_launchable_app())
                .filter(|tool| match filter {
                    HomeFilter::AllApps => true,
                    HomeFilter::Installed => self.installed_tools.contains(&tool.id),
                    HomeFilter::Favorites => self.favorites.contains(&tool.id),
                    HomeFilter::Category(category) => tool.app_category() == category,
                    HomeFilter::Recent | HomeFilter::Running => unreachable!(),
                })
                .filter(matches_query)
                .collect(),
        }
    }

    pub fn selected_home_tool(&self) -> Option<&Tool> {
        self.home_tools().get(self.home_app_index).copied()
    }

    pub fn home_filter_count(&self, filter: HomeFilter) -> usize {
        match filter {
            HomeFilter::AllApps => self
                .catalog
                .tools
                .iter()
                .filter(|tool| tool.is_launchable_app())
                .count(),
            HomeFilter::Installed => self
                .catalog
                .tools
                .iter()
                .filter(|tool| tool.is_launchable_app() && self.installed_tools.contains(&tool.id))
                .count(),
            HomeFilter::Favorites => self
                .catalog
                .tools
                .iter()
                .filter(|tool| tool.is_launchable_app() && self.favorites.contains(&tool.id))
                .count(),
            HomeFilter::Recent => self
                .recents
                .iter()
                .filter(|item| {
                    item.kind == "tool"
                        && self
                            .catalog
                            .tools
                            .iter()
                            .any(|tool| tool.id == item.id && tool.is_launchable_app())
                })
                .count(),
            HomeFilter::Running => self.app_view.as_ref().map_or(0, |view| {
                view.apps
                    .iter()
                    .filter(|running| {
                        self.catalog
                            .tools
                            .iter()
                            .any(|tool| tool.id == running.window_name && tool.is_launchable_app())
                    })
                    .count()
            }),
            HomeFilter::Category(category) => self
                .catalog
                .tools
                .iter()
                .filter(|tool| tool.is_launchable_app() && tool.app_category() == category)
                .count(),
        }
    }

    pub fn is_tool_running(&self, tool_id: &str) -> bool {
        self.app_view.as_ref().is_some_and(|view| {
            view.apps
                .iter()
                .any(|running| running.window_name == tool_id)
        })
    }

    fn selected_action_tool(&self) -> Option<&Tool> {
        if self.screen == Screen::Home {
            self.selected_home_tool()
        } else {
            self.selected_catalog_tool()
        }
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
                let state = match self.tool_updates.get(&tool.id) {
                    Some(ToolUpdateState::Current { verified, .. }) => {
                        format!("installed, T4E-verified {verified}")
                    }
                    Some(ToolUpdateState::Drift {
                        installed,
                        verified,
                    }) => {
                        format!("installed {installed}, verified target {verified}")
                    }
                    Some(ToolUpdateState::Error(_)) => "installed, version unknown".to_string(),
                    None if self.installed_tools.contains(&tool.id) => "installed".to_string(),
                    None => "not installed".to_string(),
                };
                format!(
                    "{}={} ({state}; run: {})",
                    tool.id,
                    tool.name,
                    tool.run_command_for_current_platform()
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

        format!("platform: {platform}\ninstall queue: {queue}\ncatalog apps: {apps}")
    }

    pub fn risk_label(risk: RiskLevel) -> &'static str {
        risk.label()
    }

    pub fn take_effect(&mut self) -> Option<AppEffect> {
        self.effects.pop_front()
    }

    pub fn record_log(&mut self, message: impl AsRef<str>) {
        self.logs.push(timestamp_log(message));
        self.activity_scroll = 0;
        self.trim_logs();
    }

    pub fn mark_execution_started(&mut self, tool_id: &str) {
        if let Some(job) = self
            .queue
            .iter_mut()
            .find(|job| job.item.tool_id == tool_id)
        {
            let _ = job.item.transition(QueueState::Installing);
            self.status = format!("Installing {tool_id}");
            self.logs
                .push(timestamp_log(format!("install: started {tool_id}")));
            self.trim_logs();
        }
    }

    pub fn apply_execution(&mut self, mut completed: InstallJob) {
        let tool_id = completed.item.tool_id.clone();
        let verified_version = completed.task.expected_version.clone();
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
            self.logs.push(timestamp_log(format!(
                "install: {tool_id} already installed"
            )));
        } else {
            self.logs.push(timestamp_log(format!(
                "install: {} -> {:?}",
                tool_id, state
            )));
        }
        if state == QueueState::Success {
            self.installed_tools.insert(tool_id.clone());
            if let Some(verified) = verified_version {
                self.tool_updates.insert(
                    tool_id.clone(),
                    ToolUpdateState::Current {
                        installed: verified.clone(),
                        verified,
                    },
                );
            }
            self.push_recent(tool_id.clone(), "tool");
            if self.pending_tool_install.as_deref() == Some(tool_id.as_str()) {
                self.pending_tool_install = None;
                if let Some(request) = self.pending_tool_launch.take() {
                    self.effects.push_back(AppEffect::LaunchTool(request));
                } else if self.pending_tool_configuration.as_deref() == Some(tool_id.as_str()) {
                    self.pending_tool_configuration = None;
                    self.request_tool_configuration(&tool_id);
                }
            }
        } else if state == QueueState::Failed
            && self.pending_tool_install.as_deref() == Some(tool_id.as_str())
        {
            self.pending_tool_launch = None;
            self.pending_tool_install = None;
            self.pending_tool_configuration = None;
        }
        self.trim_logs();
        if self.queue_running {
            self.request_next_queued();
        }
    }

    pub fn apply_install_authorization_error(&mut self, tool_id: &str, error: &anyhow::Error) {
        if self.pending_tool_install.as_deref() == Some(tool_id) {
            self.pending_tool_launch = None;
            self.pending_tool_install = None;
            self.pending_tool_configuration = None;
        }
        self.queue_running = false;
        self.status = format!("Authorization cancelled for {tool_id}: {error}");
        self.logs.push(timestamp_log(format!(
            "install: {tool_id} authorization failed: {error}"
        )));
        self.trim_logs();
    }

    pub fn record_output(&mut self, tool_id: &str, chunk: OutputChunk) {
        let stream = match chunk.stream {
            OutputStream::Stdout => "output",
            OutputStream::Stderr => "progress",
        };
        for line in chunk.text.lines().filter(|line| !line.trim().is_empty()) {
            self.logs.push(timestamp_log(format!(
                "{tool_id} [{stream}]: {}",
                line.trim_end()
            )));
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
                launch_preferences: self.launch_preferences.clone(),
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
        self.status = match self.managed_sessions.len() {
            0 => "No background apps running".to_string(),
            count => format!("{count} apps running in background"),
        };
    }

    pub fn apply_installed_tools(&mut self, tool_ids: BTreeSet<String>) {
        self.installed_tools = tool_ids;
        self.installed_scan_complete = true;
    }

    pub fn apply_update_probe(
        &mut self,
        tool_id: &str,
        installed: Result<String, String>,
        verified: String,
    ) {
        let state = match installed {
            Ok(installed) if installed == verified => ToolUpdateState::Current {
                installed,
                verified,
            },
            Ok(installed) => ToolUpdateState::Drift {
                installed,
                verified,
            },
            Err(error) => ToolUpdateState::Error(error),
        };
        self.tool_updates.insert(tool_id.to_string(), state);
    }

    pub fn mark_uninstall_started(&mut self, tool_id: &str) {
        self.uninstalling_tools.insert(tool_id.to_string());
        self.status = format!("Uninstalling {tool_id}");
        self.logs
            .push(timestamp_log(format!("uninstall: started {tool_id}")));
        self.trim_logs();
    }

    pub fn apply_uninstall_result(
        &mut self,
        tool_id: &str,
        success: bool,
        error: &str,
        reinstall: bool,
    ) {
        self.uninstalling_tools.remove(tool_id);
        if success {
            self.installed_tools.remove(tool_id);
            self.queue.retain(|job| job.item.tool_id != tool_id);
            self.install_index = self.install_index.min(self.queue.len().saturating_sub(1));
            self.logs
                .push(timestamp_log(format!("uninstall: completed {tool_id}")));
            if reinstall {
                self.queue_tool_ids(vec![tool_id.to_string()]);
                self.logs
                    .push(timestamp_log(format!("reinstall: queued {tool_id}")));
                self.request_execute_selected();
                if self.confirmation.is_none() {
                    self.status = format!("Reset {tool_id}; reinstalling now");
                }
            } else {
                self.status = format!("Uninstalled {tool_id}");
            }
        } else {
            self.status = if reinstall {
                format!("Could not reset {tool_id}: {error}")
            } else {
                format!("Uninstall failed for {tool_id}: {error}")
            };
            self.logs.push(timestamp_log(format!(
                "uninstall: failed {tool_id}: {error}"
            )));
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
        self.logs.push(timestamp_log(format!(
            "workspace: {} {}",
            if outcome.created {
                "launched"
            } else {
                "reused"
            },
            outcome.session_name
        )));
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
        self.logs.push(timestamp_log(format!(
            "workspace: {action} failed: {error}"
        )));
        self.trim_logs();
    }

    pub fn apply_workspace_hash(&mut self, workspace_id: &str, hash: &str) {
        self.status = format!("{workspace_id} snapshot {}", &hash[..12.min(hash.len())]);
        self.logs.push(timestamp_log(format!(
            "workspace: snapshot {workspace_id} {hash}"
        )));
        self.trim_logs();
    }

    pub fn apply_codex_event(&mut self, event: CodexEvent) {
        let provider = AiProvider::Codex;
        let event = match event {
            CodexEvent::Ready { account } => {
                AiEvent::ProviderReady(ProviderReadiness { provider, account })
            }
            CodexEvent::ThreadStarted(id) => AiEvent::ThreadStarted { provider, id },
            CodexEvent::TurnStarted(id) => AiEvent::TurnStarted { provider, id },
            CodexEvent::Delta(text) => AiEvent::Delta { provider, text },
            CodexEvent::Message(text) => AiEvent::Message { provider, text },
            CodexEvent::ActionProposed { kind, target } => AiEvent::ActionProposed {
                provider,
                kind,
                target,
            },
            CodexEvent::Usage(usage) => AiEvent::Usage { provider, usage },
            CodexEvent::TurnCompleted(status) => AiEvent::TurnCompleted { provider, status },
            CodexEvent::ApprovalDenied(method) => AiEvent::Diagnostic {
                provider,
                message: format!("denied app-server request {method}"),
            },
            CodexEvent::Diagnostic(message) => AiEvent::Diagnostic { provider, message },
            CodexEvent::Error(message) => AiEvent::Error { provider, message },
        };
        self.apply_ai_event(event);
    }

    pub fn apply_ai_event(&mut self, event: AiEvent) {
        match event {
            AiEvent::ProviderReady(readiness) => {
                self.ai_ready_providers
                    .insert(readiness.provider, readiness.account.clone());
                if self.ai_ready_providers.len() == 1
                    || !self.ai_ready_providers.contains_key(&self.ai_provider)
                {
                    self.ai_provider = readiness.provider;
                }
                self.refresh_ai_identity();
            }
            AiEvent::ProviderUnavailable { provider, reason } => {
                self.ai_ready_providers.remove(&provider);
                self.ai_status = format!("{} unavailable: {reason}", provider.label());
                self.refresh_ai_identity();
            }
            AiEvent::ThreadStarted { provider, id } if provider == self.ai_provider => {
                self.ai_status = format!("{} thread {}", provider.label(), short_id(&id));
            }
            AiEvent::TurnStarted { provider, id } if provider == self.ai_provider => {
                self.ai_streaming.clear();
                self.ai_status = format!("{} working {}", provider.label(), short_id(&id));
            }
            AiEvent::Delta { provider, text } if provider == self.ai_provider => {
                self.ai_streaming.push_str(&text);
            }
            AiEvent::Message { provider, text } => {
                self.ai_streaming.clear();
                self.ai_messages.push(AiMessage {
                    role: provider.label().to_string(),
                    text,
                });
                if self.ai_messages.len() > 50 {
                    self.ai_messages.remove(0);
                }
            }
            AiEvent::ActionProposed {
                provider,
                kind,
                target,
            } => {
                let target_exists = self.catalog.tools.iter().any(|tool| tool.id == target);
                let supported = matches!(
                    kind.as_str(),
                    "catalog_search" | "install_plan" | "verified_update" | "launch_app"
                );
                if provider == self.ai_provider && target_exists && supported {
                    self.pending_ai_action = Some(AiAction {
                        kind: kind.clone(),
                        target: target.clone(),
                    });
                    self.ai_status = format!("Proposed {kind} {target} · press A to approve");
                } else {
                    self.ai_status = format!("Rejected unsupported AI action {kind}:{target}");
                }
            }
            AiEvent::Usage { provider, usage } if provider == self.ai_provider => {
                self.ai_usage = usage;
            }
            AiEvent::TurnCompleted { provider, status } if provider == self.ai_provider => {
                if self.pending_ai_action.is_none() {
                    self.ai_status = format!("{} turn {status}", provider.label());
                }
            }
            AiEvent::Diagnostic { provider, message } => {
                self.logs
                    .push(timestamp_log(format!("{}: {message}", provider.label())));
            }
            AiEvent::Error { provider, message } => {
                self.ai_status = format!("{} error: {message}", provider.label());
                self.logs
                    .push(timestamp_log(format!("{}: {message}", provider.label())));
            }
            _ => {}
        }
        self.trim_logs();
    }

    pub fn add_ai_provider(&mut self, readiness: ProviderReadiness) {
        self.apply_ai_event(AiEvent::ProviderReady(readiness));
    }

    pub fn ai_ready(&self) -> bool {
        self.ai_ready_providers.contains_key(&self.ai_provider)
    }

    fn refresh_ai_identity(&mut self) {
        if let Some(account) = self.ai_ready_providers.get(&self.ai_provider) {
            self.ai_account = account.clone();
            self.ai_status = format!("{} ready", self.ai_provider.label());
        } else if let Some((&provider, account)) = self.ai_ready_providers.iter().next() {
            self.ai_provider = provider;
            self.ai_account = account.clone();
            self.ai_status = format!("{} ready", provider.label());
        } else {
            self.ai_account = "not connected".to_string();
            self.ai_status = "AI unavailable · connect a provider in Settings".to_string();
        }
    }

    fn handle_search_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Down | KeyCode::Right | KeyCode::Enter if self.screen == Screen::Home => {
                self.search_mode = false;
                self.home_focus = HomeFocus::AppList;
                self.home_app_index = 0;
                self.status = format!("Showing search results for {}", self.search_query);
            }
            KeyCode::Esc | KeyCode::Enter => self.search_mode = false,
            KeyCode::Backspace => {
                self.search_query.pop();
                self.catalog_index = 0;
                self.home_app_index = 0;
            }
            KeyCode::Char(ch) => {
                self.search_query.push(ch);
                self.catalog_index = 0;
                self.home_app_index = 0;
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
                if !prompt.is_empty() && self.ai_ready() {
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
                let target = confirmation.action.target;
                match confirmation.action.kind.as_str() {
                    "catalog_search" => {
                        self.screen = Screen::Home;
                        self.home_filter_index = HomeFilter::ALL
                            .iter()
                            .position(|filter| *filter == HomeFilter::AllApps)
                            .unwrap_or_default();
                        self.search_query = target.clone();
                        self.home_app_index = 0;
                        self.home_focus = HomeFocus::AppList;
                        self.status = format!("AI searched HOME for {target}");
                    }
                    "install_plan" => {
                        self.queue_tool_ids(vec![target.clone()]);
                        self.screen = Screen::Install;
                    }
                    "verified_update" => self.queue_verified_update(&target),
                    "launch_app" => self.request_tool_configuration(&target),
                    _ => self.ai_status = "Unsupported AI action".to_string(),
                }
                self.logs.push(timestamp_log(format!(
                    "ai: approved {} {target}",
                    confirmation.action.kind
                )));
            }
            _ => {}
        }
    }

    fn handle_screen_key(&mut self, code: KeyCode) {
        match self.screen {
            Screen::Home => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_home_selection(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_home_selection(-1),
                KeyCode::Left | KeyCode::Char('h') => {
                    self.home_focus = if self.home_focus == HomeFocus::Assistant {
                        HomeFocus::AppList
                    } else {
                        HomeFocus::Views
                    };
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    if self.home_focus == HomeFocus::AppList {
                        self.home_focus = HomeFocus::Assistant;
                    } else if !self.home_tools().is_empty() {
                        self.home_focus = HomeFocus::AppList;
                    }
                }
                KeyCode::Enter if self.home_focus == HomeFocus::Views => {
                    if self.home_tools().is_empty() {
                        self.status = format!("No apps in {}", self.selected_home_filter().label());
                    } else {
                        self.home_focus = HomeFocus::AppList;
                        self.status = format!("Browsing {}", self.selected_home_filter().label());
                    }
                }
                KeyCode::Enter if self.home_focus == HomeFocus::Assistant => self.focus_ai(),
                KeyCode::Enter => self.request_selected_tool_launch(),
                KeyCode::Char('/') => {
                    self.begin_home_search();
                }
                KeyCode::Char('I') => self.queue_selected_tool(),
                KeyCode::Char('U') => self.request_selected_uninstall(),
                KeyCode::Char('R') => self.request_selected_reinstall(),
                KeyCode::Char('u') => self.request_selected_verified_update(),
                KeyCode::Char('f') => self.toggle_favorite(),
                KeyCode::Char('c') => self.open_all_catalog(),
                KeyCode::Char('i') => self.screen = Screen::Install,
                KeyCode::Char('a') => self.focus_ai(),
                KeyCode::Char('[') => self.cycle_ai_provider(-1),
                KeyCode::Char(']') => self.cycle_ai_provider(1),
                KeyCode::Char('x') if self.home_focus == HomeFocus::Assistant => {
                    self.effects.push_back(AppEffect::CodexInterrupt)
                }
                KeyCode::Char('A') => self.begin_ai_action_confirmation(),
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
                KeyCode::Char('R') => self.request_selected_reinstall(),
                KeyCode::Char('u') => self.request_selected_verified_update(),
                KeyCode::Char('f') => self.toggle_favorite(),
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
            Screen::Agents => {
                self.screen = Screen::Home;
                self.focus_ai();
            }
            Screen::Logs => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_activity(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_activity(-1),
                KeyCode::PageDown => self.move_activity(10),
                KeyCode::PageUp => self.move_activity(-10),
                KeyCode::Home => self.activity_scroll = 0,
                KeyCode::End => {
                    self.activity_scroll = self.logs.len().saturating_sub(1);
                }
                KeyCode::Char('c') => {
                    self.logs.clear();
                    self.activity_scroll = 0;
                    self.status = "Activity log cleared".to_string();
                }
                _ => {}
            },
            Screen::Settings => match code {
                KeyCode::Down | KeyCode::Char('j') => self.move_setting(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_setting(-1),
                KeyCode::Left | KeyCode::Char('h') => self.adjust_setting(-1),
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => self.adjust_setting(1),
                KeyCode::Enter if self.settings_index == 3 => self.reset_saved_preferences(),
                _ => {}
            },
            Screen::Help => {}
        }
    }

    fn handle_app_view_key(&mut self, key: KeyEvent) {
        if self.link_picker.is_some() {
            self.handle_link_picker_key(key.code);
            return;
        }
        match key.code {
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => self.move_app_view(-1),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => self.move_app_view(1),
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
        let Some(pane_id) = self
            .app_view
            .as_ref()
            .and_then(|view| view.apps.get(view.selected))
            .map(|app| app.pane_id.clone())
        else {
            return;
        };
        self.effects.push_back(AppEffect::ReadAppLinks {
            pane_id,
            action: if open {
                LinkAction::Open
            } else {
                LinkAction::Copy
            },
        });
    }

    pub fn apply_app_links(&mut self, action: LinkAction, content: &str) {
        let mut urls = extract_urls(content);
        urls.reverse();
        let mut seen = BTreeSet::new();
        urls.retain(|url| seen.insert(url.clone()));
        if urls.is_empty() {
            self.status = "No HTTP(S) link found in the current app".to_string();
            return;
        }
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
            Screen::Home => self.move_home_selection(delta),
            Screen::Catalog => self.move_catalog(delta),
            Screen::Install => self.move_install(delta),
            Screen::Workspace => self.move_workspace(delta),
            Screen::AppView => self.move_app_view(delta),
            Screen::Agents => self.move_agent(delta),
            Screen::Settings => self.move_setting(delta),
            Screen::Logs => self.move_activity(delta),
            Screen::Help => {}
        }
    }

    fn select_list_row(&mut self, column: u16, row: u16) {
        let Some(index) = row.checked_sub(4).map(usize::from) else {
            return;
        };
        if self.screen == Screen::Home {
            self.home_focus = if column < 24 {
                HomeFocus::Views
            } else {
                HomeFocus::AppList
            };
        }
        match self.screen {
            Screen::Home if self.home_focus == HomeFocus::Views => {
                let filter_index = if (7..=9).contains(&row) {
                    Some(usize::from(row - 7))
                } else if row >= 12 {
                    Some(3 + usize::from(row - 12))
                } else {
                    None
                };
                if let Some(filter_index) =
                    filter_index.filter(|index| *index < HomeFilter::ALL.len())
                {
                    self.home_filter_index = filter_index;
                    self.home_app_index = 0;
                }
            }
            Screen::Home if index < self.home_tools().len() => self.home_app_index = index,
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
            | Screen::Settings
            | Screen::Help => {}
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

    fn move_home_selection(&mut self, delta: isize) {
        match self.home_focus {
            HomeFocus::Views => {
                if delta < 0 && self.home_filter_index == 0 {
                    self.begin_home_search();
                    return;
                }
                self.home_filter_index =
                    move_index(self.home_filter_index, HomeFilter::ALL.len(), delta);
                self.home_app_index = 0;
            }
            HomeFocus::AppList => {
                self.home_app_index =
                    move_index(self.home_app_index, self.home_tools().len(), delta);
            }
            HomeFocus::Assistant => {}
        }
    }

    fn move_home_focus(&mut self, delta: isize) {
        let current = match self.home_focus {
            HomeFocus::Views => 0,
            HomeFocus::AppList => 1,
            HomeFocus::Assistant => 2,
        };
        self.home_focus = match move_index(current, 3, delta) {
            0 => HomeFocus::Views,
            1 => HomeFocus::AppList,
            _ => HomeFocus::Assistant,
        };
    }

    fn focus_ai(&mut self) {
        self.screen = Screen::Home;
        self.home_focus = HomeFocus::Assistant;
        if self.ai_ready() {
            self.ai_input_mode = true;
            self.ai_status = format!("Compose with {}", self.ai_provider.label());
        } else {
            self.ai_input_mode = false;
            self.ai_status =
                "AI unavailable · connect Codex, Claude, or Gemini in Settings".to_string();
        }
    }

    fn cycle_ai_provider(&mut self, delta: isize) {
        let providers = self.ai_ready_providers.keys().copied().collect::<Vec<_>>();
        if providers.is_empty() {
            self.refresh_ai_identity();
            return;
        }
        let current = providers
            .iter()
            .position(|provider| *provider == self.ai_provider)
            .unwrap_or_default();
        self.ai_provider = providers[move_index(current, providers.len(), delta)];
        self.refresh_ai_identity();
    }

    fn begin_home_search(&mut self) {
        self.home_filter_index = HomeFilter::ALL
            .iter()
            .position(|filter| *filter == HomeFilter::AllApps)
            .unwrap_or_default();
        self.home_app_index = 0;
        self.search_mode = true;
        self.status = "Search all HOME apps".to_string();
    }

    fn move_setting(&mut self, delta: isize) {
        self.settings_index = move_index(self.settings_index, 4, delta);
    }

    fn queue_selected_tool(&mut self) {
        let Some(tool) = self.selected_action_tool() else {
            self.status = "No app selected".to_string();
            return;
        };
        self.queue_tool_ids(vec![tool.id.clone()]);
    }

    fn request_selected_verified_update(&mut self) {
        let Some(tool_id) = self.selected_action_tool().map(|tool| tool.id.clone()) else {
            self.status = "No app selected".to_string();
            return;
        };
        self.queue_verified_update(&tool_id);
    }

    fn queue_verified_update(&mut self, tool_id: &str) {
        let platform = if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Linux
        };
        let Some(tool) = self.catalog.tools.iter().find(|tool| tool.id == tool_id) else {
            self.status = format!("Unknown app {tool_id}");
            return;
        };
        let Some(installer) = tool
            .installers
            .iter()
            .find(|installer| installer.platform == platform)
        else {
            self.status = format!("No installer for {tool_id} on this platform");
            return;
        };
        let Ok(task) = build_verified_update_task(tool, installer, &InstallPolicy::default())
        else {
            self.status = format!("No T4E-verified update for {tool_id}");
            return;
        };
        if self
            .queue
            .iter()
            .any(|job| job.item.tool_id == tool_id && job.item.state == QueueState::Installing)
        {
            self.status = format!("{tool_id} is already installing");
            return;
        }
        self.queue.retain(|job| job.item.tool_id != tool_id);
        let channel = format!(
            "verified {}",
            task.expected_version.as_deref().unwrap_or("version")
        );
        self.queue.push(InstallJob::new(task, channel));
        self.install_index = self.queue.len().saturating_sub(1);
        self.screen = Screen::Install;
        self.status = format!("Queued T4E-verified update for {tool_id}");
        self.request_execute_selected();
    }

    fn request_selected_uninstall(&mut self) {
        let Some(tool) = self.selected_action_tool() else {
            return;
        };
        let tool_id = tool.id.clone();
        if !self.installed_tools.contains(&tool_id) {
            self.status = format!("{tool_id} is not installed");
            return;
        }
        let Some(request) = uninstall_request_for_tool(tool, false) else {
            self.status = format!("No uninstall method for {tool_id}");
            return;
        };
        self.uninstall_confirmation = Some(request);
        self.status = "Confirm app uninstall".to_string();
    }

    fn request_selected_reinstall(&mut self) {
        let Some(tool) = self.selected_action_tool() else {
            return;
        };
        let tool_id = tool.id.clone();
        let Some(request) = uninstall_request_for_tool(tool, true) else {
            self.status = format!("Reset and reinstall is unavailable for {tool_id}");
            return;
        };
        self.uninstall_confirmation = Some(request);
        self.status = format!("Confirm reset and reinstall of {tool_id}");
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
        let Some(tool_id) = self.selected_action_tool().map(|tool| tool.id.clone()) else {
            self.status = "No app selected".to_string();
            return;
        };
        self.request_tool_configuration(&tool_id);
    }

    fn request_tool_configuration(&mut self, requested_tool_id: &str) {
        let Some(tool) = self
            .catalog
            .tools
            .iter()
            .find(|tool| tool.id == requested_tool_id)
        else {
            self.status = "App is no longer available".to_string();
            return;
        };
        let tool_id = tool.id.clone();
        let tool_name = tool.name.clone();
        let command = tool.run_command_for_current_platform().to_string();
        let keep_open = tool.run.keep_open;
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
        if self.installed_scan_complete && !self.installed_tools.contains(&tool_id) {
            self.install_then_configure(tool_id);
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
            self.request_tool_launch(
                tool_name,
                ToolLaunchRequest {
                    tool_id,
                    command,
                    keep_open,
                    output_filter: None,
                },
            );
            return;
        }
        self.open_launch_options(tool_id, tool_name, command, None, options);
    }

    fn open_launch_options(
        &mut self,
        tool_id: String,
        tool_name: String,
        command: String,
        argument: Option<String>,
        options: Vec<RunOption>,
    ) {
        let saved_preferences = self.launch_preferences.get(&tool_id);
        let selections = options
            .iter()
            .map(|option| {
                let saved = saved_preferences.and_then(|options| options.get(&option.id));
                LaunchOptionSelection {
                    enabled: saved.map_or(option.default_enabled, |saved| saved.enabled),
                    value_index: saved
                        .and_then(|saved| saved.value.as_ref())
                        .and_then(|value| {
                            option
                                .values
                                .iter()
                                .position(|candidate| candidate == value)
                        })
                        .or_else(|| {
                            option.default_value.as_ref().and_then(|default| {
                                option.values.iter().position(|value| value == default)
                            })
                        })
                        .unwrap_or_default(),
                }
            })
            .collect();
        self.launch_options = Some(LaunchOptionsState {
            tool_id,
            tool_name,
            command,
            argument,
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
                let tool_id = tool.id.clone();
                let tool_name = tool.name.clone();
                let keep_open = tool.run.keep_open;
                let options = tool.run_options.clone();
                let base_command = tool.run_command_for_current_platform().to_string();
                let command = format!("{base_command} {}", shell_quote(value));
                if options.is_empty() {
                    self.effects
                        .push_back(AppEffect::LaunchTool(ToolLaunchRequest {
                            tool_id,
                            command,
                            keep_open,
                            output_filter: None,
                        }));
                    self.status = format!("Opening {tool_name}");
                } else {
                    self.open_launch_options(
                        tool_id,
                        tool_name,
                        base_command,
                        Some(shell_quote(value)),
                        options,
                    );
                }
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
        let mut command = options.command;
        let mut output_filter = None;
        let mut preferences = BTreeMap::new();
        for (option, selection) in tool.run_options.iter().zip(&options.selections) {
            preferences.insert(
                option.id.clone(),
                LaunchOptionPreference {
                    enabled: selection.enabled,
                    value: option.values.get(selection.value_index).cloned(),
                },
            );
            if !selection.enabled {
                continue;
            }
            if !option.flag.is_empty() {
                command.push(' ');
                command.push_str(&option.flag);
            }
            if let Some(value) = option.values.get(selection.value_index) {
                command.push(' ');
                command.push_str(value);
            }
            if option.output_filter.is_some() {
                output_filter = option.output_filter;
            }
        }
        if let Some(argument) = options.argument {
            command.push(' ');
            command.push_str(&argument);
        }
        let tool_id = tool.id.clone();
        let tool_name = tool.name.clone();
        let keep_open = tool.run.keep_open;
        self.launch_preferences.insert(tool_id.clone(), preferences);
        self.request_tool_launch(
            tool_name,
            ToolLaunchRequest {
                tool_id,
                command,
                keep_open,
                output_filter,
            },
        );
        if self.launch_approval.is_none() {
            self.status.push_str("; launch options saved");
        }
    }

    fn request_tool_launch(&mut self, tool_name: String, request: ToolLaunchRequest) {
        let needs_camera_approval = self
            .catalog
            .tools
            .iter()
            .find(|tool| tool.id == request.tool_id)
            .is_some_and(|tool| {
                tool.capabilities
                    .contains(&crate::catalog::models::Capability::CameraCapture)
            })
            && !self.approved_camera_tools.contains(&request.tool_id);
        if needs_camera_approval {
            self.launch_approval = Some(LaunchApproval { tool_name, request });
            self.status = "Camera access approval required".to_string();
        } else {
            self.status = format!("Opening {tool_name}");
            self.effects.push_back(AppEffect::LaunchTool(request));
        }
    }

    fn handle_launch_approval_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.launch_approval = None;
                self.status = "Camera launch cancelled".to_string();
            }
            KeyCode::Enter => {
                let Some(approval) = self.launch_approval.take() else {
                    return;
                };
                self.approved_camera_tools
                    .insert(approval.request.tool_id.clone());
                self.status = format!("Opening {}; camera access approved", approval.tool_name);
                self.effects
                    .push_back(AppEffect::LaunchTool(approval.request));
            }
            _ => {}
        }
    }

    pub fn install_then_launch(&mut self, request: ToolLaunchRequest) {
        let tool_id = request.tool_id.clone();
        self.install_tool_then_launch(tool_id, request);
    }

    pub fn install_tool_then_launch(&mut self, tool_id: String, request: ToolLaunchRequest) {
        self.pending_tool_launch = Some(request);
        self.pending_tool_configuration = None;
        self.pending_tool_install = Some(tool_id.clone());
        self.begin_pending_install(tool_id);
    }

    fn install_then_configure(&mut self, tool_id: String) {
        self.pending_tool_launch = None;
        self.pending_tool_configuration = Some(tool_id.clone());
        self.pending_tool_install = Some(tool_id.clone());
        self.begin_pending_install(tool_id);
    }

    fn begin_pending_install(&mut self, tool_id: String) {
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
                    self.status = format!("Installing {tool_id}; options will open when ready");
                    return;
                }
                QueueState::Queued | QueueState::Idle => {}
            }
        } else {
            self.queue_tool_ids(vec![tool_id.clone()]);
        }

        self.status = format!("{tool_id} is not installed; installing before setup");
        self.request_execute_selected();
    }

    fn return_to_main(&mut self) {
        if self.screen == Screen::Catalog {
            self.search_query.clear();
        }
        self.screen = Screen::Home;
    }

    fn open_all_catalog(&mut self) {
        self.search_query.clear();
        self.search_mode = false;
        self.catalog_index = 0;
        self.screen = Screen::Catalog;
        self.status = "Showing all catalog tools".to_string();
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
            self.logs
                .push(timestamp_log(format!("queue: added {tool_id}")));
            added += 1;
        }
        self.install_index = self.queue.len().saturating_sub(1);
        self.status = format!("Queued {added} tools; skipped {skipped}");
        self.trim_logs();
    }

    fn toggle_favorite(&mut self) {
        let Some(tool_id) = self.selected_action_tool().map(|tool| tool.id.clone()) else {
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
            self.logs
                .push(timestamp_log(format!("queue: retry {}", tool_id)));
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
        let current_task = if job.task.is_verified_update() {
            build_current_verified_task(&self.catalog, &job.item.tool_id)
        } else {
            self.build_current_task(&job.item.tool_id)
        };
        let Some(current_task) = current_task else {
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
            let expected = job.task.expected_version.as_ref().map_or_else(
                || format!("INSTALL {tool_id}"),
                |version| format!("UPDATE {tool_id} {version}"),
            );
            self.confirmation = Some(InstallConfirmation {
                command: job.task.command.clone(),
                expected,
                input: String::new(),
                typed,
                tool_id,
            });
            self.status = if typed {
                "Type the confirmation phrase exactly".to_string()
            } else {
                "Press Enter to approve this installation".to_string()
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
        self.logs
            .push(timestamp_log(format!("queue: removed {tool_id}")));
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
                self.pending_tool_install = None;
                self.pending_tool_configuration = None;
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
        if self.settings_index == 3 {
            return;
        }
        match self.settings_index {
            0 => {
                self.mouse_enabled = !self.mouse_enabled;
                self.settings.mouse_enabled = self.mouse_enabled;
                self.mouse_selection = None;
                self.effects
                    .push_back(AppEffect::SetMouseCapture(self.mouse_enabled));
            }
            1 => {
                let next = self.settings.max_install_attempts as isize + delta;
                self.settings.max_install_attempts = next.clamp(1, 5) as u32;
            }
            2 => {
                self.settings.confirm_all_installs = !self.settings.confirm_all_installs;
            }
            _ => {}
        }
        self.status = "Settings updated".to_string();
    }

    fn reset_saved_preferences(&mut self) {
        self.settings = UserSettings::default();
        self.mouse_enabled = self.settings.mouse_enabled;
        self.mouse_selection = None;
        self.effects
            .push_back(AppEffect::SetMouseCapture(self.mouse_enabled));
        self.launch_preferences.clear();
        self.status = "Runtime settings and saved app options reset".to_string();
        self.logs.push(timestamp_log(
            "settings: reset runtime settings and app options",
        ));
        self.trim_logs();
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

    fn move_activity(&mut self, delta: isize) {
        let max = self.logs.len().saturating_sub(1);
        self.activity_scroll = if delta.is_negative() {
            self.activity_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.activity_scroll.saturating_add(delta as usize).min(max)
        };
        self.status = if self.logs.is_empty() {
            "Activity log is empty".to_string()
        } else {
            format!(
                "Activity line {} of {}",
                self.activity_scroll + 1,
                self.logs.len()
            )
        };
    }

    fn trim_logs(&mut self) {
        const MAX_LOG_LINES: usize = 500;
        if self.logs.len() > MAX_LOG_LINES {
            self.logs.drain(0..self.logs.len() - MAX_LOG_LINES);
        }
        self.activity_scroll = self.activity_scroll.min(self.logs.len().saturating_sub(1));
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
    saved.logs.extend(refreshed.into_iter().map(|tool_id| {
        timestamp_log(format!("queue: refreshed stale install plan for {tool_id}"))
    }));
}

fn timestamp_log(message: impl AsRef<str>) -> String {
    format!(
        "[{}] {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z"),
        message.as_ref()
    )
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

fn build_current_verified_task(
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
    build_verified_update_task(tool, installer, &InstallPolicy::default()).ok()
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
        || saved.expected_version != current.expected_version
        || saved.version_probe != current.version_probe
}

fn uninstall_request_for_tool(tool: &Tool, reinstall: bool) -> Option<UninstallRequest> {
    let platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    };
    let installer = tool
        .installers
        .iter()
        .find(|installer| installer.platform == platform)?;
    let packages = if installer.method == InstallMethod::Cargo {
        installer.package_hints.join(" ")
    } else {
        installer.package_hints.first()?.clone()
    };
    let command = uninstall_command(&installer.method, &packages, reinstall)?;
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
        .unwrap_or_else(|| tool.id.clone());
    Some(UninstallRequest {
        tool_id: tool.id.clone(),
        command,
        check_command,
        method: installer.method.clone(),
        reinstall,
    })
}

fn uninstall_command(
    method: &InstallMethod,
    package: &str,
    tolerate_missing: bool,
) -> Option<String> {
    let command = match method {
        InstallMethod::Brew if tolerate_missing => format!(
            "if brew list --formula {package} >/dev/null 2>&1; then brew uninstall {package}; fi"
        ),
        InstallMethod::Brew => format!("brew uninstall {package}"),
        InstallMethod::BrewCask if tolerate_missing => format!(
            "if brew list --cask {package} >/dev/null 2>&1; then brew uninstall --cask {package}; fi"
        ),
        InstallMethod::BrewCask => format!("brew uninstall --cask {package}"),
        InstallMethod::Apt if tolerate_missing => format!(
            "if dpkg-query -W -f='${{db:Status-Abbrev}}' {package} 2>/dev/null | grep -q '^ii'; then sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 remove -y {package}; fi"
        ),
        InstallMethod::Apt => {
            format!(
                "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 remove -y {package}"
            )
        }
        InstallMethod::Dnf if tolerate_missing => format!(
            "if rpm -q {package} >/dev/null 2>&1; then sudo -n dnf remove -y {package}; fi"
        ),
        InstallMethod::Dnf => format!("sudo -n dnf remove -y {package}"),
        InstallMethod::Pacman if tolerate_missing => format!(
            "if pacman -Q {package} >/dev/null 2>&1; then sudo -n pacman -Rns --noconfirm {package}; fi"
        ),
        InstallMethod::Pacman => format!("sudo -n pacman -Rns --noconfirm {package}"),
        InstallMethod::Snap | InstallMethod::SnapClassic if tolerate_missing => format!(
            "if snap list {package} >/dev/null 2>&1; then sudo -n snap remove {package}; fi"
        ),
        InstallMethod::Snap | InstallMethod::SnapClassic => {
            format!("sudo -n snap remove {package}")
        }
        InstallMethod::Pipx if tolerate_missing => format!(
            "if pipx list --short 2>/dev/null | cut -d' ' -f1 | grep -Fxq {package}; then pipx uninstall {package}; fi"
        ),
        InstallMethod::Pipx => format!("pipx uninstall {package}"),
        InstallMethod::NpmGlobal if tolerate_missing => format!(
            "if npm list --global --depth=0 {package} >/dev/null 2>&1; then npm uninstall --global {package}; fi"
        ),
        InstallMethod::NpmGlobal => format!("npm uninstall --global {package}"),
        InstallMethod::Cargo => format!(
            "for package in {package}; do cargo uninstall \"$package\" || ! command -v \"$package\" >/dev/null 2>&1 || exit 1; done"
        ),
        InstallMethod::LazyVim => "rm -f \"$HOME/.local/bin/t4e-lazyvim\" && rm -rf \"${XDG_CONFIG_HOME:-$HOME/.config}/t4e-lazyvim\" \"${XDG_DATA_HOME:-$HOME/.local/share}/t4e-lazyvim\" \"${XDG_STATE_HOME:-$HOME/.local/state}/t4e-lazyvim\" \"${XDG_CACHE_HOME:-$HOME/.cache}/t4e-lazyvim\"".to_string(),
        InstallMethod::Tplay => "rm -f \"$HOME/.local/bin/t4e-tplay\" && rm -rf \"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/tplay\" && (cargo uninstall tplay || ! command -v tplay >/dev/null 2>&1)".to_string(),
        InstallMethod::YoutubeTui => "rm -f \"$HOME/.local/bin/t4e-youtube-tui\" \"$HOME/.local/bin/t4e-youtube-tui-v2\" && rm -rf \"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/youtube-tui\" && (cargo uninstall youtube-tui || ! command -v youtube-tui >/dev/null 2>&1)".to_string(),
        InstallMethod::Yewtube => "rm -f \"$HOME/.local/bin/t4e-yewtube\" && if pipx list --short 2>/dev/null | cut -d' ' -f1 | grep -Fxq yewtube; then pipx uninstall yewtube; fi".to_string(),
        InstallMethod::AsciiCamera => {
            "rm -f \"$HOME/.local/bin/t4e-ascii-camera\" \"$HOME/.local/bin/t4e-ascii-camera-v2\""
                .to_string()
        }
        InstallMethod::Newsboat => "rm -f \"$HOME/.local/bin/t4e-newsboat\" && rm -rf \"$HOME/snap/newsboat/common/t4e\" \"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/newsboat\" && if command -v snap >/dev/null 2>&1; then if snap list newsboat >/dev/null 2>&1; then sudo -n snap remove newsboat; fi; elif command -v brew >/dev/null 2>&1 && brew list --formula newsboat >/dev/null 2>&1; then brew uninstall newsboat; fi".to_string(),
        InstallMethod::Fastfetch if tolerate_missing => "if dpkg-query -W -f='${db:Status-Abbrev}' fastfetch 2>/dev/null | grep -q '^ii'; then sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 remove -y fastfetch; fi".to_string(),
        InstallMethod::Fastfetch => "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 remove -y fastfetch".to_string(),
        InstallMethod::Builtin | InstallMethod::Go | InstallMethod::Script | InstallMethod::Other => {
            return None;
        }
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
