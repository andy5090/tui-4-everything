#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Catalog,
    Install,
    Workspace,
    AppView,
    Agents,
    Logs,
    Settings,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAction {
    GoCatalog,
    GoInstall,
    GoAgents,
    FocusAi,
    GoLogs,
    GoSettings,
    MoveDown,
    MoveUp,
    Select,
    QueueInstall,
    Retry,
    ShowHash,
    OpenDetail,
    LaunchWorkspace,
    FilterLogs,
    QuitOrBack,
    Noop,
}

pub fn map_key(screen: Screen, key: char) -> EventAction {
    match (screen, key) {
        (Screen::Home, 'c') => EventAction::GoCatalog,
        (Screen::Home, 'i') => EventAction::GoInstall,
        (Screen::Home, 'a') => EventAction::FocusAi,
        (Screen::Home, 'l') => EventAction::GoLogs,
        (Screen::Home, 's') => EventAction::GoSettings,
        (Screen::Catalog, 'j') => EventAction::MoveDown,
        (Screen::Catalog, 'k') => EventAction::MoveUp,
        (Screen::Catalog, '\n') => EventAction::OpenDetail,
        (Screen::Catalog, 'I') => EventAction::QueueInstall,
        (Screen::Install, 'r') => EventAction::Retry,
        (Screen::Workspace, '\n') => EventAction::LaunchWorkspace,
        (Screen::Workspace, 'h') => EventAction::ShowHash,
        (Screen::AppView, _) => EventAction::Noop,
        (Screen::Agents, '\n') => EventAction::OpenDetail,
        (Screen::Logs, '/') => EventAction::FilterLogs,
        (Screen::Help, _) => EventAction::Noop,
        (_, 'q') => EventAction::QuitOrBack,
        _ => EventAction::Noop,
    }
}
