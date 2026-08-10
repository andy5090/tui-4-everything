use ansi_to_tui::IntoText as _;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::{Frame, Terminal};

use crate::catalog::models::RiskLevel;
use crate::installer::queue::QueueState;
use crate::mux::workspace::TmuxView;
use crate::storage::{ProviderAuthMode, default_api_provider_profiles};

use super::events::Screen;
use super::state::{
    AiWorkflowPhase, AppState, HomeFilter, HomeFocus, LinkAction, NAVIGATION_TAB_LABELS,
    ToolUpdateState,
};
use super::theme::{activate as activate_theme, active_palette};

fn accent() -> Color {
    active_palette().accent
}

fn muted() -> Color {
    active_palette().muted
}

fn selected() -> Color {
    active_palette().selected
}

type ScreenPoint = (u16, u16);
type PanelSelection = (Rect, ScreenPoint, ScreenPoint);

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let area = frame.area();
    activate_theme(app.settings.theme);
    let palette = active_palette();
    frame.render_widget(
        Block::default().style(
            Style::default()
                .bg(palette.background)
                .fg(palette.foreground),
        ),
        area,
    );
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
        render_mouse_selection(frame, app);
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
    } else if app.api_provider_setup.is_some() {
        render_api_provider_setup(frame, app, area);
    }
    render_mouse_selection(frame, app);
}

pub(crate) fn extract_panel_selection(
    app: &mut AppState,
    width: u16,
    height: u16,
    start: ScreenPoint,
    end: ScreenPoint,
) -> Option<String> {
    if width < 2 || height < 2 {
        return None;
    }
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).ok()?;
    terminal.draw(|frame| render(frame, app)).ok()?;
    selection_text(terminal.backend().buffer(), start, end)
}

fn render_mouse_selection(frame: &mut Frame<'_>, app: &AppState) {
    let Some(selection) = app.mouse_selection else {
        return;
    };
    let Some((panel, start, end)) =
        panel_selection(frame.buffer_mut(), selection.start, selection.end)
    else {
        return;
    };
    let buffer = frame.buffer_mut();
    for_each_selected_cell(panel, start, end, |x, y| {
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
    });
}

fn selection_text(buffer: &Buffer, start: ScreenPoint, end: ScreenPoint) -> Option<String> {
    let (panel, start, end) = panel_selection(buffer, start, end)?;
    let mut lines = Vec::<String>::new();
    let mut current_row = None;
    for_each_selected_cell(panel, start, end, |x, y| {
        if current_row != Some(y) {
            lines.push(String::new());
            current_row = Some(y);
        }
        if let Some(cell) = buffer.cell((x, y)) {
            if is_wide_continuation(buffer, x, y) {
                return;
            }
            lines
                .last_mut()
                .expect("selection row exists")
                .push_str(cell.symbol());
        }
    });
    for line in &mut lines {
        *line = line.trim_end().to_string();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        return None;
    }

    let compact = lines.iter().map(|line| line.trim()).collect::<Vec<_>>();
    if compact
        .first()
        .is_some_and(|line| line.starts_with("https://") || line.starts_with("http://"))
        && compact
            .iter()
            .all(|line| !line.is_empty() && line.split_whitespace().count() == 1)
    {
        return Some(compact.concat());
    }
    Some(lines.join("\n"))
}

fn is_wide_continuation(buffer: &Buffer, x: u16, y: u16) -> bool {
    (buffer.area().x..x).rev().any(|lead_x| {
        buffer.cell((lead_x, y)).is_some_and(|cell| {
            let width = Span::raw(cell.symbol()).width() as u16;
            width > 1 && lead_x.saturating_add(width) > x
        })
    })
}

fn panel_selection(
    buffer: &Buffer,
    start: ScreenPoint,
    end: ScreenPoint,
) -> Option<PanelSelection> {
    let panel = enclosing_panel(buffer, start)?;
    let left = panel.x.saturating_add(1);
    let right = panel.right().saturating_sub(2);
    let top = panel.y.saturating_add(1);
    let bottom = panel.bottom().saturating_sub(2);
    if start.0 < left || start.0 > right || start.1 < top || start.1 > bottom {
        return None;
    }
    let end = (end.0.clamp(left, right), end.1.clamp(top, bottom));
    let (start, end) = if (start.1, start.0) <= (end.1, end.0) {
        (start, end)
    } else {
        (end, start)
    };
    Some((panel, start, end))
}

fn enclosing_panel(buffer: &Buffer, point: ScreenPoint) -> Option<Rect> {
    let area = *buffer.area();
    let mut panels = Vec::new();
    for top in area.y..point.1 {
        for left in area.x..point.0 {
            if buffer.cell((left, top)).map(|cell| cell.symbol()) != Some("┌") {
                continue;
            }
            for right in point.0.saturating_add(1)..area.right() {
                if buffer.cell((right, top)).map(|cell| cell.symbol()) != Some("┐") {
                    continue;
                }
                for bottom in point.1.saturating_add(1)..area.bottom() {
                    if buffer.cell((left, bottom)).map(|cell| cell.symbol()) != Some("└")
                        || buffer.cell((right, bottom)).map(|cell| cell.symbol()) != Some("┘")
                    {
                        continue;
                    }
                    let sides_are_intact = (top + 1..bottom).all(|y| {
                        buffer.cell((left, y)).map(|cell| cell.symbol()) == Some("│")
                            && buffer.cell((right, y)).map(|cell| cell.symbol()) == Some("│")
                    });
                    let bottom_is_intact = (left + 1..right)
                        .all(|x| buffer.cell((x, bottom)).map(|cell| cell.symbol()) == Some("─"));
                    if sides_are_intact && bottom_is_intact {
                        panels.push(Rect::new(
                            left,
                            top,
                            right.saturating_sub(left).saturating_add(1),
                            bottom.saturating_sub(top).saturating_add(1),
                        ));
                    }
                }
            }
        }
    }
    panels
        .into_iter()
        .max_by_key(|panel| u32::from(panel.width) * u32::from(panel.height))
}

fn for_each_selected_cell(
    panel: Rect,
    start: ScreenPoint,
    end: ScreenPoint,
    mut visit: impl FnMut(u16, u16),
) {
    let left = panel.x + 1;
    let right = panel.right() - 2;
    for y in start.1..=end.1 {
        let row_start = if y == start.1 { start.0 } else { left };
        let row_end = if y == end.1 { end.0 } else { right };
        for x in row_start..=row_end {
            visit(x, y);
        }
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
                            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(format!(" · {section} ")),
                    ]))
                    .borders(Borders::ALL)
                    .style(tab_style())
                    .border_style(tab_border_style()),
            )
            .select(app.navigation_tab_index())
            .style(tab_style())
            .highlight_style(selected_tab_style())
            .padding("", "")
            .divider(" | "),
        area,
    );
}

fn render_home(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let [library_area, apps_column_area, trailing_area] = home_layout(area);
    let split_right_columns_for_ai = area.width >= 110 && apps_column_area.height >= 16;
    let (apps_area, ai_area, information_area) = if split_right_columns_for_ai {
        let right_area = Rect::new(
            apps_column_area.x,
            apps_column_area.y,
            trailing_area.right().saturating_sub(apps_column_area.x),
            apps_column_area.height,
        );
        let right_sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(right_area);
        let upper_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(apps_column_area.width),
                Constraint::Min(1),
            ])
            .split(right_sections[0]);
        (
            upper_columns[0],
            Some(right_sections[1]),
            Some(upper_columns[1]),
        )
    } else if app.home_focus == HomeFocus::Assistant {
        (apps_column_area, Some(trailing_area), None)
    } else {
        (apps_column_area, None, Some(trailing_area))
    };
    let left_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(library_area);
    let library_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(6)])
        .split(left_sections[1]);
    let search_value = if app.search_query.is_empty() {
        Span::styled("Search apps...", Style::default().fg(muted()))
    } else {
        Span::raw(app.search_query.clone())
    };
    let search_cursor = app.search_mode.then(|| {
        Span::styled(
            "│",
            Style::default().fg(selected()).add_modifier(Modifier::BOLD),
        )
    });
    let mut search_line = vec![Span::raw(" "), search_value];
    if let Some(cursor) = search_cursor {
        search_line.push(cursor);
    }
    frame.render_widget(
        Paragraph::new(Line::from(search_line)).block(home_panel("Search", app.search_mode)),
        left_sections[0],
    );
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
    let library_selected =
        (!app.search_mode && app.home_filter_index < 3).then_some(app.home_filter_index);
    let mut library_state = ListState::default().with_selected(library_selected);
    let quick_access_focused =
        !app.search_mode && app.home_focus == HomeFocus::Views && app.home_filter_index < 3;
    frame.render_stateful_widget(
        List::new(library_filters)
            .block(home_panel("Quick Access", quick_access_focused))
            .highlight_style(home_selection_style(quick_access_focused))
            .highlight_symbol(home_selection_symbol(quick_access_focused)),
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
    let category_selected = (!app.search_mode)
        .then(|| app.home_filter_index.checked_sub(3))
        .flatten();
    let mut category_state = ListState::default().with_selected(category_selected);
    let categories_focused =
        !app.search_mode && app.home_focus == HomeFocus::Views && app.home_filter_index >= 3;
    frame.render_stateful_widget(
        List::new(categories)
            .block(home_panel("Apps", categories_focused))
            .highlight_style(home_selection_style(categories_focused))
            .highlight_symbol(home_selection_symbol(categories_focused)),
        library_sections[1],
        &mut category_state,
    );

    let tools = app.home_tools();
    let app_items = tools
        .iter()
        .map(|tool| {
            let is_installing = app
                .queue
                .iter()
                .any(|job| job.item.tool_id == tool.id && job.item.state == QueueState::Installing);
            let (state, state_style) = if app.is_tool_running(&tool.id) {
                ("RUNNING", Style::default().fg(accent()))
            } else if is_installing {
                (
                    "INSTALLING",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else if matches!(
                app.tool_updates.get(&tool.id),
                Some(ToolUpdateState::Drift { .. })
            ) {
                (
                    "UPDATE",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else if app.installed_tools.contains(&tool.id) {
                ("INSTALLED", Style::default().fg(Color::Green))
            } else {
                ("", Style::default())
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
                    Style::default().fg(muted()),
                ),
                Span::styled(state, state_style),
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
    let app_list_focused = !app.search_mode && app.home_focus == HomeFocus::AppList;
    frame.render_stateful_widget(
        List::new(app_items)
            .block(home_panel(&app_title, app_list_focused))
            .highlight_style(home_selection_style(app_list_focused))
            .highlight_symbol(home_selection_symbol(app_list_focused)),
        apps_area,
        &mut app_state,
    );
    if let Some(ai_area) = ai_area {
        render_home_ai(frame, app, ai_area);
    }

    let mut information = if app.system_overview.logo.is_empty() {
        app.system_overview
            .lines
            .iter()
            .map(|line| ansi_line(line))
            .collect::<Vec<_>>()
    } else {
        let logo_width = app
            .system_overview
            .logo
            .iter()
            .map(|line| ansi_line(line).width())
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
                let logo = ansi_line(logo);
                let padding = logo_width.saturating_sub(logo.width());
                let mut spans = logo.spans;
                if let Some(detail) = app.system_overview.lines.get(index) {
                    spans.push(Span::raw(" ".repeat(padding + 2)));
                    spans.extend(ansi_line(detail).spans);
                }
                Line::from(spans)
            })
            .collect::<Vec<_>>()
    };
    information.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled("Available: ", Style::default().fg(accent())),
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
            Span::styled("Running: ", Style::default().fg(Color::Green)),
            Span::raw(format!(
                "{} apps",
                app.app_view.as_ref().map_or(0, |view| view.apps.len())
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
    let active_install = app.selected_home_tool().and_then(|tool| {
        app.queue
            .iter()
            .find(|job| job.item.tool_id == tool.id && job.item.state == QueueState::Installing)
            .map(|job| (tool, job))
    });
    let (information_summary_area, install_log_area) =
        information_area.map_or((Rect::default(), None), |information_area| {
            active_install.map_or((information_area, None), |_| {
                let areas = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(8), Constraint::Length(7)])
                    .split(information_area);
                (areas[0], Some(areas[1]))
            })
        });
    let information_title = format!("Information · {}", app.system_overview.source);
    if !information_summary_area.is_empty() {
        let information = Paragraph::new(information).block(panel(&information_title));
        frame.render_widget(information, information_summary_area);
    }
    if let (Some((tool, job)), Some(log_area)) = (active_install, install_log_area) {
        let mut log_lines = vec![Line::styled(
            format!(
                "attempt {}/{} · {}",
                job.item.attempts.saturating_add(1),
                app.settings.max_install_attempts,
                job.item.channel
            ),
            Style::default().fg(muted()),
        )];
        let recent = recent_tool_activity(app, &tool.id, 4);
        if recent.is_empty() {
            log_lines.push(Line::from("Waiting for installer output..."));
        } else {
            log_lines.extend(recent.into_iter().map(Line::from));
        }
        frame.render_widget(
            Paragraph::new(log_lines).block(panel(&format!("Installing · {}", tool.name))),
            log_area,
        );
    }
}

fn render_home_ai(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let focused = app.home_focus == HomeFocus::Assistant;
    let (workflow_area, conversation_area) = if area.height >= 9 {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(area);
        (Some(sections[0]), sections[1])
    } else {
        (None, area)
    };
    if let Some(workflow_area) = workflow_area {
        frame.render_widget(
            Paragraph::new(ai_workflow_line(app.ai_workflow_phase))
                .alignment(Alignment::Center)
                .block(home_panel("Request · Review · Run", focused)),
            workflow_area,
        );
    }
    let provider_count = app.ai_ready_providers.len();
    let mut lines = Vec::new();
    if workflow_area.is_none() {
        lines.push(ai_workflow_line(app.ai_workflow_phase));
    }
    lines.extend([
        Line::from(vec![
            Span::styled(
                format!("{} · ", app.ai_provider.label()),
                Style::default().fg(if app.ai_ready() {
                    Color::Green
                } else {
                    Color::Red
                }),
            ),
            Span::raw(&app.ai_status),
        ]),
        Line::styled(
            if app.ai_ready() {
                format!(
                    "account: {} · {} provider{} ready · change in Settings",
                    app.ai_account,
                    provider_count,
                    if provider_count == 1 { "" } else { "s" }
                )
            } else {
                "Configure a CLI or API provider in Settings".to_string()
            },
            Style::default().fg(muted()),
        ),
    ]);
    for message in &app.ai_messages {
        lines.push(Line::styled(
            format!("{}:", message.role),
            Style::default().fg(accent()),
        ));
        lines.extend(
            message
                .text
                .lines()
                .map(|line| Line::from(line.to_string())),
        );
        lines.push(Line::from(""));
    }
    if !app.ai_streaming.is_empty() {
        lines.push(Line::styled(
            format!("{}:", app.ai_provider.label()),
            Style::default().fg(accent()),
        ));
        lines.extend(app.ai_streaming.lines().map(Line::from));
        lines.push(Line::from(""));
    }
    let composer = if app.ai_input_mode {
        format!("> {}_", app.ai_input)
    } else if app.ai_ready() {
        format!(
            "Type to ask AI · /: skill · permission: {}",
            app.settings.ai_approval_mode.label()
        )
    } else {
        "AI input disabled until a provider is ready".to_string()
    };
    lines.push(Line::styled(
        composer,
        Style::default().fg(if focused { selected() } else { muted() }),
    ));
    let conversation = Paragraph::new(lines)
        .block(home_panel("Assistant", focused))
        .wrap(Wrap { trim: true });
    let max_scroll = conversation
        .line_count(conversation_area.width)
        .saturating_sub(usize::from(conversation_area.height))
        .min(usize::from(u16::MAX));
    let scroll = max_scroll.saturating_sub(app.ai_conversation_scroll.min(max_scroll)) as u16;
    frame.render_widget(conversation.scroll((scroll, 0)), conversation_area);
}

fn ai_workflow_line(phase: AiWorkflowPhase) -> Line<'static> {
    let active = phase.step();
    let mut spans = Vec::new();
    for (index, label) in ["REQUEST", "REVIEW", "RUN"].into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  >  ", Style::default().fg(muted())));
        }
        let (marker, style) = match active {
            Some(current) if index < current => (
                "✓ ",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Some(current) if index == current => (
                "● ",
                Style::default()
                    .fg(selected())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ),
            _ => ("○ ", Style::default().fg(muted())),
        };
        spans.push(Span::styled(format!("{marker}{label}"), style));
    }
    Line::from(spans)
}

fn ansi_line(value: &str) -> Line<'static> {
    value
        .into_text()
        .ok()
        .and_then(|text| text.lines.into_iter().next())
        .unwrap_or_else(|| Line::from(value.to_string()))
}

fn home_layout(area: Rect) -> [Rect; 3] {
    if area.width >= 120 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(24),
                Constraint::Min(40),
                Constraint::Length(56),
            ])
            .split(area);
        return [columns[0], columns[1], columns[2]];
    }
    if area.width >= 110 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22),
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

fn home_selection_style(_focused: bool) -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn home_selection_symbol(focused: bool) -> &'static str {
    if focused { "> " } else { "  " }
}

fn home_panel(title: &str, focused: bool) -> Block<'_> {
    panel(title).border_style(if focused {
        Style::default().fg(accent())
    } else {
        Style::default().fg(active_palette().border)
    })
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
                let recent = recent_tool_activity(app, &tool.id, 4);
                if !recent.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::styled(
                        match job.item.state {
                            QueueState::Installing => "Live install output",
                            QueueState::Failed => "Last output before failure",
                            _ => "Recent install output",
                        },
                        Style::default().fg(muted()),
                    ));
                    lines.extend(recent.into_iter().map(Line::from));
                }
            } else {
                let (status, style) = catalog_install_status(app, &tool.id);
                lines.push(Line::styled(format!("install: {status}"), style));
            }
            lines.extend([
                Line::from(""),
                Line::styled(
                    "Enter run  I install  U remove  R reinstall  f favorite  Backspace HOME",
                    Style::default().fg(muted()),
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
            QueueState::Idle => ("PENDING".to_string(), Style::default().fg(muted())),
        };
    }
    if let Some(ToolUpdateState::Drift {
        installed,
        verified,
    }) = app.tool_updates.get(tool_id)
    {
        return (
            format!("UPDATE {installed}→{verified}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    }
    if app.installed_tools.contains(tool_id) {
        ("INSTALLED".to_string(), Style::default().fg(Color::Green))
    } else {
        ("NOT INSTALLED".to_string(), Style::default().fg(muted()))
    }
}

fn recent_tool_activity<'a>(app: &'a AppState, tool_id: &str, limit: usize) -> Vec<&'a str> {
    let mut recent = app
        .logs
        .iter()
        .rev()
        .filter_map(|line| {
            let message = activity_message(line);
            (message.starts_with(&format!("{tool_id} ["))
                || message.starts_with(&format!("install: {tool_id}"))
                || message.starts_with(&format!("uninstall: {tool_id}")))
            .then_some(message)
        })
        .take(limit)
        .collect::<Vec<_>>();
    recent.reverse();
    recent
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
                    Style::default().fg(muted()),
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
                Style::default().fg(muted()),
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
                    .borders(Borders::ALL)
                    .style(tab_style())
                    .border_style(tab_border_style()),
            )
            .select(view.selected)
            .style(tab_style())
            .highlight_style(selected_tab_style())
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
    let canvas_style = app_canvas_background(&content, sections[1].width.saturating_sub(2))
        .map_or_else(Style::default, |background| Style::default().bg(background));
    frame.render_widget(
        Paragraph::new(content)
            .style(canvas_style)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .style(canvas_style),
            )
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

fn app_canvas_background(content: &Text<'_>, viewport_width: u16) -> Option<Color> {
    if viewport_width == 0 {
        return None;
    }

    let minimum_line_width = usize::from(viewport_width).div_ceil(2);
    let mut candidates = Vec::new();
    for line in &content.lines {
        let line_width = line.width();
        if line_width < minimum_line_width {
            continue;
        }

        let mut colors = Vec::<(Color, usize)>::new();
        for span in &line.spans {
            let effective_style = content.style.patch(line.style).patch(span.style);
            let Some(background) = effective_style.bg.filter(|color| *color != Color::Reset) else {
                continue;
            };
            let width = span.width();
            if let Some((_, count)) = colors.iter_mut().find(|(color, _)| *color == background) {
                *count += width;
            } else {
                colors.push((background, width));
            }
        }
        let Some((background, covered)) = colors.into_iter().max_by_key(|(_, count)| *count) else {
            continue;
        };
        if covered.saturating_mul(4) >= line_width.saturating_mul(3) {
            candidates.push(background);
        }
    }

    if candidates.len() < 2 {
        return None;
    }
    let mut counts = Vec::<(Color, usize)>::new();
    for background in candidates.iter().copied() {
        if let Some((_, count)) = counts.iter_mut().find(|(color, _)| *color == background) {
            *count += 1;
        } else {
            counts.push((background, 1));
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| count.saturating_mul(2) > candidates.len())
        .map(|(background, _)| background)
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
                selected()
            } else {
                accent()
            }),
        ));
        conversation.extend(message.text.lines().map(Line::from));
        conversation.push(Line::from(""));
    }
    if !app.ai_streaming.is_empty() {
        conversation.push(Line::styled("Codex:", Style::default().fg(accent())));
        conversation.extend(app.ai_streaming.lines().map(Line::from));
    }
    let conversation = Paragraph::new(conversation)
        .block(panel("Conversation"))
        .wrap(Wrap { trim: false });
    let conversation_scroll = conversation
        .line_count(chunks[0].width)
        .saturating_sub(usize::from(chunks[0].height))
        .min(usize::from(u16::MAX)) as u16;
    frame.render_widget(conversation.scroll((conversation_scroll, 0)), chunks[0]);

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
        Line::styled("Enter/i prompt   x interrupt", Style::default().fg(muted())),
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
            .style(Style::default().fg(if app.ai_input_mode {
                selected()
            } else {
                muted()
            })),
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
        .map(|entry| activity_line(entry))
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

fn activity_line(entry: &str) -> Line<'_> {
    let mut spans = vec![Span::styled("> ", Style::default().fg(muted()))];
    let (timestamp, message) = entry
        .strip_prefix('[')
        .and_then(|entry| entry.split_once("] "))
        .map_or((None, entry), |(timestamp, message)| {
            (Some(timestamp), message)
        });
    if let Some(timestamp) = timestamp {
        spans.push(Span::styled(
            format!("[{timestamp}] "),
            Style::default().fg(muted()),
        ));
    }

    if let Some((tool_id, stream_and_text)) = message.split_once(" [")
        && let Some((stream, text)) = stream_and_text.split_once("]: ")
    {
        let stream_color = match stream {
            "output" => Color::Green,
            "progress" => Color::Yellow,
            "error" | "err" => Color::Red,
            _ => accent(),
        };
        spans.extend([
            Span::styled(
                tool_id.to_string(),
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[{stream}]"),
                Style::default()
                    .fg(stream_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": "),
            Span::styled(text.to_string(), activity_message_style(text)),
        ]);
        return Line::from(spans);
    }

    if let Some((event, text)) = message.split_once(": ") {
        let event_color = match event {
            "queue" => Color::Cyan,
            "install" => Color::Green,
            "uninstall" | "reinstall" => Color::Yellow,
            "codex" | "codex diagnostic" => Color::Magenta,
            "settings" => Color::Blue,
            "workspace" => accent(),
            _ => Color::White,
        };
        spans.extend([
            Span::styled(
                format!("{event}:"),
                Style::default()
                    .fg(event_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(text.to_string(), activity_message_style(text)),
        ]);
    } else {
        spans.push(Span::styled(
            message.to_string(),
            activity_message_style(message),
        ));
    }
    Line::from(spans)
}

fn activity_message_style(message: &str) -> Style {
    let message = message.to_ascii_lowercase();
    if [
        "failed",
        "failure",
        "error",
        "denied",
        "cancelled",
        "timed out",
        "-> failed",
    ]
    .iter()
    .any(|word| message.contains(word))
    {
        Style::default().fg(Color::Red)
    } else if ["completed", "success", "installed", "ready"]
        .iter()
        .any(|word| message.contains(word))
    {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    }
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
        format!(
            "Mouse controls            {}",
            if app.mouse_enabled { "on" } else { "off" }
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
        format!(
            "AI connection            {} · {}",
            app.ai_provider.label(),
            provider_auth_mode(app).label()
        ),
        format!(
            "AI permission mode       {}",
            app.settings.ai_approval_mode.label()
        ),
        format!("Theme                   {}", app.settings.theme.label()),
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

    let detail = setting_detail(app);
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel("Setting details"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
}

fn setting_detail(app: &AppState) -> Text<'static> {
    let (title, value, description, effect, controls) = match app.settings_index {
        0 => (
            "Mouse controls",
            if app.mouse_enabled { "On" } else { "Off" }.to_string(),
            "Enables clicking, scrolling, and drag-to-copy in T4E panels.",
            "The choice is saved. Alt+M remains available as a global toggle.",
            "Left/Right or Space toggle",
        ),
        1 => (
            "Maximum install attempts",
            app.settings.max_install_attempts.to_string(),
            "Total automatic attempts allowed for one installation.",
            "Adjusts from 1 to 5. Cancelled installs stop immediately instead of retrying.",
            "Left/Right adjusts by one attempt",
        ),
        2 => (
            "Confirm all installs",
            if app.settings.confirm_all_installs {
                "On"
            } else {
                "Off"
            }
            .to_string(),
            "Requests approval before every package-manager installation.",
            "Script and DANGER installs always show a detailed review; Enter approves without retyping commands.",
            "Left/Right or Space toggle",
        ),
        3 => (
            "AI connection",
            format!(
                "{} · {} · {}",
                app.ai_provider.label(),
                provider_auth_mode(app).label(),
                if app.ai_ready() {
                    "ready"
                } else {
                    "not configured"
                }
            ),
            "Select one of Codex, Claude, Gemini, Zhipu AI, Kimi, or Custom for HOME AI.",
            "Enter opens one setup flow for subscription or API-key mode. Session keys are never saved; environment-variable names and profile metadata are saved.",
            "Left/Right provider · Enter configure and use",
        ),
        4 => (
            "AI permission mode",
            app.settings.ai_approval_mode.label().to_string(),
            "Auto immediately runs validated AI actions and asks only at separate high-risk gates. Ask opens Yes/No as soon as an action is proposed. Bypass runs the complete validated action chain without approval input.",
            "Bypass includes install execution, verified updates, default launch options, and sensitive install/device approvals. Required values missing from the request still need input.",
            "Left/Right select permission mode",
        ),
        5 => (
            "Theme",
            app.settings.theme.label().to_string(),
            "Selects the T4E interface palette independently from terminal apps.",
            "Future keeps T4E's cyan phosphor core and adds a Tron-inspired electric-orange selection signal. Amber, Retro Green, and Terracotta provide alternate complete palettes.",
            "Left/Right select theme",
        ),
        _ => (
            "Reset saved preferences",
            "Ready".to_string(),
            "Restores runtime settings and clears remembered launch options.",
            "Favorites, recent apps, activity history, and installed applications are kept.",
            "Enter resets",
        ),
    };
    Text::from(vec![
        Line::styled(title, Style::default().add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::from(vec![
            Span::styled("Current: ", Style::default().fg(accent())),
            Span::raw(value),
        ]),
        Line::from(""),
        Line::from(description),
        Line::from(""),
        Line::from(effect),
        Line::from(""),
        Line::styled(controls, Style::default().fg(muted())),
        Line::styled(
            "Changes are saved automatically.",
            Style::default().fg(muted()),
        ),
    ])
}

fn provider_auth_mode(app: &AppState) -> ProviderAuthMode {
    app.settings
        .api_providers
        .get(app.ai_provider.profile_id())
        .map(|profile| profile.auth_mode)
        .or_else(|| {
            default_api_provider_profiles()
                .get(app.ai_provider.profile_id())
                .map(|profile| profile.auth_mode)
        })
        .unwrap_or(ProviderAuthMode::ApiKey)
}

fn render_api_provider_setup(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(setup) = &app.api_provider_setup else {
        return;
    };
    let compact = area.height < 22 || area.width < 76;
    let popup = centered_rect(92, if compact { 14 } else { 20 }, area);
    frame.render_widget(Clear, popup);
    let key_mask = if setup.api_key.is_empty() {
        "(use environment variable)".to_string()
    } else {
        "•".repeat(setup.api_key.chars().count().min(48))
    };
    let (labels, values): (Vec<&str>, Vec<String>) = match setup.auth_mode {
        ProviderAuthMode::Subscription => (
            vec!["Connection", "Save and use"],
            vec![
                setup.auth_mode.label().to_string(),
                "Press Enter".to_string(),
            ],
        ),
        ProviderAuthMode::ApiKey => (
            vec![
                "Connection",
                "Display name",
                "Base URL",
                "Model",
                "Key environment",
                "Session API key",
                "Save and use",
            ],
            vec![
                setup.auth_mode.label().to_string(),
                setup.label.clone(),
                setup.base_url.clone(),
                setup.model.clone(),
                setup.api_key_env.clone(),
                key_mask,
                "Press Enter".to_string(),
            ],
        ),
    };
    let mut lines = vec![
        Line::styled(
            "Unified AI connection",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Line::from(format!("Provider: {}", setup.provider.label())),
    ];
    let visible_rows = labels
        .iter()
        .zip(values.iter())
        .enumerate()
        .filter(|(index, _)| !compact || *index == setup.field);
    for (index, (label, value)) in visible_rows {
        lines.push(Line::from(vec![
            Span::styled(
                if setup.field == index { "> " } else { "  " },
                Style::default().fg(selected()),
            ),
            Span::styled(format!("{label:<17}"), Style::default().fg(muted())),
            Span::styled(
                value.clone(),
                if setup.field == index {
                    Style::default().fg(selected()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
        ]));
    }
    lines.push(Line::from(""));
    if !compact {
        lines.push(Line::from(match setup.auth_mode {
            ProviderAuthMode::Subscription => {
                "Uses the provider CLI's detected signed-in subscription."
            }
            ProviderAuthMode::ApiKey => {
                "The session API key stays in memory; profile metadata is saved."
            }
        }));
    } else {
        lines.push(Line::from(format!(
            "Field {}/{}",
            setup.field + 1,
            labels.len()
        )));
    }
    lines.push(Line::styled(
        "←/→ mode   Tab/↑/↓ field   Enter next/save   Esc cancel",
        Style::default().fg(muted()),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("AI provider setup"))
            .wrap(Wrap { trim: false }),
        popup,
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
        format!(
            "Search all apps: {}_   Tab panel   Shift+Tab tabs   ↓/→ results   Enter apply   Esc cancel",
            app.search_query
        )
    } else if app.ai_input_mode {
        "AI prompt input   PgUp/PgDn history   wheel scroll   Enter send   Esc cancel".to_string()
    } else if app.screen == Screen::Logs {
        format!(
            "{} | Shift+Tab tabs | Up/Down 1 row  PageUp/PageDown 10  Home/End  c clear",
            app.status
        )
    } else if app.screen == Screen::Home {
        let navigation = match app.home_focus {
            HomeFocus::Views if app.home_filter_index == 0 => {
                "↑ search  ↓ view  → apps  Ctrl+F search"
            }
            HomeFocus::Views => "↑/↓ view  → apps  Ctrl+F search",
            HomeFocus::AppList => "← views  ↑/↓ app  Enter run",
            HomeFocus::Assistant => "← apps  type to compose  ↑/↓ history  wheel scroll  / skill",
        };
        if area.width < 90 {
            "Tab panels  S-Tab tabs  ←/→ focus  Backspace back  ? help".to_string()
        } else {
            format!(
                "{} | Tab panels · Shift+Tab tabs | {navigation}  Backspace back  ? help",
                app.status
            )
        }
    } else {
        format!(
            "{} | Shift+Tab tabs | ↑/↓ or j/k move  Enter open/run  Backspace back  ? help",
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
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Line::from("SAFE none | LOW network, account, or file read"),
            Line::from("HIGH camera capture, file write, or delete"),
            Line::from("DANGER system, commands, or autonomous operation"),
            Line::from("Highest capability level becomes the app risk level"),
            Line::styled(
                "App details list every declared capability.",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Line::from("AI permission: Bypass / Auto / Ask in Settings"),
            Line::from("Enter run | I install | U uninstall | R reinstall"),
            Line::from("F1 Help | Ctrl+F HOME search | Tab panels | Shift+Tab tabs"),
            Line::from("Activity arrows/PgUp/PgDn | Alt+Q close | Alt+BS background"),
        ]
    } else {
        vec![
            Line::styled(
                "Capabilities and derived risk",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
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
                Style::default().fg(muted()),
            ),
            Line::styled(
                "Installation policy",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Line::from(
                "Package-manager installs use a generated catalog plan and verify required executables afterward.",
            ),
            Line::styled(
                "Script installers require approval for manual and Auto actions; AI Bypass explicitly skips it.",
                Style::default().fg(muted()),
            ),
            Line::from(""),
            Line::styled(
                "Using T4E",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Line::from("arrows / j k       move selection"),
            Line::from("Enter               enter an app list or run the selected app"),
            Line::from("I / U / R           install / uninstall / reset and reinstall"),
            Line::from("Tab                 switch HOME panels"),
            Line::from("Shift+Tab           cycle HOME / Activity / Settings / Help tabs"),
            Line::from("F1                  open Help from dashboard screens"),
            Line::from("Ctrl+F              search apps from HOME Views or Apps"),
            Line::from("/                   start an Assistant skill or command"),
            Line::from("AI permission       Settings: Bypass / Auto / Ask"),
            Line::from("Activity arrows     scroll one row; PageUp / PageDown scroll ten"),
            Line::from("Activity Home / End jump to newest / oldest entry"),
            Line::from("Alt+Left / Right    switch running apps"),
            Line::from("Alt+Backspace       leave an app running in the background"),
            Line::from("Alt+Q               close the current app; quit from HOME"),
            Line::from("Mouse drag         copy text inside one panel without its border"),
            Line::from("Alt+M               disable / enable T4E mouse controls"),
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
    let tool = app
        .catalog
        .tools
        .iter()
        .find(|tool| tool.id == confirmation.tool_id);
    let risk = tool.map_or("UNKNOWN", |tool| tool.risk_level().label());
    let capabilities = tool.map_or_else(
        || "UNKNOWN".to_string(),
        |tool| {
            if tool.capabilities.is_empty() {
                "NONE".to_string()
            } else {
                tool.capabilities
                    .iter()
                    .map(|capability| capability.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        },
    );
    let lines = vec![
        Line::styled(
            "Explicit installation approval required",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(format!("tool: {}", confirmation.tool_id)),
        Line::from(format!("risk: {risk}")),
        Line::from(format!("capabilities: {capabilities}")),
        Line::from(format!("command: {}", confirmation.command)),
        Line::from(""),
        Line::from("Review the details above. T4E never asks you to retype the command."),
        Line::from(""),
        Line::styled("Enter confirm   Esc cancel", Style::default().fg(muted())),
    ];
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
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
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
            Style::default().fg(muted()),
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
        Line::styled(&state.placeholder, Style::default().fg(muted()))
    } else {
        Line::styled(format!("> {}_", state.input), selection_style())
    };
    let content = Text::from(vec![
        Line::styled(
            &state.tool_name,
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(state.label.as_str()),
        value,
        Line::from(""),
        Line::styled("Enter launch   Esc cancel", Style::default().fg(muted())),
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
        Line::styled("Enter allow   Esc cancel", Style::default().fg(muted())),
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
            .style(Style::default().fg(muted())),
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
        Line::styled(action, Style::default().fg(muted())),
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
    let popup = centered_rect(72, 12, area);
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
        Line::from("Allow T4E to perform this catalog-bounded action?"),
        Line::from(""),
        Line::styled(
            "Y / Enter  Yes     N / Esc  No",
            Style::default().fg(selected()).add_modifier(Modifier::BOLD),
        ),
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
    let palette = active_palette();
    Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .style(Style::default().bg(palette.surface).fg(palette.foreground))
        .border_style(Style::default().fg(palette.border))
}

fn tab_style() -> Style {
    let palette = active_palette();
    Style::default()
        .bg(palette.surface)
        .fg(palette.tab_foreground)
}

fn tab_border_style() -> Style {
    Style::default().fg(active_palette().border)
}

fn selected_tab_style() -> Style {
    let palette = active_palette();
    Style::default()
        .bg(palette.selected)
        .fg(palette.background)
        .add_modifier(Modifier::BOLD)
}

fn selection_style() -> Style {
    Style::default().fg(selected()).add_modifier(Modifier::BOLD)
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
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Text};
    use ratatui::widgets::{Block, Borders, Widget};

    use super::{
        activity_line, app_canvas_background, compact_usage, home_layout, home_selection_style,
        home_selection_symbol, selection_text,
    };

    #[test]
    fn desktop_home_gives_information_more_width_without_squeezing_apps() {
        let [quick_access, apps, information] = home_layout(Rect::new(0, 0, 160, 30));

        assert_eq!(quick_access.width, 24);
        assert_eq!(information.width, 56);
        assert!(apps.width >= 40);
    }

    #[test]
    fn usage_summary_keeps_only_limit_name_and_percent() {
        let raw = r#"{"rateLimits":{"limitName":"Codex Plan","primary":{"usedPercent":12.4}}}"#;
        assert_eq!(compact_usage(raw), "Codex Plan: 12% used");
    }

    #[test]
    fn activity_highlighting_separates_timestamp_stream_event_and_failure() {
        let output =
            activity_line("[2026-07-26 12:34:56 +09:00] yazi [progress]: Compiling dependency");
        assert_eq!(output.spans[1].style.fg, Some(Color::Rgb(166, 180, 183)));
        assert_eq!(output.spans[2].content, "yazi");
        assert_eq!(output.spans[2].style.fg, Some(Color::Rgb(93, 225, 242)));
        assert_eq!(output.spans[4].content, "[progress]");
        assert_eq!(output.spans[4].style.fg, Some(Color::Yellow));

        let failure = activity_line("[2026-07-26 12:34:56 +09:00] install: failed youtube-tui");
        assert_eq!(failure.spans[2].content, "install:");
        assert_eq!(failure.spans[2].style.fg, Some(Color::Green));
        assert_eq!(failure.spans[4].style.fg, Some(Color::Red));
    }

    #[test]
    fn inactive_home_selection_has_no_arrow_or_reversed_background() {
        assert_eq!(home_selection_symbol(false), "  ");
        assert_eq!(
            home_selection_style(false),
            Style::default().add_modifier(Modifier::BOLD)
        );
        assert_ne!(home_selection_style(false).bg, Some(Color::White));
    }

    #[test]
    fn app_canvas_uses_a_repeated_full_width_background() {
        let content = Text::from(vec![
            Line::styled(" ".repeat(40), Style::default().bg(Color::White)),
            Line::styled(" ".repeat(40), Style::default().bg(Color::White)),
        ]);

        assert_eq!(app_canvas_background(&content, 40), Some(Color::White));
    }

    #[test]
    fn app_canvas_ignores_short_or_isolated_background_accents() {
        let content = Text::from(vec![
            Line::styled(" ERROR ", Style::default().bg(Color::Red)),
            Line::from("ordinary application output that fills the remaining line"),
        ]);

        assert_eq!(app_canvas_background(&content, 60), None);
    }

    #[test]
    fn app_canvas_ignores_conflicting_full_width_backgrounds() {
        let content = Text::from(vec![
            Line::styled(" ".repeat(40), Style::default().bg(Color::Red)),
            Line::styled(" ".repeat(40), Style::default().bg(Color::Blue)),
        ]);

        assert_eq!(app_canvas_background(&content, 40), None);
    }

    #[test]
    fn panel_selection_excludes_outline_and_rejoins_a_wrapped_url() {
        let area = Rect::new(0, 0, 24, 5);
        let mut buffer = Buffer::empty(area);
        Block::default()
            .borders(Borders::ALL)
            .render(area, &mut buffer);
        buffer.set_string(1, 1, "https://example.com/a", Style::default());
        buffer.set_string(1, 2, "?token=xyz", Style::default());

        let copied = selection_text(&buffer, (1, 1), (10, 2)).expect("selection copies");

        assert_eq!(copied, "https://example.com/a?token=xyz");
        assert!(!copied.contains(['┌', '┐', '└', '┘', '│']));
        assert!(selection_text(&buffer, (0, 0), (10, 2)).is_none());
    }

    #[test]
    fn panel_selection_cannot_merge_adjacent_panel_borders() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 5));
        let left = Rect::new(0, 0, 10, 5);
        let right = Rect::new(10, 0, 10, 5);
        Block::default()
            .borders(Borders::ALL)
            .render(left, &mut buffer);
        Block::default()
            .borders(Borders::ALL)
            .render(right, &mut buffer);
        buffer.set_string(1, 1, "LEFT", Style::default());
        buffer.set_string(11, 1, "RIGHT", Style::default());

        let copied = selection_text(&buffer, (1, 1), (18, 1)).expect("left panel selection");

        assert_eq!(copied, "LEFT");
        assert!(!copied.contains("RIGHT"));
    }

    #[test]
    fn panel_selection_does_not_insert_spaces_for_wide_korean_cells() {
        let area = Rect::new(0, 0, 32, 4);
        let mut buffer = Buffer::empty(area);
        Block::default()
            .borders(Borders::ALL)
            .render(area, &mut buffer);
        let message = "현재 T4E 파이프를 실행합니다";
        buffer.set_string(1, 1, message, Style::default());

        let copied = selection_text(&buffer, (1, 1), (30, 1)).expect("selection copies");

        assert_eq!(copied, message);
    }
}
