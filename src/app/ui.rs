use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};

use crate::catalog::models::{Exposure, Risk};

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
        Screen::Agents => render_agents(frame, app, sections[1]),
        Screen::Logs => render_logs(frame, app, sections[1]),
        Screen::Settings => render_settings(frame, app, sections[1]),
    }
    render_footer(frame, app, sections[2]);

    if app.show_help {
        render_help(frame, area);
    } else if app.confirmation.is_some() {
        render_confirmation(frame, app, area);
    } else if app.ai_confirmation.is_some() {
        render_ai_confirmation(frame, app, area);
    }
}

fn render_header(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let labels = if area.width < 80 {
        ["1 H", "2 App", "3 Q", "4 Work", "5 AI", "6 Log", "7 Set"]
    } else {
        [
            "1 Home", "2 Apps", "3 Queue", "4 Work", "5 AI", "6 Logs", "7 Setup",
        ]
    };
    let titles = labels.into_iter().map(Line::from).collect::<Vec<_>>();
    let selected = match app.screen {
        Screen::Home => 0,
        Screen::Catalog => 1,
        Screen::Install => 2,
        Screen::Workspace => 3,
        Screen::Agents => 4,
        Screen::Logs => 5,
        Screen::Settings => 6,
    };
    let tabs = Tabs::new(titles)
        .block(Block::default().title(" t4e ").borders(Borders::ALL))
        .select(selected)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .divider(if area.width < 80 { " " } else { " | " });
    frame.render_widget(tabs, area);
}

fn render_home(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = responsive_split(area, 88);
    let pack_items = app
        .catalog
        .packs
        .iter()
        .map(|pack| {
            let visibility = match pack.exposure {
                Exposure::Starter => "starter",
                Exposure::SearchOnly => "search",
                Exposure::Labs => "labs",
            };
            ListItem::new(format!(
                "{:<24} {:>2} tools  {}",
                pack.title,
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
            Span::styled("Sessions ", Style::default().fg(Color::Green)),
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
        Line::from("Enter view pack   I queue pack"),
        Line::from("c catalog   i queue   w workspaces"),
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
            ListItem::new(Line::from(vec![
                Span::raw(format!(
                    "{}{} ",
                    if app.selected_tools.contains(&tool.id) {
                        "[x]"
                    } else {
                        "[ ]"
                    },
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
                Span::raw(&tool.name),
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
                Line::from(format!("run: {}", tool.run.cmd)),
                Line::from(""),
            ];
            lines.extend(tool.installers.iter().map(|installer| {
                Line::from(format!("{:?}: {:?}", installer.platform, installer.method))
            }));
            lines.extend([
                Line::from(""),
                Line::styled(
                    "Space select  a all  I queue  f favorite  p all packs",
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
                "{:<20} {:<8} {:<7} {} panes",
                workspace.title,
                format!("{:?}", workspace.mux).to_ascii_lowercase(),
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
                Line::from(format!(
                    "target: {}",
                    workspace.session_name.as_deref().unwrap_or("auto")
                )),
                Line::from(format!("tools: {}", workspace.recommended_tools.join(", "))),
                Line::from(format!(
                    "session: {}",
                    app.managed_sessions
                        .iter()
                        .find(|session| session.workspace_id == workspace.id)
                        .map_or("stopped", |session| session.name.as_str())
                )),
                Line::from(""),
            ];
            lines.extend(workspace.layout.panes.iter().map(|pane| {
                Line::from(format!(
                    "{} <- {}  {}  {}  {}",
                    pane.id,
                    pane.split,
                    format!("{:?}", pane.direction).to_ascii_lowercase(),
                    pane.size,
                    pane.cmd
                ))
            }));
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Enter launch  a attach  x stop  r refresh  h hash  I install tools",
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
    status.extend(
        app.agent_tools()
            .iter()
            .map(|agent| Line::from(format!("{} ({})", agent.name, agent.run.cmd))),
    );
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
    } else if area.width < 80 {
        format!("Tab j/k ? help q back | {}", app.status)
    } else {
        format!(
            "{} | Tab switch  arrows/jk move  ? help  q back/quit",
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
        Line::from("1-7 / Tab     switch screens"),
        Line::from("arrows / j k  move selection"),
        Line::from("Enter         screen action"),
        Line::from("/             search catalog"),
        Line::from("q / Esc       back, then quit"),
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
    let content = Text::from(vec![
        Line::styled(
            "Explicit installation approval required",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("tool: {}", confirmation.tool_id)),
        Line::from(format!("command: {}", confirmation.command)),
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
            .block(panel("Install confirmation"))
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
