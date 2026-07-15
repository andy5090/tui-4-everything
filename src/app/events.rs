#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Catalog,
    Install,
    Workspace,
    Agents,
    Logs,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAction {
    GoCatalog,
    GoInstall,
    GoWorkspace,
    GoAgents,
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
        (Screen::Home, 'w') => EventAction::GoWorkspace,
        (Screen::Home, 'a') => EventAction::GoAgents,
        (Screen::Home, 'l') => EventAction::GoLogs,
        (Screen::Home, 's') => EventAction::GoSettings,
        (Screen::Catalog, 'j') => EventAction::MoveDown,
        (Screen::Catalog, 'k') => EventAction::MoveUp,
        (Screen::Catalog, '\n') => EventAction::OpenDetail,
        (Screen::Catalog, 'I') => EventAction::QueueInstall,
        (Screen::Install, 'r') => EventAction::Retry,
        (Screen::Workspace, '\n') => EventAction::LaunchWorkspace,
        (Screen::Workspace, 'h') => EventAction::ShowHash,
        (Screen::Agents, '\n') => EventAction::OpenDetail,
        (Screen::Logs, '/') => EventAction::FilterLogs,
        (_, 'q') => EventAction::QuitOrBack,
        _ => EventAction::Noop,
    }
}
