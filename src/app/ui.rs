use ansi_to_tui::IntoText as _;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};

use crate::catalog::models::RiskLevel;
use crate::installer::queue::QueueState;
use crate::mux::workspace::TmuxView;

use super::events::Screen;
use super::state::{AppState, HomeFilter, HomeFocus, LinkAction, NAVIGATION_TAB_LABELS};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const SELECTED: Color = Color::Yellow;

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let area = frame.area();
    if area.width < 60 || area.height < 16 {
        frame.render_widget(
            Paragraph::new("T4E needs a terminal of at least 60x16")
                .alignment(Alignment::Center)
                .block(panel("Terminal too small")),
            area,
        );
        return;
    }

    if app.screen == Screen::AppView {
        render_app_view(frame, app, area);
        if app.link_picker.is_some() {
            render_link_picker(frame, app, area);
        }
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(frame, app, sections[0]);
    match app.screen {
        Screen::Home => render_home(frame, app, sections[1]),
        Screen::Catalog => render_catalog(frame, app, sections[1]),
        Screen::Install => render_install(frame, app, sections[1]),
        Screen::Workspace => render_workspaces(frame, app, sections[1]),
        Screen::AppView => unreachable!("app view renders before the dashboard layout"),
        Screen::Agents => render_agents(frame, app, sections[1]),
        Screen::Logs => render_logs(frame, app, sections[1]),
        Screen::Settings => render_settings(frame, app, sections[1]),
        Screen::Help => render_help(frame, sections[1]),
    }
    render_footer(frame, app, sections[2]);

    if app.launch_argument.is_some() {
        render_launch_argument(frame, app, area);
    } else if app.launch_options.is_some() {
        render_launch_options(frame, app, area);
    } else if app.launch_approval.is_some() {
        render_launch_approval(frame, app, area);
    } else if app.uninstall_confirmation.is_some() {
        render_uninstall_confirmation(frame, app, area);
    } else if app.confirmation.is_some() {
        render_confirmation(frame, app, area);
    } else if app.ai_confirmation.is_some() {
        render_ai_confirmation(frame, app, area);
    }
}

fn render_header(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let section = match app.screen {
        Screen::Home => "HOME".to_string(),
        Screen::Catalog => "App details".to_string(),
        Screen::Install => "Installs".to_string(),
        Screen::Workspace => "Legacy workspace".to_string(),
        Screen::AppView => "Running apps".to_string(),
        Screen::Agents => "AI".to_string(),
        Screen::Logs => "Activity".to_string(),
        Screen::Settings => "Settings".to_string(),
        Screen::Help => "Help".to_string(),
    };
    let titles = NAVIGATION_TAB_LABELS
        .iter()
        .map(|label| Line::from(*label))
        .collect::<Vec<_>>();
    frame.render_widget(
        Tabs::new(titles)
            .block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled(
                            " T4E",
                            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(format!(" · {section} ")),
                    ]))
                    .borders(Borders::ALL),
            )
            .select(app.navigation_tab_index())
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(SELECTED)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .padding("", "")
            .divider(" | "),
        area,
    );
}

fn render_home(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let [library_area, apps_area, information_area] = home_layout(area);
    let library_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(6)])
        .split(library_area);
    let library_filters = HomeFilter::ALL[..3]
        .iter()
        .map(|filter| {
            ListItem::new(format!(
                "{:<14} {:>2}",
                filter.label(),
                app.home_filter_count(*filter)
            ))
        })
        .collect::<Vec<_>>();
    let library_selected = (app.home_filter_index < 3).then_some(app.home_filter_index);
    let mut library_state = ListState::default().with_selected(library_selected);
    frame.render_stateful_widget(
        List::new(library_filters)
            .block(panel("Quick Access"))
            .highlight_style(home_selection_style(
                app.home_focus == HomeFocus::Views && app.home_filter_index < 3,
            ))
            .highlight_symbol("> "),
        library_sections[0],
        &mut library_state,
    );

    let categories = HomeFilter::ALL[3..]
        .iter()
        .map(|filter| {
            ListItem::new(format!(
                "{:<14} {:>2}",
                filter.label(),
                app.home_filter_count(*filter)
            ))
        })
        .collect::<Vec<_>>();
    let category_selected = app.home_filter_index.checked_sub(3);
    let mut category_state = ListState::default().with_selected(category_selected);
    frame.render_stateful_widget(
        List::new(categories)
            .block(panel("Apps"))
            .highlight_style(home_selection_style(
                app.home_focus == HomeFocus::Views && app.home_filter_index >= 3,
            ))
            .highlight_symbol("> "),
        library_sections[1],
        &mut category_state,
    );

    let tools = app.home_tools();
    let app_items = tools
        .iter()
        .map(|tool| {
            let state =
                if app.is_tool_running(&tool.id) {
                    "RUNNING"
                } else if app.installed_tools.contains(&tool.id) {
                    "INSTALLED"
                } else if app.queue.iter().any(|job| {
                    job.item.tool_id == tool.id && job.item.state == QueueState::Installing
                }) {
                    "INSTALLING"
                } else {
                    ""
                };
            ListItem::new(Line::from(vec![
                Span::raw(format!(
                    "{} ",
                    if app.favorites.contains(&tool.id) {
                        "*"
                    } else {
                        " "
                    }
                )),
                Span::raw(format!("{:<19}", tool.name)),
                Span::styled(
                    format!("{:<14}", tool.app_category().label()),
                    Style::default().fg(MUTED),
                ),
                Span::styled(
                    state,
                    if state == "RUNNING" {
                        Style::default().fg(ACCENT)
                    } else {
                        Style::default().fg(Color::Green)
                    },
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let app_selection = (!tools.is_empty()).then_some(app.home_app_index);
    let mut app_state = ListState::default().with_selected(app_selection);
    let app_title = if app.search_query.is_empty() {
        format!("{} ({})", app.selected_home_filter().label(), tools.len())
    } else {
        format!(
            "{} ({}) /{}",
            app.selected_home_filter().label(),
            tools.len(),
            app.search_query
        )
    };
    frame.render_stateful_widget(
        List::new(app_items)
            .block(panel(&app_title))
            .highlight_style(home_selection_style(app.home_focus == HomeFocus::AppList))
            .highlight_symbol("> "),
        apps_area,
        &mut app_state,
    );

    let mut information = if app.system_overview.logo.is_empty() {
        app.system_overview
            .lines
            .iter()
            .map(|line| Line::from(line.as_str()))
            .collect::<Vec<_>>()
    } else {
        let logo_width = app
            .system_overview
            .logo
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or_default();
        let row_count = app
            .system_overview
            .logo
            .len()
            .max(app.system_overview.lines.len());
        (0..row_count)
            .map(|index| {
                let logo = app
                    .system_overview
                    .logo
                    .get(index)
                    .map_or("", String::as_str);
                let mut spans = vec![Span::styled(
                    format!("{logo:<logo_width$}"),
                    Style::default().fg(ACCENT),
                )];
                if let Some(detail) = app.system_overview.lines.get(index) {
                    spans.push(Span::raw(format!("  {detail}")));
                }
                Line::from(spans)
            })
            .collect::<Vec<_>>()
    };
    information.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled("Available: ", Style::default().fg(ACCENT)),
            Span::raw(format!(
                "{} apps · {} tools",
                app.catalog
                    .tools
                    .iter()
                    .filter(|tool| tool.is_launchable_app())
                    .count(),
                app.catalog.tools.len()
            )),
        ]),
        Line::from(vec![
            Span::styled("Installs: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{} queued or completed", app.queue.len())),
        ]),
        Line::from(vec![
            Span::styled("Saved: ", Style::default().fg(Color::Magenta)),
            Span::raw(format!(
                "{} favorites, {} recents",
                app.favorites.len(),
                app.recents.len()
            )),
        ]),
    ]);
    if let Some(tool) = app.selected_home_tool() {
        information.extend([
            Line::from(""),
            Line::styled(
                tool.name.as_str(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Line::from(
                tool.description
                    .as_deref()
                    .unwrap_or("No description available"),
            ),
            Line::from(format!(
                "{} · {}",
                tool.app_category().label(),
                AppState::risk_label(tool.risk_level())
            )),
            Line::from(format!(
                "Keys: {}",
                if tool.key_hints.is_empty() {
                    "See built-in help".to_string()
                } else {
                    tool.key_hints.join(" | ")
                }
            )),
        ]);
    }
    information.extend([
        Line::from(""),
        Line::from("←/→ panel   Enter run   / search   I/U/R install/remove/reset"),
    ]);
    let information_title = format!("Information · {}", app.system_overview.source);
    let information = Paragraph::new(information).block(panel(&information_title));
    frame.render_widget(information, information_area);
}

fn home_layout(area: Rect) -> [Rect; 3] {
    if area.width >= 110 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(24),
                Constraint::Min(40),
                Constraint::Length(48),
            ])
            .split(area);
        return [columns[0], columns[1], columns[2]];
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(30)])
        .split(area);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(5)])
        .split(columns[1]);
    [columns[0], right[0], right[1]]
}

fn home_selection_style(focused: bool) -> Style {
    if focused {
        selection_style()
    } else {
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::REVERSED)
    }
}

fn render_catalog(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = responsive_split(area, 92);
    let tools = app.visible_catalog_tools();
    let items = tools
        .iter()
        .map(|tool| {
            let (install_status, install_style) = catalog_install_status(app, &tool.id);
            ListItem::new(Line::from(vec![
                Span::raw(format!(
                    "{} ",
                    if app.favorites.contains(&tool.id) {
                        "*"
                    } else {
                        " "
                    }
                )),
                Span::raw(format!("{:<20}", tool.name)),
                Span::styled(
                    format!("{:<10}", AppState::risk_label(tool.risk_level())),
                    risk_style(tool.risk_level()),
                ),
                Span::styled(install_status, install_style),
            ]))
        })
        .collect::<Vec<_>>();
    let scope = "all tools";
    let title = if app.search_query.is_empty() {
        format!("Catalog ({}) - {}", tools.len(), scope)
    } else {
        format!(
            "Catalog ({}) - {} /{}",
            tools.len(),
            scope,
            app.search_query
        )
    };
    let mut state = ListState::default().with_selected(Some(app.catalog_index));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel(&title))
            .highlight_style(selection_style())
            .highlight_symbol("> "),
        chunks[0],
        &mut state,
    );

    let detail = app.selected_catalog_tool().map_or_else(
        || Text::from("No matching tool"),
        |tool| {
            let mut lines = vec![
                Line::styled(&tool.name, Style::default().add_modifier(Modifier::BOLD)),
                Line::from(
                    tool.description
                        .as_deref()
                        .unwrap_or("No description available"),
                ),
                Line::from(""),
                Line::from(format!("id: {}", tool.id)),
                Line::from(format!(
                    "risk: {} ({})",
                    AppState::risk_label(tool.risk_level()),
                    risk_explanation(tool.risk_level())
                )),
                Line::from(format!(
                    "capabilities: {}",
                    if tool.capabilities.is_empty() {
                        "NONE".to_string()
                    } else {
                        tool.capabilities
                            .iter()
                            .map(|capability| capability.label())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                )),
                Line::from(format!("run: {}", tool.run_command_for_current_platform())),
                Line::from(format!("launch options: {}", tool.run_options.len())),
                Line::from(format!(
                    "app keys: {}",
                    if tool.key_hints.is_empty() {
                        "See the app's built-in help".to_string()
                    } else {
                        tool.key_hints.join(" | ")
                    }
                )),
                Line::from("T4E controls: Enter run | I install | U uninstall | R reinstall"),
                Line::from(""),
            ];
            if let Some(job) = app.queue.iter().find(|job| job.item.tool_id == tool.id) {
                lines.push(Line::styled(
                    format!("install: {}", catalog_install_status(app, &tool.id).0),
                    catalog_install_status(app, &tool.id).1,
                ));
                lines.push(Line::from(format!("channel: {}", job.item.channel)));
                lines.push(Line::from(format!(
                    "timeout: {} min",
                    job.task
                        .effective_timeout_sec(app.settings.install_timeout_sec)
                        .div_ceil(60)
                )));
                lines.push(Line::from(format!(
                    "attempts: {}/{}",
                    if job.item.state == QueueState::Installing {
                        job.item.attempts.saturating_add(1)
                    } else {
                        job.item.attempts
                    },
                    app.settings.max_install_attempts
                )));
                if let Some(diagnostics) = &job.diagnostics {
                    lines.push(Line::styled(
                        format!("error: {}", diagnostics.stderr_summary),
                        Style::default().fg(Color::Red),
                    ));
                }
                let recent = app
                    .logs
                    .iter()
                    .rev()
                    .filter_map(|line| {
                        let message = activity_message(line);
                        (message.starts_with(&format!("{} [", tool.id))
                            || message.starts_with(&format!("install: {}", tool.id))
                            || message.starts_with(&format!("uninstall: {}", tool.id)))
                        .then_some(message)
                    })
                    .take(4)
                    .collect::<Vec<_>>();
                if !recent.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::styled(
                        match job.item.state {
                            QueueState::Installing => "Live install output",
                            QueueState::Failed => "Last output before failure",
                            _ => "Recent install output",
                        },
                        Style::default().fg(MUTED),
                    ));
                    lines.extend(recent.into_iter().rev().map(Line::from));
                }
            } else {
                let (status, style) = catalog_install_status(app, &tool.id);
                lines.push(Line::styled(format!("install: {status}"), style));
            }
            lines.extend([
                Line::from(""),
                Line::styled(
                    "Enter run  I install  U remove  R reinstall  f favorite  Backspace HOME",
                    Style::default().fg(MUTED),
                ),
            ]);
            Text::from(lines)
        },
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel("Tool details"))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn catalog_install_status(app: &AppState, tool_id: &str) -> (String, Style) {
    if app.uninstalling_tools.contains(tool_id) {
        return (
            "UNINSTALLING...".to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    }
    if let Some(job) = app.queue.iter().find(|job| job.item.tool_id == tool_id) {
        return match job.item.state {
            QueueState::Installing => (
                "INSTALLING...".to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            QueueState::Queued => ("QUEUED".to_string(), Style::default().fg(Color::Yellow)),
            QueueState::Success => ("INSTALLED".to_string(), Style::default().fg(Color::Green)),
            QueueState::Failed => ("FAILED".to_string(), Style::default().fg(Color::Red)),
            QueueState::Idle => ("PENDING".to_string(), Style::default().fg(MUTED)),
        };
    }
    if app.installed_tools.contains(tool_id) {
        ("INSTALLED".to_string(), Style::default().fg(Color::Green))
    } else {
        ("NOT INSTALLED".to_string(), Style::default().fg(MUTED))
    }
}

fn render_install(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = responsive_split(area, 88);
    let items = app
        .queue
        .iter()
        .map(|job| {
            ListItem::new(format!(
                "{:<22} {:<12} {:?}",
                job.item.tool_id, job.item.channel, job.item.state
            ))
        })
        .collect::<Vec<_>>();
    let mut state =
        ListState::default().with_selected((!app.queue.is_empty()).then_some(app.install_index));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel(&format!("Install queue ({})", app.queue.len())))
            .highlight_style(selection_style())
            .highlight_symbol("> "),
        chunks[0],
        &mut state,
    );

    let text = app.queue.get(app.install_index).map_or_else(
        || Text::from("Queue tools from Catalog with Enter or i."),
        |job| {
            let mut lines = vec![
                Line::from(format!("tool: {}", job.item.tool_id)),
                Line::from(format!("channel: {}", job.item.channel)),
                Line::from(format!("state: {:?}", job.item.state)),
                Line::from(format!("attempts: {}", job.item.attempts)),
                Line::from(format!("command: {}", job.task.command)),
                Line::from(format!(
                    "check: {}",
                    job.task
                        .check_command
                        .as_deref()
                        .unwrap_or("not configured")
                )),
                Line::from(""),
            ];
            if let Some(diagnostics) = &job.diagnostics {
                lines.push(Line::styled(
                    format!("exit: {:?}", diagnostics.exit_code),
                    Style::default().fg(Color::Red),
                ));
                lines.push(Line::from(format!("error: {}", diagnostics.stderr_summary)));
                lines.push(Line::from(format!("log: {}", diagnostics.full_log_path)));
            } else {
                lines.push(Line::styled(
                    "x execute   X run queue   c cancel   r retry   d remove",
                    Style::default().fg(MUTED),
                ));
            }
            Text::from(lines)
        },
    );
    frame.render_widget(Paragraph::new(text).block(panel("Queue item")), chunks[1]);
}

fn render_workspaces(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = responsive_split(area, 92);
    let items = app
        .workspaces
        .workspaces
        .iter()
        .map(|workspace| {
            let session = app
                .managed_sessions
                .iter()
                .find(|session| session.workspace_id == workspace.id);
            ListItem::new(format!(
                "{:<24} {:<8} {} apps",
                workspace.title,
                session.map_or("stopped", |_| "running"),
                workspace.layout.panes.len()
            ))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(app.workspace_index));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel("Workspace templates"))
            .highlight_style(selection_style())
            .highlight_symbol("> "),
        chunks[0],
        &mut state,
    );

    let detail = app.selected_workspace().map_or_else(
        || Text::from("No workspace selected"),
        |workspace| {
            let mut lines = vec![
                Line::styled(
                    &workspace.title,
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Line::from(format!("tools: {}", workspace.recommended_tools.join(", "))),
                Line::from(format!(
                    "view: {}",
                    match workspace.tmux_view {
                        TmuxView::Windows => "one full-screen app at a time",
                        TmuxView::Panes => "multi-app layout",
                    }
                )),
                Line::from(format!(
                    "status: {}",
                    app.managed_sessions
                        .iter()
                        .find(|session| session.workspace_id == workspace.id)
                        .map_or("stopped", |_| "running")
                )),
                Line::from(""),
            ];
            lines.extend(
                workspace
                    .layout
                    .panes
                    .iter()
                    .map(|pane| match workspace.tmux_view {
                        TmuxView::Windows => {
                            Line::from(format!("app {:<12} {}", pane.id, pane.cmd))
                        }
                        TmuxView::Panes => Line::from(format!(
                            "{} <- {}  {}  {}  {}",
                            pane.id,
                            pane.split,
                            format!("{:?}", pane.direction).to_ascii_lowercase(),
                            pane.size,
                            pane.cmd
                        )),
                    }),
            );
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Enter start/open  a open  x stop all  r refresh  h hash  I install tools",
                Style::default().fg(MUTED),
            ));
            Text::from(lines)
        },
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel("Layout"))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn render_app_view(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(view) = &app.app_view else {
        frame.render_widget(Paragraph::new("Opening workspace apps..."), area);
        return;
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);
    let titles = view
        .apps
        .iter()
        .map(|managed_app| Line::from(managed_app.window_name.clone()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Tabs::new(titles)
            .block(
                Block::default()
                    .title(format!(
                        " T4E Apps · {} · {} ",
                        view.workspace_title,
                        if app.mouse_enabled { "MOUSE" } else { "SELECT" }
                    ))
                    .borders(Borders::ALL),
            )
            .select(view.selected)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .divider(" | "),
        sections[0],
    );
    let title = view
        .apps
        .get(view.selected)
        .map(|managed_app| format!(" {} · {} ", managed_app.window_name, managed_app.process))
        .unwrap_or_else(|| " App ".to_string());
    let content = view
        .content
        .to_text()
        .unwrap_or_else(|_| Text::from(view.content.as_str()));
    frame.render_widget(
        Paragraph::new(content)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        sections[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("[Alt-Left/Right] Switch  [Alt-BS] Background  [Alt-Q] Close"),
            Line::from("[Alt-O] Open link  [Alt-C] Copy clean link"),
        ])
        .style(Style::default().fg(Color::Gray)),
        sections[2],
    );
}

fn render_agents(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(area);
    let chunks = responsive_split(vertical[0], 94);
    let mut conversation = Vec::new();
    for message in &app.ai_messages {
        conversation.push(Line::styled(
            format!("{}:", message.role),
            Style::default().fg(if message.role == "You" {
                SELECTED
            } else {
                ACCENT
            }),
        ));
        conversation.extend(message.text.lines().map(Line::from));
        conversation.push(Line::from(""));
    }
    if !app.ai_streaming.is_empty() {
        conversation.push(Line::styled("Codex:", Style::default().fg(ACCENT)));
        conversation.extend(app.ai_streaming.lines().map(Line::from));
    }
    frame.render_widget(
        Paragraph::new(conversation)
            .block(panel("Conversation"))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    let mut status = vec![
        Line::styled(
            &app.ai_status,
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from(format!("account: {}", app.ai_account)),
        Line::from("transport: local stdio"),
        Line::from("sandbox: read-only"),
        Line::from("approvals: denied"),
        Line::from(format!("usage: {}", compact_usage(&app.ai_usage))),
        Line::from(""),
    ];
    status.extend(app.agent_tools().iter().map(|agent| {
        Line::from(format!(
            "{} ({})",
            agent.name,
            agent.run_command_for_current_platform()
        ))
    }));
    status.extend([
        Line::from(""),
        Line::styled("Enter/i prompt   x interrupt", Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(Text::from(status))
            .block(panel("Codex app-server"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );

    let input = if app.ai_input_mode {
        format!("> {}_", app.ai_input)
    } else {
        "Press Enter to compose a request".to_string()
    };
    frame.render_widget(
        Paragraph::new(input)
            .block(panel("Prompt"))
            .style(Style::default().fg(if app.ai_input_mode { SELECTED } else { MUTED })),
        vertical[1],
    );
}

fn compact_usage(raw: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return raw.chars().take(96).collect();
    };
    if let Some(rate_limits) = value.get("rateLimits") {
        let name = rate_limits
            .get("limitName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("plan");
        if let Some(used) = rate_limits
            .pointer("/primary/usedPercent")
            .and_then(serde_json::Value::as_f64)
        {
            return format!("{name}: {used:.0}% used");
        }
        return name.to_string();
    }
    raw.chars().take(96).collect()
}

fn render_logs(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let lines = app
        .logs
        .iter()
        .rev()
        .map(|entry| Line::from(format!("> {}", entry)))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(&format!(
                "Activity log - newest first - row {}/{}",
                if app.logs.is_empty() {
                    0
                } else {
                    app.activity_scroll + 1
                },
                app.logs.len()
            )))
            .scroll((app.activity_scroll.min(u16::MAX as usize) as u16, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn activity_message(entry: &str) -> &str {
    entry
        .strip_prefix('[')
        .and_then(|entry| entry.split_once("] "))
        .map_or(entry, |(_, message)| message)
}

fn render_settings(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = responsive_split(area, 88);
    let values = [
        format!("Default mux              {}", app.settings.default_mux),
        format!(
            "Install timeout           {} sec",
            app.settings.install_timeout_sec
        ),
        format!(
            "Maximum install attempts  {}",
            app.settings.max_install_attempts
        ),
        format!(
            "Confirm all installs      {}",
            if app.settings.confirm_all_installs {
                "on"
            } else {
                "off"
            }
        ),
        "Reset saved preferences    Enter".to_string(),
    ];
    let items = values.into_iter().map(ListItem::new).collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(app.settings_index));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel("Runtime settings"))
            .highlight_style(selection_style())
            .highlight_symbol("> "),
        chunks[0],
        &mut state,
    );

    let detail = Text::from(vec![
        Line::styled(
            "Execution policy",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from("Changes are saved immediately."),
        Line::from("Timeout and retries apply to new install runs."),
        Line::from("Confirm-all requires typed approval for every tool."),
        Line::from("Reset restores runtime defaults and clears saved app options."),
        Line::from(""),
        Line::styled(
            "h/l or arrows adjust  Space toggles  Enter resets",
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel("Policy details"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
}

fn risk_explanation(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Safe => "app-owned config, cache, and UI state only",
        RiskLevel::Low => "network, account sign-in, or selected-file reads",
        RiskLevel::High => "camera capture, file writes, synchronization, or deletion",
        RiskLevel::Danger => "system changes, commands, or autonomous actions",
    }
}

fn render_footer(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let hint = if app.search_mode {
        format!("Search: {}_   Enter apply   Esc cancel", app.search_query)
    } else if app.ai_input_mode {
        "AI prompt input   Enter send   Esc cancel".to_string()
    } else if app.screen == Screen::Logs {
        format!(
            "{} | Up/Down 1 row  PageUp/PageDown 10  Home/End  c clear",
            app.status
        )
    } else {
        format!(
            "{} | arrows/jk move  Enter open/run  Backspace back  ? help",
            app.status
        )
    };
    frame.render_widget(
        Paragraph::new(hint)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Left),
        area,
    );
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let compact = area.height <= 12;
    let lines = if compact {
        vec![
            Line::styled(
                "Risk from capabilities",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Line::from("SAFE none | LOW network, account, or file read"),
            Line::from("HIGH camera capture, file write, or delete"),
            Line::from("DANGER system, commands, or autonomous operation"),
            Line::from("Highest capability level becomes the app risk level"),
            Line::styled(
                "App details list every declared capability.",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Line::from("Scripts always need approval; installs get postflight checks"),
            Line::from("Enter run | I install | U uninstall | R reinstall"),
            Line::from("Activity arrows/PgUp/PgDn | Alt+Q close | Alt+BS background"),
        ]
    } else {
        vec![
            Line::styled(
                "Capabilities and derived risk",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Line::from(
                "SAFE     no declared capability beyond app-owned configuration, cache, and local UI state",
            ),
            Line::from(
                "LOW      NETWORK, ACCOUNT, or FILE_READ: remote access, sign-in, or reading selected files",
            ),
            Line::from(
                "HIGH     CAMERA_CAPTURE, FILE_WRITE, or DELETE: can capture video or change selected files",
            ),
            Line::from(
                "DANGER   SYSTEM, COMMANDS, or AUTONOMOUS: system changes, general commands, or agentic action",
            ),
            Line::styled(
                "An app receives the highest level among its capabilities. Details show the complete capability list.",
                Style::default().fg(MUTED),
            ),
            Line::styled(
                "Installation policy",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Line::from(
                "Package-manager installs use a generated catalog plan and verify required executables afterward.",
            ),
            Line::styled(
                "Script installers always show the command and require explicit approval, regardless of app risk.",
                Style::default().fg(MUTED),
            ),
            Line::from(""),
            Line::styled(
                "Using T4E",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Line::from("arrows / j k       move selection"),
            Line::from("Enter               enter an app list or run the selected app"),
            Line::from("I / U / R           install / uninstall / reset and reinstall"),
            Line::from("Tab / Shift+Tab     switch dashboard tabs"),
            Line::from("/                   search apps in the current HOME view"),
            Line::from("Activity arrows     scroll one row; PageUp / PageDown scroll ten"),
            Line::from("Activity Home / End jump to newest / oldest entry"),
            Line::from("Alt+Left / Right    switch running apps"),
            Line::from("Alt+Backspace       leave an app running in the background"),
            Line::from("Alt+Q               close the current app"),
            Line::from("Alt+M               toggle text selection and T4E mouse controls"),
            Line::from("Alt+O / Alt+C       open or copy a link from the current app"),
            Line::from("Backspace / Esc     return to HOME outside a running app"),
            Line::from("q                   return to HOME, then quit"),
        ]
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(panel("Help"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_confirmation(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(confirmation) = &app.confirmation else {
        return;
    };
    let popup = centered_rect(76, 14, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled(
            "Explicit installation approval required",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("tool: {}", confirmation.tool_id)),
        Line::from(format!("command: {}", confirmation.command)),
        Line::from(""),
    ];
    if confirmation.typed {
        lines.push(Line::from(format!("Type: {}", confirmation.expected)));
        lines.push(Line::styled(
            format!("> {}_", confirmation.input),
            Style::default().fg(SELECTED),
        ));
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(
            "This install does not require a typed confirmation phrase.",
        ));
        lines.push(Line::from(""));
    }
    lines.push(Line::styled(
        "Enter confirm   Esc cancel",
        Style::default().fg(MUTED),
    ));
    let content = Text::from(lines);
    frame.render_widget(
        Paragraph::new(content)
            .block(panel("Install confirmation"))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_launch_options(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(state) = &app.launch_options else {
        return;
    };
    let Some(tool) = app
        .catalog
        .tools
        .iter()
        .find(|tool| tool.id == state.tool_id)
    else {
        return;
    };
    let popup = centered_rect(72, (tool.run_options.len() as u16 + 8).min(22), area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled(
            format!("{} launch options", state.tool_name),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
    ];
    for (index, (option, selection)) in tool.run_options.iter().zip(&state.selections).enumerate() {
        let value = option
            .values
            .get(selection.value_index)
            .map_or(String::new(), |value| format!("  < {value} >"));
        let setting = option
            .output_filter
            .map_or(option.flag.as_str(), |_| "lolcat");
        let line = format!(
            "{} [{}] {:<24} {}{}",
            if index == state.selected { ">" } else { " " },
            if selection.enabled { "x" } else { " " },
            option.label,
            setting,
            value
        );
        lines.push(Line::styled(
            line,
            if index == state.selected {
                selection_style()
            } else if selection.enabled {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            },
        ));
    }
    lines.extend([
        Line::from(""),
        Line::styled(
            "Space enable  Left/Right value  Enter launch  Esc cancel",
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Configure app"))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_launch_argument(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(state) = &app.launch_argument else {
        return;
    };
    let popup = centered_rect(84, 9, area);
    frame.render_widget(Clear, popup);
    let value = if state.input.is_empty() {
        Line::styled(&state.placeholder, Style::default().fg(MUTED))
    } else {
        Line::styled(format!("> {}_", state.input), selection_style())
    };
    let content = Text::from(vec![
        Line::styled(
            &state.tool_name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(state.label.as_str()),
        value,
        Line::from(""),
        Line::styled("Enter launch   Esc cancel", Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .block(panel("Launch input"))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_launch_approval(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(approval) = &app.launch_approval else {
        return;
    };
    let popup = centered_rect(76, 12, area);
    frame.render_widget(Clear, popup);
    let content = Text::from(vec![
        Line::styled(
            "Camera access required",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("app: {}", approval.tool_name)),
        Line::from("capability: CAMERA_CAPTURE (HIGH)"),
        Line::from(""),
        Line::from("This app will read live frames from the selected camera."),
        Line::from("Approval lasts for the current T4E session."),
        Line::from(""),
        Line::styled("Enter allow   Esc cancel", Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .block(panel("Sensitive device approval"))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_link_picker(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(picker) = &app.link_picker else {
        return;
    };
    let popup = centered_rect(100, (picker.urls.len() as u16 + 6).min(18), area);
    frame.render_widget(Clear, popup);
    let items = picker
        .urls
        .iter()
        .enumerate()
        .map(|(index, url)| {
            let prefix = if index == 0 { "Latest  " } else { "        " };
            ListItem::new(format!("{prefix}{url}"))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(picker.selected));
    let action = match picker.action {
        LinkAction::Open => "Open link",
        LinkAction::Copy => "Copy link",
    };
    frame.render_stateful_widget(
        List::new(items)
            .block(panel(action))
            .highlight_style(selection_style())
            .highlight_symbol("> "),
        popup,
        &mut state,
    );
    let hint = Rect {
        x: popup.x + 2,
        y: popup.y + popup.height.saturating_sub(2),
        width: popup.width.saturating_sub(4),
        height: 1,
    };
    frame.render_widget(
        Paragraph::new("Up/Down select   Enter confirm   Esc cancel")
            .style(Style::default().fg(MUTED)),
        hint,
    );
}

fn render_uninstall_confirmation(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(request) = &app.uninstall_confirmation else {
        return;
    };
    let popup = centered_rect(76, 12, area);
    frame.render_widget(Clear, popup);
    let (heading, explanation, action, title) = if request.reinstall {
        (
            "Reset and reinstall app",
            "T4E will remove the current package, clear its queue item, and reinstall it.",
            "Enter reset and reinstall   Esc cancel",
            "Reinstall confirmation",
        )
    } else {
        (
            "Remove installed app",
            "The package manager will remove this app.",
            "Enter uninstall   Esc cancel",
            "Uninstall confirmation",
        )
    };
    let content = Text::from(vec![
        Line::styled(
            heading,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("app: {}", request.tool_id)),
        Line::from(format!("command: {}", request.command)),
        Line::from(""),
        Line::from(explanation),
        Line::from(""),
        Line::styled(action, Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .block(panel(title))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_ai_confirmation(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(confirmation) = &app.ai_confirmation else {
        return;
    };
    let popup = centered_rect(76, 14, area);
    frame.render_widget(Clear, popup);
    let content = Text::from(vec![
        Line::styled(
            "AI action approval required",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("action: {}", confirmation.action.kind)),
        Line::from(format!("target: {}", confirmation.action.target)),
        Line::from(""),
        Line::from(format!("Type: {}", confirmation.expected)),
        Line::styled(
            format!("> {}_", confirmation.input),
            Style::default().fg(SELECTED),
        ),
        Line::from(""),
        Line::styled("Enter confirm   Esc cancel", Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .block(panel("Bounded action confirmation"))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn responsive_split(area: Rect, breakpoint: u16) -> Vec<Rect> {
    let (direction, constraints) = if area.width >= breakpoint {
        (
            Direction::Horizontal,
            [Constraint::Percentage(44), Constraint::Percentage(56)],
        )
    } else {
        (
            Direction::Vertical,
            [Constraint::Percentage(52), Constraint::Percentage(48)],
        )
    };
    Layout::default()
        .direction(direction)
        .constraints(constraints)
        .split(area)
        .to_vec()
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn panel(title: &str) -> Block<'_> {
    Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
}

fn selection_style() -> Style {
    Style::default().fg(SELECTED).add_modifier(Modifier::BOLD)
}

fn risk_style(risk: RiskLevel) -> Style {
    let color = match risk {
        RiskLevel::Safe => Color::Green,
        RiskLevel::Low => Color::Cyan,
        RiskLevel::High => Color::Yellow,
        RiskLevel::Danger => Color::Red,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::compact_usage;

    #[test]
    fn usage_summary_keeps_only_limit_name_and_percent() {
        let raw = r#"{"rateLimits":{"limitName":"Codex Plan","primary":{"usedPercent":12.4}}}"#;
        assert_eq!(compact_usage(raw), "Codex Plan: 12% used");
    }
}
