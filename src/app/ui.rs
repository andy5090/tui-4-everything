use ansi_to_tui::IntoText as _;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};

use crate::catalog::models::{Exposure, Risk};
use crate::installer::queue::QueueState;
use crate::mux::workspace::TmuxView;

use super::events::Screen;
use super::state::AppState;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const SELECTED: Color = Color::Yellow;

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let area = frame.area();
    if area.width < 60 || area.height < 16 {
        frame.render_widget(
            Paragraph::new("t4e needs a terminal of at least 60x16")
                .alignment(Alignment::Center)
                .block(panel("Terminal too small")),
            area,
        );
        return;
    }

    if app.screen == Screen::AppView {
        render_app_view(frame, app, area);
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
    }
    render_footer(frame, app, sections[2]);

    if app.show_help {
        render_help(frame, area);
    } else if app.launch_options.is_some() {
        render_launch_options(frame, app, area);
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
        Screen::Home => "Packs".to_string(),
        Screen::Catalog => app
            .active_pack
            .as_ref()
            .and_then(|id| app.catalog.packs.iter().find(|pack| &pack.id == id))
            .map(|pack| pack.title.clone())
            .unwrap_or_else(|| "All apps".to_string()),
        Screen::Install => "Installs".to_string(),
        Screen::Workspace => "Workspaces".to_string(),
        Screen::AppView => "Running apps".to_string(),
        Screen::Agents => "AI".to_string(),
        Screen::Logs => "Activity".to_string(),
        Screen::Settings => "Settings".to_string(),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "t4e",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  /  "),
            Span::raw(section),
        ]))
        .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn render_home(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = responsive_split(area, 88);
    let pack_items = app
        .catalog
        .packs
        .iter()
        .map(|pack| {
            let app_count = pack
                .tool_ids
                .iter()
                .filter_map(|id| app.catalog.tools.iter().find(|tool| &tool.id == id))
                .filter(|tool| tool.is_launchable_app())
                .count();
            let visibility = match pack.exposure {
                Exposure::Starter => "starter",
                Exposure::SearchOnly => "search",
                Exposure::Labs => "labs",
            };
            ListItem::new(format!(
                "{:<24} {:>2} apps / {:>2} tools  {}",
                pack.title,
                app_count,
                pack.tool_ids.len(),
                visibility
            ))
        })
        .collect::<Vec<_>>();
    let mut pack_state = ListState::default().with_selected(Some(app.pack_index));
    frame.render_stateful_widget(
        List::new(pack_items)
            .block(panel("Starter packs"))
            .highlight_style(selection_style())
            .highlight_symbol("> "),
        chunks[0],
        &mut pack_state,
    );

    let summary = vec![
        Line::from(vec![
            Span::styled("Catalog  ", Style::default().fg(ACCENT)),
            Span::raw(format!("{} tools", app.catalog.tools.len())),
        ]),
        Line::from(vec![
            Span::styled("Workspaces ", Style::default().fg(Color::Green)),
            Span::raw(format!(
                "{} workspace templates",
                app.workspaces.workspaces.len()
            )),
        ]),
        Line::from(vec![
            Span::styled("Queue    ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{} planned installs", app.queue.len())),
        ]),
        Line::from(vec![
            Span::styled("Saved    ", Style::default().fg(Color::Magenta)),
            Span::raw(format!(
                "{} favorites, {} recents",
                app.favorites.len(),
                app.recents.len()
            )),
        ]),
        Line::from(""),
        Line::from("Enter open pack   I install pack"),
        Line::from("c all apps   i installs   w workspaces"),
        Line::from("a agents   l logs   s settings"),
    ];
    frame.render_widget(
        Paragraph::new(summary)
            .block(panel("System overview"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
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
                Span::styled(
                    format!("{:<10}", AppState::risk_label(&tool.risk)),
                    risk_style(&tool.risk),
                ),
                Span::raw(format!("{:<20}", tool.name)),
                Span::styled(install_status, install_style),
            ]))
        })
        .collect::<Vec<_>>();
    let scope = app.active_pack.as_deref().unwrap_or("all packs");
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
                Line::from(format!("id: {}", tool.id)),
                Line::from(format!("risk: {}", AppState::risk_label(&tool.risk))),
                Line::from(format!("run: {}", tool.run_command_for_current_platform())),
                Line::from(format!("launch options: {}", tool.run_options.len())),
                Line::from(""),
            ];
            if let Some(job) = app.queue.iter().find(|job| job.item.tool_id == tool.id) {
                lines.push(Line::styled(
                    format!("install: {}", catalog_install_status(app, &tool.id).0),
                    catalog_install_status(app, &tool.id).1,
                ));
                lines.push(Line::from(format!("channel: {}", job.item.channel)));
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
                    .filter(|line| {
                        line.starts_with(&format!("{} [", tool.id))
                            || line.starts_with(&format!("install: {}", tool.id))
                            || line.starts_with(&format!("uninstall: {}", tool.id))
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
                    lines.extend(
                        recent
                            .into_iter()
                            .rev()
                            .map(|line| Line::from(line.as_str())),
                    );
                }
            } else {
                let (status, style) = catalog_install_status(app, &tool.id);
                lines.push(Line::styled(format!("install: {status}"), style));
            }
            lines.extend([
                Line::from(""),
                Line::styled(
                    "Enter run  I install  U uninstall  f favorite  Backspace packs",
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
                        " t4e Apps · {} · {} ",
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
        Paragraph::new("[Tab] Next  [Shift-Tab] Previous  [Alt-BS] Background  [Alt-Q] Close")
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
        Line::styled(
            "Enter/i prompt   x interrupt   A approve action",
            Style::default().fg(MUTED),
        ),
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
            .block(panel("Activity log - newest first"))
            .wrap(Wrap { trim: false }),
        area,
    );
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
        Line::from(""),
        Line::styled(
            "h/l or arrows adjust  Space toggles",
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

fn render_footer(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let hint = if app.search_mode {
        format!("Search: {}_   Enter apply   Esc cancel", app.search_query)
    } else if app.ai_input_mode {
        "AI prompt input   Enter send   Esc cancel".to_string()
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
    let popup = centered_rect(64, 16, area);
    frame.render_widget(Clear, popup);
    let help = Text::from(vec![
        Line::styled(
            "Navigation",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from("arrows / j k  move selection"),
        Line::from("Enter         open pack or run app"),
        Line::from("Tab           switch running apps"),
        Line::from("Backspace     back / keep app running"),
        Line::from("Alt+M         toggle text selection / mouse controls"),
        Line::from("/             search catalog"),
        Line::from("q / Esc       main, then quit"),
        Line::from("Ctrl-C        quit immediately"),
        Line::from(""),
        Line::styled("Press any key to close", Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(help)
            .block(panel("Help"))
            .wrap(Wrap { trim: true }),
        popup,
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
        lines.push(Line::from("SAFE app: no confirmation phrase is required."));
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
        let line = format!(
            "{} [{}] {:<24} {}{}",
            if index == state.selected { ">" } else { " " },
            if selection.enabled { "x" } else { " " },
            option.label,
            option.flag,
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

fn render_uninstall_confirmation(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(request) = &app.uninstall_confirmation else {
        return;
    };
    let popup = centered_rect(76, 12, area);
    frame.render_widget(Clear, popup);
    let content = Text::from(vec![
        Line::styled(
            "Remove installed app",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("app: {}", request.tool_id)),
        Line::from(format!("command: {}", request.command)),
        Line::from(""),
        Line::from("The package manager will remove this app."),
        Line::from(""),
        Line::styled("Enter uninstall   Esc cancel", Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .block(panel("Uninstall confirmation"))
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

fn risk_style(risk: &Risk) -> Style {
    let color = match risk {
        Risk::Safe => Color::Green,
        Risk::Caution => Color::Yellow,
        Risk::Admin => Color::Magenta,
        Risk::High => Color::Red,
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
