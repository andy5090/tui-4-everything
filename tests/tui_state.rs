use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use t4e::app::events::Screen;
use t4e::app::state::{AppEffect, AppState};
use t4e::app::ui::render;
use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::models::Platform;
use t4e::codex::service::CodexEvent;
use t4e::installer::checks::CheckResult;
use t4e::installer::engine::{InstallPolicy, build_install_task};
use t4e::installer::execution::InstallJob;
use t4e::installer::queue::QueueState;

fn app() -> AppState {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces load");
    AppState::new(catalog, workspaces)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn navigation_keeps_screen_specific_selection() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.screen, Screen::Catalog);
    assert_eq!(app.catalog_index, 1);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.screen, Screen::Install);
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Catalog);
    assert_eq!(app.catalog_index, 1);
}

#[test]
fn catalog_search_filters_and_queues_a_tool_once() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ripgrep".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let visible = app.visible_catalog_tools();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, "ripgrep");

    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.queue.len(), 1);
    assert_eq!(app.queue[0].item.tool_id, "ripgrep");
}

#[test]
fn workspace_action_emits_a_bounded_launch_request() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('4')));
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::LaunchWorkspace(request)) = app.take_effect() else {
        panic!("workspace launch request expected");
    };
    assert_eq!(request.workspace.id, "video-desk");
    assert_eq!(
        request.required_tools,
        [
            ("yewtube".to_string(), "yewtube".to_string()),
            ("mpv".to_string(), "mpv".to_string()),
            ("yazi".to_string(), "yazi".to_string()),
        ]
    );
}

#[test]
fn q_returns_home_then_quits() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('5')));
    app.handle_key(key(KeyCode::Char('q')));
    assert_eq!(app.screen, Screen::Home);
    assert!(!app.should_quit);

    app.handle_key(key(KeyCode::Char('q')));
    assert!(app.should_quit);
}

#[test]
fn every_screen_renders_at_supported_terminal_sizes() {
    let sizes = [(60, 16), (70, 20), (80, 24), (120, 35)];

    for (width, height) in sizes {
        for screen_key in ['1', '2', '3', '4', '5', '6', '7'] {
            let mut app = app();
            app.handle_key(key(KeyCode::Char(screen_key)));
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("test terminal");

            terminal
                .draw(|frame| render(frame, &mut app))
                .expect("screen renders");

            assert!(
                terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .any(|cell| cell.symbol() == "t"),
                "expected nonblank render for screen {screen_key} at {width}x{height}"
            );
        }
    }
}

#[test]
fn narrow_layout_keeps_every_screen_target_and_back_key_visible() {
    let mut app = app();
    let backend = TestBackend::new(60, 16);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("home renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("7 Set"));
    assert!(rendered.contains("q back"));
}

#[test]
fn home_pack_can_filter_catalog_and_queue_the_pack() {
    let mut app = app();
    let first_pack_ids = app.catalog.packs[0].tool_ids.clone();

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Catalog);
    assert_eq!(app.active_pack.as_deref(), Some("music-pack"));
    assert!(
        app.visible_catalog_tools()
            .iter()
            .all(|tool| first_pack_ids.contains(&tool.id))
    );

    app.handle_key(key(KeyCode::Char('1')));
    app.handle_key(key(KeyCode::Char('I')));
    assert!(!app.queue.is_empty());
    assert!(
        app.queue
            .iter()
            .all(|job| first_pack_ids.contains(&job.item.tool_id))
    );
}

#[test]
fn global_catalog_entry_clears_a_transient_pack_filter() {
    let mut app = app();
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.active_pack.as_deref(), Some("music-pack"));
    app.search_query = "stale-search".to_string();

    app.handle_key(key(KeyCode::Char('q')));
    app.handle_key(key(KeyCode::Char('2')));

    assert_eq!(app.screen, Screen::Catalog);
    assert!(app.active_pack.is_none());
    assert!(app.search_query.is_empty());
    assert_eq!(app.status, "Showing all catalog tools");
}

#[test]
fn preflight_success_reports_that_tool_was_already_installed() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ripgrep".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    let mut completed = app.queue[0].clone();
    completed
        .item
        .transition(QueueState::Installing)
        .expect("starts");
    completed
        .item
        .transition(QueueState::Success)
        .expect("succeeds");
    completed.preflight = Some(CheckResult {
        command: "rg".to_string(),
        installed: true,
        resolved_path: Some("/usr/bin/rg".to_string()),
    });

    app.apply_execution(completed);
    assert_eq!(app.status, "ripgrep is already installed and ready");
    assert!(
        app.logs
            .iter()
            .any(|line| line == "install: ripgrep already installed")
    );
}

#[test]
fn catalog_multi_select_queues_once_and_tracks_favorites() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    let first_id = app.selected_catalog_tool().expect("first tool").id.clone();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('f')));
    app.handle_key(key(KeyCode::Down));
    let second_id = app.selected_catalog_tool().expect("second tool").id.clone();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('I')));

    assert!(app.favorites.contains(&first_id));
    assert!(app.selected_tools.is_empty());
    assert!(app.queue.iter().any(|job| job.item.tool_id == first_id));
    assert!(app.queue.iter().any(|job| job.item.tool_id == second_id));
}

#[test]
fn settings_controls_update_execution_policy() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('7')));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.default_mux, "zellij");
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.install_timeout_sec, 660);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.max_install_attempts, 3);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char(' ')));
    assert!(app.settings.confirm_all_installs);
}

#[test]
fn queue_run_schedules_pack_jobs_sequentially() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('I')));
    assert!(app.queue.len() > 1);
    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('X')));

    let Some(AppEffect::Execute(first)) = app.take_effect() else {
        panic!("first queued job should execute");
    };
    assert!(app.take_effect().is_none());
    let first_id = first.item.tool_id.clone();
    let mut completed = *first;
    completed
        .item
        .transition(t4e::installer::queue::QueueState::Installing)
        .expect("starts");
    completed
        .item
        .transition(t4e::installer::queue::QueueState::Success)
        .expect("succeeds");
    app.apply_execution(completed);

    let Some(AppEffect::Execute(second)) = app.take_effect() else {
        panic!("second queued job should execute after completion");
    };
    assert_ne!(second.item.tool_id, first_id);
    assert!(app.take_effect().is_none());
}

#[test]
fn ai_home_composes_prompts_and_applies_stream_events() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('5')));
    app.handle_key(key(KeyCode::Enter));
    for ch in "show workspaces".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::CodexPrompt(prompt)) = app.take_effect() else {
        panic!("Codex prompt expected");
    };
    assert_eq!(prompt, "show workspaces");
    app.apply_codex_event(CodexEvent::Ready {
        account: "chatgpt".to_string(),
    });
    app.apply_codex_event(CodexEvent::TurnStarted("turn_123".to_string()));
    app.apply_codex_event(CodexEvent::Delta("Available ".to_string()));
    app.apply_codex_event(CodexEvent::Delta("workspaces".to_string()));
    assert_eq!(app.ai_streaming, "Available workspaces");
    app.apply_codex_event(CodexEvent::Message("Available workspaces".to_string()));
    app.apply_codex_event(CodexEvent::TurnCompleted("completed".to_string()));

    assert_eq!(app.ai_account, "chatgpt");
    assert!(app.ai_streaming.is_empty());
    assert_eq!(app.ai_messages.last().expect("AI message").role, "Codex");
    assert!(app.ai_status.contains("completed"));
}

#[test]
fn codex_stderr_diagnostic_does_not_replace_working_status() {
    let mut app = app();
    app.apply_codex_event(CodexEvent::TurnStarted("turn_123".to_string()));
    let working = app.ai_status.clone();
    app.apply_codex_event(CodexEvent::Diagnostic(
        "models cache fallback warning".to_string(),
    ));

    assert_eq!(app.ai_status, working);
    assert!(
        app.logs
            .iter()
            .any(|line| line.contains("models cache fallback warning"))
    );
}

#[test]
fn ai_workspace_action_requires_typed_approval_before_launch_effect() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('5')));
    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "workspace_launch".to_string(),
        target: "video-desk".to_string(),
    });
    assert!(app.pending_ai_action.is_some());
    assert!(app.take_effect().is_none());
    app.apply_codex_event(CodexEvent::TurnCompleted("completed".to_string()));
    assert!(app.ai_status.contains("Approval required"));

    app.handle_key(key(KeyCode::Char('A')));
    assert!(app.ai_confirmation.is_some());
    for ch in "APPROVE workspace_launch video-desk".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.ai_confirmation.is_none());
    assert_eq!(app.screen, Screen::Workspace);
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchWorkspace(_))
    ));
}

#[test]
fn ai_catalog_search_is_bounded_to_local_navigation() {
    let mut app = app();
    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "catalog_search".to_string(),
        target: "music".to_string(),
    });

    assert_eq!(app.screen, Screen::Catalog);
    assert_eq!(app.search_query, "music");
    assert!(app.take_effect().is_none());
}

#[test]
fn queued_safe_tool_emits_execution_effect_only_after_explicit_action() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ripgrep".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.take_effect().is_none());

    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(matches!(app.take_effect(), Some(AppEffect::Execute(_))));
}

#[test]
fn high_risk_tool_requires_typed_confirmation() {
    let mut app = app();
    let platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    };
    let tool = app
        .catalog
        .tools
        .iter()
        .find(|tool| tool.id == "claude-code")
        .expect("agent exists");
    let installer = tool
        .installers
        .iter()
        .find(|installer| installer.platform == platform)
        .expect("platform installer");
    let task = build_install_task(tool, installer, &InstallPolicy::default()).expect("task builds");
    app.queue.push(InstallJob::new(task, "agent-test"));
    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.confirmation.is_some());
    assert!(app.take_effect().is_none());

    for ch in "INSTALL claude-code".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.confirmation.is_none());
    assert!(matches!(app.take_effect(), Some(AppEffect::Execute(_))));
}

#[test]
fn stale_persisted_command_is_not_executed() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Enter));
    app.queue[0].task.command = "echo tampered".to_string();
    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('x')));

    assert!(app.status.contains("stale"));
    assert!(app.take_effect().is_none());
}
