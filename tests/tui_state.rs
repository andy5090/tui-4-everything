use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use t4e::ai::service::{AiEvent, AiProvider, ProviderReadiness};
use t4e::app::events::Screen;
use t4e::app::state::{AiMessage, AiWorkflowPhase, AppEffect, AppState, HomeFilter, HomeFocus};
use t4e::app::ui::render;
use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::models::{AppCategory, InstallMethod, KeyHint, OutputFilter, Platform};
use t4e::codex::service::CodexEvent;
use t4e::installer::checks::CheckResult;
use t4e::installer::engine::{InstallPolicy, build_install_task};
use t4e::installer::environment::InstallEnvironment;
use t4e::installer::execution::{InstallAttempt, InstallJob};
use t4e::installer::queue::QueueState;
use t4e::mux::runtime::ManagedApp;
use t4e::storage::{AiApprovalMode, AppTheme, PersistentState, UserSettings, save_state};

fn app() -> AppState {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces load");
    AppState::new_with_install_environment(
        catalog,
        workspaces,
        InstallEnvironment::with_commands(
            Platform::current(),
            "x86_64",
            [
                "awk",
                "apt-get",
                "brew",
                "cargo",
                "curl",
                "dnf",
                "go",
                "ldd",
                "npm",
                "pacman",
                "pipx",
                "pkg",
                "sha256sum",
                "snap",
                "tar",
                "uname",
                "xbps-install",
            ],
        ),
    )
}

fn temp_state_file() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("t4e-tui-state-{}-{nonce}", std::process::id()))
        .join("state.json")
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn open_catalog_search(app: &mut AppState, query: &str) {
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in query.chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
}

fn open_home_search(app: &mut AppState) {
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
}

#[test]
fn shift_tab_switches_header_sections_outside_running_apps() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.screen, Screen::Catalog);
    assert_eq!(app.catalog_index, 1);

    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Logs);
    assert_eq!(app.catalog_index, 1);

    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Settings);
}

#[test]
fn agents_conversation_keeps_the_latest_message_visible() {
    let mut app = app();
    app.screen = Screen::Agents;
    for index in 0..30 {
        app.ai_messages.push(AiMessage {
            role: "Codex".to_string(),
            text: format!("history message {index}"),
        });
    }
    app.ai_messages.push(AiMessage {
        role: "Codex".to_string(),
        text: "LATEST CONVERSATION MESSAGE".to_string(),
    });

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("conversation renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("LATEST CONVERSATION MESSAGE"));
}

#[test]
fn tab_switches_home_panels_and_shift_tab_cycles_dashboard_tabs() {
    let mut app = app();
    open_home_search(&mut app);
    assert!(app.search_mode);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.home_focus, HomeFocus::AppList);
    assert!(!app.search_mode);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.home_focus, HomeFocus::Assistant);

    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Logs);
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Settings);
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

    app.handle_key(key(KeyCode::Char('I')));
    app.handle_key(key(KeyCode::Char('I')));
    assert_eq!(app.queue.len(), 1);
    assert_eq!(app.queue[0].item.tool_id, "ripgrep");
}

#[test]
fn catalog_row_places_app_name_before_risk_column() {
    let mut app = app();
    open_catalog_search(&mut app, "cmatrix");
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("catalog renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    let name_index = rendered.find("cmatrix").expect("app name is rendered");
    let risk_index = rendered.find("SAFE").expect("risk is rendered");
    assert!(name_index < risk_index);
}

#[test]
fn catalog_detail_previews_app_keys_and_explains_risk() {
    let mut app = app();
    open_catalog_search(&mut app, "cmatrix");
    let backend = TestBackend::new(140, 35);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("catalog renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("risk: SAFE (app-owned config, cache, and UI state only)"));
    assert!(rendered.contains("App key guide"));
    assert!(rendered.contains("toggle asynchronous scrolling"));
    assert!(rendered.contains("random bold / all bold / bold off"));
    assert!(rendered.contains("input check: No documented conflicts with T4E"));
    assert!(rendered.contains("T4E controls: Enter run | I install | U uninstall | R reinstall"));
}

#[test]
fn catalog_enter_launches_and_missing_app_auto_launches_after_install() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ripgrep".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::LaunchTool(request)) = app.take_effect() else {
        panic!("app launch request expected");
    };
    assert_eq!(request.tool_id, "ripgrep");
    assert_eq!(request.command, "rg");

    app.install_then_launch(request);
    let Some(AppEffect::Execute(job)) = app.take_effect() else {
        panic!("missing app should install immediately");
    };
    let mut completed = *job;
    completed
        .item
        .transition(QueueState::Installing)
        .expect("starts");
    completed
        .item
        .transition(QueueState::Success)
        .expect("succeeds");
    app.apply_execution(completed);
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request)) if request.tool_id == "ripgrep"
    ));
}

#[test]
fn catalog_builds_launch_command_from_allowlisted_options() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "cmatrix".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.launch_options.is_some());
    assert!(app.take_effect().is_none());

    app.handle_key(key(KeyCode::Char(' ')));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));

    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request))
            if request.tool_id == "cmatrix" && request.command == "cmatrix -b -u 6"
    ));
    assert_eq!(
        app.launch_preferences["cmatrix"]["update-delay"]
            .value
            .as_deref(),
        Some("6")
    );
    assert!(app.launch_preferences["cmatrix"]["update-delay"].enabled);
}

#[test]
fn missing_app_finishes_installation_before_showing_launch_options() {
    let mut app = app();
    app.apply_installed_tools(Default::default());
    open_catalog_search(&mut app, "cmatrix");
    app.handle_key(key(KeyCode::Enter));

    assert!(app.launch_options.is_none());
    let Some(AppEffect::Execute(job)) = app.take_effect() else {
        panic!("missing app should begin installation");
    };
    let mut completed = *job;
    completed
        .item
        .transition(QueueState::Installing)
        .expect("install starts");
    completed
        .item
        .transition(QueueState::Success)
        .expect("install succeeds");
    app.apply_execution(completed);

    assert!(app.launch_options.is_some());
    assert!(app.launch_argument.is_none());
    assert!(app.take_effect().is_none());
}

#[test]
fn failed_installation_does_not_open_launch_options() {
    let mut app = app();
    app.apply_installed_tools(Default::default());
    open_catalog_search(&mut app, "cmatrix");
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::Execute(job)) = app.take_effect() else {
        panic!("missing app should begin installation");
    };
    let mut completed = *job;
    completed
        .item
        .transition(QueueState::Installing)
        .expect("install starts");
    completed
        .item
        .transition(QueueState::Failed)
        .expect("install fails");
    app.apply_execution(completed);

    assert!(app.launch_options.is_none());
    assert!(app.launch_argument.is_none());
    assert!(app.take_effect().is_none());
}

#[test]
fn launch_options_survive_state_reload_and_ignore_option_order() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces load");
    let path = temp_state_file();
    let mut app =
        AppState::persistent(catalog, workspaces, path.clone()).expect("persistent app starts");

    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "cmatrix".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));
    app.persist().expect("launch preferences save");

    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog reloads");
    let workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces reload");
    let mut restored =
        AppState::persistent(catalog, workspaces, path.clone()).expect("state reloads");
    restored.handle_key(key(KeyCode::Char('2')));
    restored.handle_key(key(KeyCode::Char('/')));
    for ch in "cmatrix".chars() {
        restored.handle_key(key(KeyCode::Char(ch)));
    }
    restored.handle_key(key(KeyCode::Enter));
    restored.handle_key(key(KeyCode::Enter));

    let options = restored.launch_options.expect("launch options reopen");
    assert!(options.selections[3].enabled);
    assert_eq!(options.selections[3].value_index, 3);
    let _ = fs::remove_dir_all(path.parent().expect("state parent"));
}

#[test]
fn removed_saved_option_value_falls_back_to_catalog_default() {
    let mut app = app();
    app.launch_preferences.insert(
        "cmatrix".to_string(),
        [(
            "update-delay".to_string(),
            t4e::storage::LaunchOptionPreference {
                enabled: true,
                value: Some("removed-value".to_string()),
            },
        )]
        .into(),
    );
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "cmatrix".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    let options = app.launch_options.expect("launch options open");
    assert!(options.selections[3].enabled);
    assert_eq!(options.selections[3].value_index, 2);
}

#[test]
fn youtube_apps_pass_the_selected_external_video_renderer_to_managed_launchers() {
    for (tool_id, renderer_steps, expected_command) in [
        ("yewtube", 1, "t4e-yewtube --renderer TCT"),
        ("youtube-tui", 2, "t4e-youtube-tui-v2 --renderer CACA"),
    ] {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('2')));
        app.handle_key(key(KeyCode::Char('/')));
        for ch in tool_id.chars() {
            app.handle_key(key(KeyCode::Char(ch)));
        }
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Enter));
        for _ in 0..renderer_steps {
            app.handle_key(key(KeyCode::Right));
        }
        app.handle_key(key(KeyCode::Enter));

        assert!(matches!(
            app.take_effect(),
            Some(AppEffect::LaunchTool(request))
                if request.tool_id == tool_id && request.command == expected_command
        ));
    }
}

#[test]
fn one_shot_fun_app_requests_a_persistent_output_pane() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "fortune".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request))
            if request.tool_id == "fortune"
                && request.command == "fortune"
                && request.keep_open
                && request.output_filter.is_none()
    ));
}

#[test]
fn compatible_app_can_enable_lolcat_as_a_structured_output_filter() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "fortune".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Enter));

    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request))
            if request.tool_id == "fortune"
                && request.command == "fortune"
                && request.output_filter == Some(OutputFilter::Lolcat)
    ));
}

#[test]
fn figlet_collects_text_then_applies_options_before_the_quoted_message() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "figlet".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    assert!(app.launch_argument.is_some());
    for ch in "T4E's shell".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.launch_options.is_some());
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Enter));

    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request))
            if request.tool_id == "figlet"
                && request.command == "figlet -f small -c 'T4E'\"'\"'s shell'"
                && request.keep_open
                && request.output_filter == Some(OutputFilter::Lolcat)
    ));
}

#[test]
fn output_filter_install_resumes_the_original_app_launch() {
    let mut app = app();
    let request = t4e::app::state::ToolLaunchRequest {
        tool_id: "fortune".to_string(),
        command: "fortune".to_string(),
        keep_open: true,
        output_filter: Some(OutputFilter::Lolcat),
    };

    app.install_tool_then_launch("lolcat".to_string(), request);
    let Some(AppEffect::Execute(job)) = app.take_effect() else {
        panic!("lolcat dependency should install immediately");
    };
    assert_eq!(job.item.tool_id, "lolcat");
    let mut completed = *job;
    completed
        .item
        .transition(QueueState::Installing)
        .expect("starts");
    completed
        .item
        .transition(QueueState::Success)
        .expect("succeeds");
    app.apply_execution(completed);

    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request))
            if request.tool_id == "fortune"
                && request.output_filter == Some(OutputFilter::Lolcat)
    ));
}

#[test]
fn required_launch_argument_is_prompted_and_shell_quoted() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "tplay".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    assert!(app.launch_argument.is_some());
    assert!(app.take_effect().is_none());

    let media = "https://example.com/watch?q=a'b;echo unsafe";
    for ch in media.chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::LaunchTool(request)) = app.take_effect() else {
        panic!("launch request expected");
    };
    assert_eq!(request.tool_id, "tplay");
    assert!(
        request
            .command
            .ends_with("'https://example.com/watch?q=a'\"'\"'b;echo unsafe'")
    );
}

#[test]
fn tplay_uninstall_removes_managed_runtime_and_cargo_binary() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "tplay".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.installed_tools.insert("tplay".to_string());

    app.handle_key(key(KeyCode::Char('U')));
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::Uninstall(request)) = app.take_effect() else {
        panic!("uninstall request expected");
    };
    assert_eq!(
        request.method,
        if cfg!(target_os = "macos") {
            InstallMethod::Cargo
        } else {
            InstallMethod::Tplay
        }
    );
    if !cfg!(target_os = "macos") {
        assert!(request.command.contains("t4e-tplay"));
        assert!(request.command.contains("/t4e/tplay"));
    }
    assert!(request.command.contains("cargo uninstall"));
    assert!(request.command.contains("tplay"));
    assert_eq!(
        request.check_command,
        if cfg!(target_os = "macos") {
            "tplay"
        } else {
            "t4e-tplay"
        }
    );
}

#[test]
fn newsboat_uninstall_removes_managed_feeds_and_snap() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "newsboat".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.installed_tools.insert("newsboat".to_string());

    app.handle_key(key(KeyCode::Char('U')));
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::Uninstall(request)) = app.take_effect() else {
        panic!("uninstall request expected");
    };
    assert_eq!(request.method, InstallMethod::Newsboat);
    assert!(request.command.contains("t4e-newsboat"));
    assert!(request.command.contains("snap/newsboat/common/t4e"));
    assert!(request.command.contains("snap remove newsboat"));
    assert_eq!(request.check_command, "t4e-newsboat");
}

#[test]
fn reset_reinstall_recovers_a_partial_termusic_install_and_stale_queue_item() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "termusic".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('I')));
    assert_eq!(app.queue.len(), 1);

    app.handle_key(key(KeyCode::Char('R')));
    let confirmation = app
        .uninstall_confirmation
        .as_ref()
        .expect("reinstall confirmation opens");
    assert!(confirmation.reinstall);
    if cfg!(target_os = "macos") {
        assert!(confirmation.command.contains("brew uninstall termusic"));
    } else {
        assert!(
            confirmation
                .command
                .contains("cargo uninstall \"$package\"")
        );
        assert!(confirmation.command.contains("termusic termusic-server"));
    }

    app.handle_key(key(KeyCode::Enter));
    let Some(AppEffect::Uninstall(request)) = app.take_effect() else {
        panic!("reset uninstall request expected");
    };
    assert!(request.reinstall);

    app.apply_uninstall_result("termusic", true, "", true);

    assert_eq!(app.queue.len(), 1);
    assert_eq!(app.queue[0].item.tool_id, "termusic");
    assert_eq!(app.queue[0].item.state, QueueState::Queued);
    assert_eq!(
        app.queue[0].task.additional_check_commands,
        ["termusic-server"]
    );
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::Execute(job)) if job.item.tool_id == "termusic"
    ));
}

#[test]
fn reset_reinstall_tolerates_missing_packages_across_install_channels() {
    let cases = if cfg!(target_os = "macos") {
        [
            ("lynx", "brew list"),
            ("asciiquarium", "brew list"),
            ("yewtube", "brew list"),
            ("youtube-tui", "cargo uninstall"),
        ]
    } else {
        [
            ("lynx", "dpkg-query"),
            ("asciiquarium", "snap list"),
            ("yewtube", "pipx list --short"),
            ("youtube-tui", "cargo uninstall"),
        ]
    };
    for (tool_id, installed_probe) in cases {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('2')));
        app.handle_key(key(KeyCode::Char('/')));
        for ch in tool_id.chars() {
            app.handle_key(key(KeyCode::Char(ch)));
        }
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Char('R')));

        let request = app
            .uninstall_confirmation
            .as_ref()
            .unwrap_or_else(|| panic!("{tool_id} supports reset and reinstall"));
        assert!(request.reinstall);
        assert!(
            request.command.contains(installed_probe),
            "{tool_id} reset command should probe its install channel: {}",
            request.command
        );
    }
}

#[test]
fn installed_app_can_emit_a_confirmed_package_manager_uninstall() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ripgrep".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.installed_tools.insert("ripgrep".to_string());

    app.handle_key(key(KeyCode::Char('U')));
    assert!(app.uninstall_confirmation.is_some());
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::Uninstall(request)) = app.take_effect() else {
        panic!("uninstall request expected");
    };
    assert_eq!(request.tool_id, "ripgrep");
    if cfg!(target_os = "macos") {
        assert_eq!(request.command, "brew uninstall ripgrep");
    } else {
        assert_eq!(
            request.command,
            "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 remove -y ripgrep"
        );
    }
    assert_eq!(request.check_command, "rg");
    app.mark_uninstall_started("ripgrep");
    app.apply_uninstall_result("ripgrep", true, "", false);
    assert!(!app.installed_tools.contains("ripgrep"));
    assert!(!app.uninstalling_tools.contains("ripgrep"));
}

#[test]
fn lazyvim_uninstall_only_removes_the_t4e_profile() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "lazyvim".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.installed_tools.insert("lazyvim".to_string());

    app.handle_key(key(KeyCode::Char('U')));
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::Uninstall(request)) = app.take_effect() else {
        panic!("uninstall request expected");
    };
    assert_eq!(request.tool_id, "lazyvim");
    assert_eq!(request.method, InstallMethod::LazyVim);
    assert!(request.command.contains("t4e-lazyvim"));
    assert!(!request.command.contains("/.config/nvim"));
    assert!(!request.command.contains("snap remove"));
    assert_eq!(request.check_command, "t4e-lazyvim");
}

#[test]
fn legacy_workspace_templates_are_not_a_primary_menu_destination() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('4')));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.home_focus, HomeFocus::Assistant);
    assert!(app.take_effect().is_none());
}

#[test]
fn app_view_switches_closes_and_forwards_keys_without_tmux_shortcuts() {
    let mut app = app();
    app.open_app_view(
        "t4e-video".to_string(),
        vec![
            ManagedApp {
                pane_id: "%1".to_string(),
                window_index: 0,
                window_name: "video".to_string(),
                pane_index: 0,
                process: "mpv".to_string(),
            },
            ManagedApp {
                pane_id: "%2".to_string(),
                window_index: 1,
                window_name: "files".to_string(),
                pane_index: 0,
                process: "yazi".to_string(),
            },
        ],
    );
    assert_eq!(app.screen, Screen::AppView);

    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(!app.should_quit);
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Key(key) })
            if pane_id == "%1" && key == "C-c"
    ));

    app.handle_key(key(KeyCode::Tab));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Key(key) })
            if pane_id == "%1" && key == "Tab"
    ));
    app.handle_key(key(KeyCode::F(1)));
    assert_eq!(app.screen, Screen::AppView);
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Key(key) })
            if pane_id == "%1" && key == "F1"
    ));
    assert_eq!(app.app_view.as_ref().expect("app view").selected, 0);
    app.handle_key(key(KeyCode::BackTab));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Key(key) })
            if pane_id == "%1" && key == "BTab"
    ));
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(app.app_view.as_ref().expect("app view").selected, 1);
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(app.app_view.as_ref().expect("app view").selected, 0);
    app.focus_app("video");
    assert_eq!(app.app_view.as_ref().expect("app view").selected, 0);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    app.handle_key(key(KeyCode::Char('j')));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Text(text) })
            if pane_id == "%2" && text == "j"
    ));
    app.handle_key(key(KeyCode::Backspace));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Key(key) })
            if pane_id == "%2" && key == "BSpace"
    ));

    app.app_view.as_mut().expect("app view").content =
        "\u{1b}[31membedded app output\u{1b}[0m".to_string();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("app view renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("embedded app output"));
    assert!(rendered.contains("[Alt+K Keys]"));
    assert!(rendered.contains("[Background]"));
    assert!(rendered.contains("[Close]"));
    assert!(rendered.contains("Shift+Alt"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Red),
        "ANSI red should be preserved in App View"
    );

    app.app_view.as_mut().expect("app view").content = format!(
        "\u{1b}[107m{}\u{1b}[0m\n\u{1b}[107m{}\u{1b}[0m",
        " ".repeat(78),
        " ".repeat(78)
    );
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("app background renders");
    let canvas_cell = terminal
        .backend()
        .buffer()
        .cell((40, 12))
        .expect("app canvas cell");
    assert_eq!(
        canvas_cell.bg,
        Color::White,
        "the app background should fill uncaptured canvas rows"
    );

    app.handle_key(key(KeyCode::Esc));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Key(key) })
            if pane_id == "%2" && key == "Escape"
    ));
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::CloseApp(pane_id)) if pane_id == "%2"
    ));
    app.update_app_view(Vec::new(), String::new());
    assert_eq!(app.screen, Screen::Home);
    assert!(app.app_view.is_none());
}

#[test]
fn app_view_key_guide_explains_conflicts_and_shift_alt_passthrough() {
    let mut app = app();
    app.open_app_view(
        "t4e-apps".to_string(),
        vec![ManagedApp {
            pane_id: "%30".to_string(),
            window_index: 0,
            window_name: "cava".to_string(),
            pane_index: 0,
            process: "cava".to_string(),
        }],
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT));
    let view = app.app_view.as_ref().expect("app view");
    assert!(view.key_guide_open);

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("key guide renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Cava key guide"));
    assert!(rendered.contains("cycle background color"));
    assert!(rendered.contains("No documented conflicts"));
    assert!(rendered.contains("Shift+Alt"));

    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.app_view.as_ref().expect("app view").key_guide_scroll, 1);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.app_view.as_ref().expect("app view").key_guide_open);

    for expected in ["b", "f"] {
        app.handle_key(key(KeyCode::Char(
            expected.chars().next().expect("one character"),
        )));
        assert!(matches!(
            app.take_effect(),
            Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Text(text) })
                if pane_id == "%30" && text == expected
        ));
    }

    app.handle_key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::SHIFT | KeyModifiers::ALT,
    ));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SendAppInput { pane_id, input: t4e::app::state::AppInput::Key(key) })
            if pane_id == "%30" && key == "M-q"
    ));

    app.catalog
        .tools
        .iter_mut()
        .find(|tool| tool.id == "cava")
        .expect("Cava catalog entry")
        .key_hints
        .push(KeyHint::Binding {
            keys: vec!["M-q".to_string()],
            action: "alternate app command".to_string(),
        });
    app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT));
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("conflict guide renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Alt+Q: app alternate app command / T4E close app"));
    app.handle_key(key(KeyCode::Esc));

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::CloseApp(pane_id)) if pane_id == "%30"
    ));
}

#[test]
fn app_view_selects_recent_or_older_clean_urls() {
    let mut app = app();
    app.open_app_view(
        "t4e-links".to_string(),
        vec![ManagedApp {
            pane_id: "%50".to_string(),
            window_index: 0,
            window_name: "spotatui".to_string(),
            pane_index: 0,
            process: "spotatui".to_string(),
        }],
    );
    let auth_url = "https://accounts.spotify.com/authorize?redirect_uri=http%3A%2F%2F127.0.0.1%3A8989%2Flogin&client_id=test&code_challenge=long-value";
    let latest_url = "https://example.com/authorization-complete";
    let content = format!(
        "Using redirect URI: http://127.0.0.1:8989/login\nOpen this URL:\n{auth_url}\nNew URL:\n{latest_url}\nWaiting..."
    );
    app.app_view.as_mut().expect("app view").content = content.clone();

    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT));
    let Some(AppEffect::ReadAppLinks { pane_id, action }) = app.take_effect() else {
        panic!("joined link capture should be requested");
    };
    assert_eq!(pane_id, "%50");
    assert_eq!(action, t4e::app::state::LinkAction::Open);
    app.apply_app_links(action, &content);
    let picker = app.link_picker.as_ref().expect("link picker");
    assert_eq!(picker.urls[0], latest_url);
    assert_eq!(picker.selected, 0);
    assert!(app.take_effect().is_none());
    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::OpenUrl(url)) if url == latest_url
    ));

    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT));
    let Some(AppEffect::ReadAppLinks { pane_id, action }) = app.take_effect() else {
        panic!("joined link capture should be requested");
    };
    assert_eq!(pane_id, "%50");
    assert_eq!(action, t4e::app::state::LinkAction::Copy);
    app.apply_app_links(action, &content);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::CopyUrl(url)) if url == auth_url
    ));
}

#[test]
fn app_process_exit_keeps_remaining_apps_or_returns_to_t4e() {
    let mut app = app();
    app.handle_key(key(KeyCode::Enter));
    app.open_app_view(
        "t4e-exit".to_string(),
        vec![
            ManagedApp {
                pane_id: "%60".to_string(),
                window_index: 0,
                window_name: "first".to_string(),
                pane_index: 0,
                process: "first".to_string(),
            },
            ManagedApp {
                pane_id: "%61".to_string(),
                window_index: 1,
                window_name: "second".to_string(),
                pane_index: 0,
                process: "second".to_string(),
            },
        ],
    );

    app.update_app_view(
        vec![ManagedApp {
            pane_id: "%61".to_string(),
            window_index: 1,
            window_name: "second".to_string(),
            pane_index: 0,
            process: "second".to_string(),
        }],
        String::new(),
    );
    assert_eq!(app.screen, Screen::AppView);
    assert_eq!(app.app_view.as_ref().expect("app view").apps.len(), 1);

    app.update_app_view(Vec::new(), String::new());
    assert_eq!(app.screen, Screen::Home);
    assert!(app.app_view.is_none());
    assert_eq!(app.status, "App closed");
}

#[test]
fn alt_backspace_returns_to_the_previous_screen_without_closing_the_app() {
    let mut app = app();
    app.handle_key(key(KeyCode::Enter));
    app.open_app_view(
        "t4e-background".to_string(),
        vec![ManagedApp {
            pane_id: "%30".to_string(),
            window_index: 0,
            window_name: "app".to_string(),
            pane_index: 0,
            process: "app".to_string(),
        }],
    );

    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));

    assert_eq!(app.screen, Screen::Home);
    assert!(app.app_view.is_some());
    assert!(app.take_effect().is_none());
}

#[test]
fn catalog_enter_reopens_running_app_without_launch_options_or_duplicate_launch() {
    let mut app = app();
    app.remember_app_view(
        "t4e-apps".to_string(),
        vec![ManagedApp {
            pane_id: "%40".to_string(),
            window_index: 0,
            window_name: "cmatrix".to_string(),
            pane_index: 0,
            process: "cmatrix".to_string(),
        }],
    );
    open_catalog_search(&mut app, "cmatrix");
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.screen, Screen::AppView);
    assert_eq!(app.app_view.as_ref().expect("app view").selected, 0);
    assert!(app.launch_options.is_none());
    assert!(app.take_effect().is_none());
}

#[test]
fn ascii_camera_requires_only_the_first_session_launch_approval() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ascii-camera".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    app.handle_key(key(KeyCode::Enter));
    assert!(app.launch_options.is_some());
    app.handle_key(key(KeyCode::Enter));
    assert!(app.launch_approval.is_some());
    assert!(app.take_effect().is_none());

    app.handle_key(key(KeyCode::Enter));
    let Some(AppEffect::LaunchTool(first)) = app.take_effect() else {
        panic!("approved camera launch expected");
    };
    assert_eq!(first.tool_id, "ascii-camera");
    assert!(first.command.contains("--device 0"));
    assert!(first.command.contains("--vo tct"));

    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.launch_approval.is_none());
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request)) if request.tool_id == "ascii-camera"
    ));
}

#[test]
fn ascii_camera_uninstall_keeps_the_shared_mpv_package() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ascii-camera".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.installed_tools.insert("ascii-camera".to_string());

    app.handle_key(key(KeyCode::Char('U')));
    let request = app
        .uninstall_confirmation
        .as_ref()
        .expect("uninstall confirmation");
    assert!(request.command.contains("t4e-ascii-camera"));
    assert!(!request.command.contains("apt-get"));
    assert!(!request.command.contains("brew uninstall"));
}

#[test]
fn mouse_selects_lists_switches_tabs_and_closes_from_the_footer() {
    let mut app = app();
    assert!(app.mouse_enabled, "mouse controls are enabled by default");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 9,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.home_filter_index, 2);
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 3,
            row: 9,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.home_filter_index, 3);

    app.open_app_view(
        "t4e-mouse".to_string(),
        vec![
            ManagedApp {
                pane_id: "%10".to_string(),
                window_index: 0,
                window_name: "one".to_string(),
                pane_index: 0,
                process: "one".to_string(),
            },
            ManagedApp {
                pane_id: "%11".to_string(),
                window_index: 1,
                window_name: "two".to_string(),
                pane_index: 0,
                process: "two".to_string(),
            },
        ],
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.app_view.as_ref().expect("app view").selected, 1);
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 33,
            row: 22,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::CloseApp(pane_id)) if pane_id == "%11"
    ));
    app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SetMouseCapture(false))
    ));
}

#[test]
fn mouse_clicks_header_navigation_tabs() {
    let mut app = app();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 9,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.screen, Screen::Logs);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.screen, Screen::Home);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 32,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.screen, Screen::Help);
}

#[test]
fn mouse_click_opens_the_home_search_input_above_quick_access() {
    let mut app = app();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert!(app.search_mode);
    assert_eq!(app.home_focus, HomeFocus::Views);
    assert_eq!(app.selected_home_filter(), HomeFilter::AllApps);

    for ch in "figlet".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert_eq!(app.search_query, "figlet");
    assert_eq!(app.home_tools().len(), 1);
}

#[test]
fn alt_q_quits_home_even_while_search_is_focused() {
    let mut app = app();
    open_home_search(&mut app);
    assert!(app.search_mode);

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT));

    assert!(app.should_quit);
    assert!(!app.search_mode);
    assert!(app.search_query.is_empty());
}

#[test]
fn mouse_drag_requests_automatic_panel_selection_copy() {
    let mut app = app();
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 30,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert!(app.mouse_selection.is_some());
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );

    assert!(app.mouse_selection.is_none());
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::CopySelection {
            start: (30, 4),
            end: (40, 5)
        })
    ));
}

#[test]
fn mouse_wheel_scrolls_the_focused_home_assistant_conversation() {
    let mut app = app();
    app.home_focus = HomeFocus::Assistant;
    let selected_app = app.home_app_index;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 80,
            row: 24,
            modifiers: KeyModifiers::NONE,
        },
        30,
    );
    assert!(app.ai_conversation_scroll > 0);
    assert_eq!(app.home_app_index, selected_app);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 80,
            row: 24,
            modifiers: KeyModifiers::NONE,
        },
        30,
    );
    assert_eq!(app.ai_conversation_scroll, 0);
    assert_eq!(app.home_app_index, selected_app);
}

#[test]
fn assistant_conversation_remains_scrollable_while_composing() {
    let mut app = app();
    app.home_focus = HomeFocus::Assistant;
    app.ai_input_mode = true;
    app.ai_input = "draft".to_string();

    app.handle_key(key(KeyCode::PageUp));
    assert!(app.ai_conversation_scroll > 0);
    let keyboard_scroll = app.ai_conversation_scroll;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 80,
            row: 24,
            modifiers: KeyModifiers::NONE,
        },
        30,
    );

    assert!(app.ai_conversation_scroll > keyboard_scroll);
    assert!(app.ai_input_mode);
    assert_eq!(app.ai_input, "draft");
}

#[test]
fn closing_an_app_returns_to_home_when_launched_from_home() {
    let mut app = app();
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.home_focus, HomeFocus::AppList);
    app.open_app_view(
        "t4e-home-return".to_string(),
        vec![ManagedApp {
            pane_id: "%20".to_string(),
            window_index: 0,
            window_name: "app".to_string(),
            pane_index: 0,
            process: "app".to_string(),
        }],
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::CloseApp(pane_id)) if pane_id == "%20"
    ));
    app.update_app_view(Vec::new(), String::new());

    assert_eq!(app.screen, Screen::Home);
    assert!(app.app_view.is_none());
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
        for screen_key in ['1', '2', '3', '4', '5', '6', '7', '8'] {
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

    assert!(rendered.contains("T4E"));
    assert!(rendered.contains("HOME"));
    assert!(!rendered.contains(" AI "));
    assert!(rendered.contains("Activity"));
    assert!(rendered.contains("Settings"));
    assert!(rendered.contains("Help"));
    assert!(rendered.contains("Backspace back"));
}

#[test]
fn home_shows_compact_fastfetch_information_and_application_entry_points() {
    let mut app = app();
    app.system_overview.logo = vec!["\u{1b}[31mASCII-OS\u{1b}[0m".to_string()];
    app.remember_app_view(
        "t4e-information-running".to_string(),
        vec![ManagedApp {
            pane_id: "%71".to_string(),
            window_index: 0,
            window_name: "yazi".to_string(),
            pane_index: 0,
            process: "yazi".to_string(),
        }],
    );
    let backend = TestBackend::new(120, 35);
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

    assert!(rendered.contains("Apps"));
    assert!(rendered.contains("Search apps..."));
    assert!(rendered.contains("Quick Access"));
    assert!(rendered.contains("All Apps"));
    assert!(rendered.contains("Information"));
    assert!(rendered.contains("ASCII-OS"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "A" && cell.fg == Color::Red),
        "fastfetch ANSI logo color should reach the rendered cells"
    );
    assert!(rendered.contains("Running: 1 apps"));
    assert!(!rendered.contains("Installs:"));
    assert!(!rendered.contains("Saved:"));
    assert!(rendered.contains("→ apps"));
    assert!(rendered.contains("↑/↓ view"));
    for label in ["OS:", "Host:", "Kernel:", "CPU:", "Memory:"] {
        assert!(
            rendered.contains(label),
            "missing system information {label}"
        );
    }
    assert!(!rendered.contains("workspace templates"));
}

#[test]
fn home_spans_assistant_below_the_app_and_information_columns() {
    let mut app = app();
    let app_title = app.selected_home_filter().label().to_string();
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("HOME renders");
    let rows = terminal
        .backend()
        .buffer()
        .content()
        .chunks(120)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();
    let find_row = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle} title renders"))
    };

    let app_row = find_row(&app_title);
    let assistant_row = find_row("Assistant");
    let information_row = find_row("Information");
    assert!(assistant_row > app_row);
    assert!(information_row < assistant_row);
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer.cell((24, assistant_row as u16)).unwrap().symbol(),
        "┌"
    );
    assert_eq!(
        buffer.cell((119, assistant_row as u16)).unwrap().symbol(),
        "┐"
    );
}

#[test]
fn home_assistant_accumulates_full_messages_and_scrolls_to_history() {
    let mut app = app();
    app.home_focus = HomeFocus::Assistant;
    app.ai_messages.push(AiMessage {
        role: "You".to_string(),
        text: "OLDEST LINE 1\nOLDEST LINE 2\nOLDEST LINE 3\nOLDEST LINE 4".to_string(),
    });
    for index in 0..12 {
        app.ai_messages.push(AiMessage {
            role: "Codex".to_string(),
            text: format!("history response {index}"),
        });
    }
    app.ai_messages.push(AiMessage {
        role: "Codex".to_string(),
        text: "LATEST ASSISTANT RESPONSE".to_string(),
    });

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("latest conversation renders");
    let latest = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(latest.contains("LATEST ASSISTANT RESPONSE"));
    assert!(!latest.contains("OLDEST LINE 4"));

    for _ in 0..100 {
        app.handle_key(key(KeyCode::Up));
    }
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("conversation history renders");
    let history = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(history.contains("OLDEST LINE 1"));
    assert!(history.contains("OLDEST LINE 4"));
}

#[test]
fn minimum_home_uses_the_compact_assistant_fallback() {
    let mut app = app();
    app.home_focus = HomeFocus::Assistant;
    let backend = TestBackend::new(60, 16);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("minimum HOME renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("Assistant"));
    assert!(rendered.contains("Tab panels"));
    assert!(rendered.contains("S-Tab tabs"));
}

#[test]
fn home_information_shows_live_logs_for_the_selected_installing_app() {
    let mut app = app();
    let tool = app.selected_home_tool().expect("HOME selects an app");
    let tool_id = tool.id.clone();
    let tool_name = tool.name.clone();
    app.handle_key(key(KeyCode::Char('I')));
    app.mark_execution_started(&tool_id);
    app.record_output(
        &tool_id,
        t4e::installer::execution::OutputChunk {
            stream: t4e::installer::execution::OutputStream::Stdout,
            text: "Downloading selected app\n".to_string(),
        },
    );
    app.record_output(
        &tool_id,
        t4e::installer::execution::OutputChunk {
            stream: t4e::installer::execution::OutputStream::Stderr,
            text: "Compiling selected app\n".to_string(),
        },
    );

    let backend = TestBackend::new(160, 35);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("HOME renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains(&format!("Installing · {tool_name}")));
    assert!(rendered.contains("attempt 1/"));
    assert!(rendered.contains("Downloading selected app"));
    assert!(rendered.contains("Compiling selected app"));
}

#[test]
fn home_distinguishes_installing_from_installed_with_status_priority_and_color() {
    let mut app = app();
    let tool_id = app
        .selected_home_tool()
        .expect("HOME selects an app")
        .id
        .clone();
    app.handle_key(key(KeyCode::Char('I')));
    app.mark_execution_started(&tool_id);
    app.installed_tools.insert(tool_id);

    let backend = TestBackend::new(160, 35);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("HOME renders");
    let cells = terminal.backend().buffer().content();
    let installing_is_yellow = cells
        .windows("INSTALLING".len())
        .filter(|window| {
            window.iter().map(|cell| cell.symbol()).collect::<String>() == "INSTALLING"
        })
        .any(|window| window.iter().all(|cell| cell.fg == Color::Yellow));

    assert!(
        installing_is_yellow,
        "the HOME installing status should be yellow"
    );
    assert!(
        !cells.windows("INSTALLED".len()).any(|window| window
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            == "INSTALLED"),
        "an actively installing app must not be labeled installed"
    );
}

#[test]
fn home_up_from_quick_access_opens_search_and_hints_follow_focus() {
    let mut app = app();
    app.home_filter_index = 0;
    assert_eq!(app.home_focus, HomeFocus::Views);

    app.handle_key(key(KeyCode::Up));
    assert!(app.search_mode);

    app.handle_key(key(KeyCode::Esc));
    assert!(!app.search_mode);
    app.home_filter_index = 3;
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.home_focus, HomeFocus::AppList);

    let backend = TestBackend::new(160, 35);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("HOME renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("← views"));
    assert!(rendered.contains("↑/↓ app"));
    assert!(!rendered.contains("←/→ switch panel"));
}

#[test]
fn home_search_ignores_the_previous_view_and_arrows_open_results() {
    let mut app = app();
    app.home_filter_index = HomeFilter::ALL
        .iter()
        .position(|filter| *filter == HomeFilter::Favorites)
        .expect("Favorites exists");
    open_home_search(&mut app);
    for ch in "yazi".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(app.selected_home_filter(), HomeFilter::AllApps);
    assert_eq!(
        app.home_tools().first().expect("global result exists").id,
        "yazi"
    );

    app.handle_key(key(KeyCode::Down));
    assert!(!app.search_mode);
    assert_eq!(app.home_focus, HomeFocus::AppList);

    open_home_search(&mut app);
    assert!(app.search_mode);
    app.handle_key(key(KeyCode::Right));
    assert!(!app.search_mode);
    assert_eq!(app.home_focus, HomeFocus::AppList);
}

#[test]
fn help_tab_explains_capabilities_and_derived_risk() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('?')));
    assert_eq!(app.screen, Screen::Help);

    let backend = TestBackend::new(120, 35);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("help renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    for label in ["SAFE", "LOW", "HIGH", "DANGER", "Installation policy"] {
        assert!(rendered.contains(label), "missing help label {label}");
    }
    assert!(rendered.contains("NETWORK"));
    assert!(rendered.contains("AUTONOMOUS"));
    assert!(rendered.contains("CAMERA_CAPTURE"));
    assert!(rendered.contains("Ctrl+F"));

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.screen, Screen::Help);
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Home);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.home_focus, HomeFocus::AppList);
}

#[test]
fn f1_opens_dashboard_help_and_closes_transient_input_modes() {
    let mut app = app();
    open_home_search(&mut app);
    assert!(app.search_mode);

    app.handle_key(key(KeyCode::F(1)));

    assert_eq!(app.screen, Screen::Help);
    assert!(!app.search_mode);
    assert!(!app.ai_input_mode);
}

#[test]
fn home_library_queues_only_the_selected_app() {
    let mut app = app();

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.home_focus, HomeFocus::AppList);
    let selected_id = app
        .selected_home_tool()
        .expect("HOME selects an app")
        .id
        .clone();
    app.handle_key(key(KeyCode::Char('I')));
    assert_eq!(app.queue.len(), 1);
    assert_eq!(app.queue[0].item.tool_id, selected_id);
}

#[test]
fn home_library_filters_saved_and_running_apps() {
    let mut app = app();
    app.favorites.insert("newsboat".to_string());
    app.installed_tools.insert("yazi".to_string());
    app.remember_app_view(
        "t4e-running-filter".to_string(),
        vec![ManagedApp {
            pane_id: "%70".to_string(),
            window_index: 0,
            window_name: "yazi".to_string(),
            pane_index: 0,
            process: "yazi".to_string(),
        }],
    );

    for (filter, expected) in [
        (HomeFilter::Favorites, "newsboat"),
        (HomeFilter::Installed, "yazi"),
        (HomeFilter::Running, "yazi"),
    ] {
        app.home_filter_index = HomeFilter::ALL
            .iter()
            .position(|candidate| *candidate == filter)
            .expect("filter exists");
        let tools = app.home_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, expected);
    }
}

#[test]
fn ai_category_lists_its_three_apps() {
    let mut app = app();
    app.home_filter_index = HomeFilter::ALL
        .iter()
        .position(|filter| *filter == HomeFilter::Category(AppCategory::Ai))
        .expect("AI category exists");

    let visible = app.home_tools();
    assert_eq!(visible.len(), 3);
    assert!(
        visible
            .iter()
            .all(|tool| tool.app_category() == AppCategory::Ai)
    );
}

#[test]
fn editors_category_includes_termleaf_as_a_default_app() {
    let mut app = app();
    app.home_filter_index = HomeFilter::ALL
        .iter()
        .position(|filter| *filter == HomeFilter::Category(AppCategory::Editors))
        .expect("Editors category exists");

    let visible = app.home_tools();
    let termleaf = visible
        .iter()
        .find(|tool| tool.id == "termleaf")
        .expect("termleaf is visible in Editors");
    assert_eq!(termleaf.run.cmd, "termleaf");
    assert_eq!(termleaf.exposure, t4e::catalog::models::Exposure::Starter);
}

#[test]
fn global_catalog_entry_clears_a_transient_home_search() {
    let mut app = app();
    app.search_query = "stale-search".to_string();

    app.handle_key(key(KeyCode::Char('2')));

    assert_eq!(app.screen, Screen::Catalog);
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
    app.handle_key(key(KeyCode::Char('I')));

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
            .any(|line| line.ends_with("install: ripgrep already installed"))
    );
}

#[test]
fn activity_supports_arrow_and_page_navigation_with_timestamped_entries() {
    let mut app = app();
    for index in 0..25 {
        app.record_log(format!("activity event {index}"));
    }
    app.handle_key(key(KeyCode::Char('6')));

    let latest = app.logs.last().expect("activity entry");
    assert!(latest.ends_with("activity event 24"));
    chrono::DateTime::parse_from_str(&latest[1..27], "%Y-%m-%d %H:%M:%S %:z")
        .expect("timestamp includes local UTC offset");

    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.activity_scroll, 1);
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.activity_scroll, 11);
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.activity_scroll, 1);
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.activity_scroll, 0);

    app.handle_key(key(KeyCode::End));
    assert_eq!(app.activity_scroll, app.logs.len() - 1);
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.activity_scroll, app.logs.len() - 1);
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.activity_scroll, 0);
}

#[test]
fn app_execution_errors_are_recorded_in_activity() {
    let mut app = app();
    app.apply_app_error("launch", &anyhow::anyhow!("renderer unavailable"));

    assert_eq!(
        app.logs.last().and_then(|entry| entry.split("] ").nth(1)),
        Some("app: launch failed: renderer unavailable")
    );
    assert_eq!(app.status, "App launch failed: renderer unavailable");
}

#[test]
fn catalog_installs_current_app_and_tracks_favorites() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    let first_id = app.selected_catalog_tool().expect("first tool").id.clone();
    app.handle_key(key(KeyCode::Char('f')));
    app.handle_key(key(KeyCode::Char('I')));

    assert!(app.favorites.contains(&first_id));
    assert!(app.queue.iter().any(|job| job.item.tool_id == first_id));
    assert_eq!(app.queue.len(), 1);
}

#[test]
fn catalog_shows_inline_install_state_and_recent_progress_output() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    let tool_id = app.selected_catalog_tool().expect("tool").id.clone();
    app.handle_key(key(KeyCode::Char('I')));
    app.mark_execution_started(&tool_id);
    app.record_output(
        &tool_id,
        t4e::installer::execution::OutputChunk {
            stream: t4e::installer::execution::OutputStream::Stdout,
            text: "Downloading package\n".to_string(),
        },
    );
    app.record_output(
        &tool_id,
        t4e::installer::execution::OutputChunk {
            stream: t4e::installer::execution::OutputStream::Stderr,
            text: "Compiling dependency\n".to_string(),
        },
    );

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("catalog renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("INSTALLING..."));
    assert!(rendered.contains("attempts: 1/"));
    assert!(rendered.contains("Live install output"));
    assert!(rendered.contains("Downloading package"));
    assert!(rendered.contains("[progress]"));
    assert!(!rendered.contains("[err]"));
}

#[test]
fn shutdown_cancellation_is_reported_as_an_interrupted_install() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('I')));
    let mut completed = app.queue[0].clone();
    completed
        .item
        .transition(QueueState::Installing)
        .expect("starts");
    completed
        .item
        .transition(QueueState::Failed)
        .expect("fails");
    completed.attempts.push(InstallAttempt {
        attempt: 1,
        exit_code: None,
        duration_ms: 100,
        timed_out: false,
        cancelled: true,
        log_path: "/tmp/interrupted.log".to_string(),
    });
    app.should_quit = true;

    app.apply_execution(completed);

    assert!(
        app.queue[0]
            .diagnostics
            .as_ref()
            .is_some_and(|diagnostics| diagnostics.stderr_summary.contains("exited or restarted"))
    );
}

#[test]
fn settings_controls_update_execution_policy() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('7')));
    app.handle_key(key(KeyCode::Right));
    assert!(!app.mouse_enabled);
    assert!(!app.settings.mouse_enabled);
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SetMouseCapture(false))
    ));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.max_install_attempts, 3);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char(' ')));
    assert!(app.settings.confirm_all_installs);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(app.settings.ai_approval_mode, AiApprovalMode::Ask);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.theme, AppTheme::Amber);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.theme, AppTheme::GreenScreen);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.theme, AppTheme::Terracotta);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.settings.theme, AppTheme::Future);
}

#[test]
fn settings_explain_the_selected_policy_and_reset_scope() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('7')));
    for (index, expected) in [
        "clicking, scrolling, and drag-to-copy",
        "Total automatic attempts",
        "Script and DANGER installs",
        "one setup flow for subscription or API-key mode",
        "AI permission mode",
        "interface palette independently",
        "Favorites, recent apps, activity history",
    ]
    .into_iter()
    .enumerate()
    {
        app.settings_index = index;
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, &mut app))
            .expect("settings render");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(
            rendered.contains(expected),
            "setting {index} is missing its selected detail"
        );
    }
}

#[test]
fn settings_reset_restores_defaults_and_clears_saved_app_options() {
    let mut app = app();
    app.settings.install_timeout_sec = 1_200;
    app.launch_preferences.insert(
        "cmatrix".to_string(),
        [(
            "color".to_string(),
            t4e::storage::LaunchOptionPreference {
                enabled: true,
                value: Some("cyan".to_string()),
            },
        )]
        .into(),
    );
    app.handle_key(key(KeyCode::Char('7')));
    for _ in 0..6 {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.settings, UserSettings::default());
    assert!(app.launch_preferences.is_empty());
    assert!(app.status.contains("reset"));
}

#[test]
fn home_ai_shows_and_advances_request_review_run_workflow() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Ask;

    app.apply_codex_event(CodexEvent::TurnStarted("turn-flow".to_string()));
    assert_eq!(app.ai_workflow_phase, AiWorkflowPhase::Request);

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "catalog_search".to_string(),
        target: "yazi".to_string(),
    });
    assert_eq!(app.ai_workflow_phase, AiWorkflowPhase::Review);
    assert!(app.ai_confirmation.is_some());

    app.handle_key(key(KeyCode::Char('y')));
    assert_eq!(app.ai_workflow_phase, AiWorkflowPhase::Run);

    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("workflow renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    for label in ["REQUEST", "REVIEW", "RUN"] {
        assert!(rendered.contains(label), "workflow is missing {label}");
    }
}

#[test]
fn amber_theme_applies_a_full_app_palette_without_changing_the_default() {
    let mut themed = app();
    themed.settings.theme = AppTheme::Amber;
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut themed))
        .expect("amber theme renders");
    let buffer = terminal.backend().buffer();
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| { cell.fg == Color::Rgb(255, 176, 0) && cell.bg == Color::Rgb(33, 21, 0) })
    );

    assert_eq!(UserSettings::default().theme, AppTheme::Future);
}

#[test]
fn amber_theme_applies_to_the_top_navigation_tabs() {
    let mut app = app();
    app.settings.theme = AppTheme::Amber;
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("amber navigation renders");
    let buffer = terminal.backend().buffer();

    let border = buffer.cell((0, 0)).expect("navigation border");
    assert_eq!(border.bg, Color::Rgb(33, 21, 0));
    assert_eq!(border.fg, Color::Rgb(116, 80, 0));

    let row = (0..100)
        .map(|x| buffer.cell((x, 1)).expect("navigation row cell").symbol())
        .collect::<String>();
    let activity_x = row.find("Activity").expect("Activity tab") as u16;
    let activity = buffer.cell((activity_x, 1)).expect("Activity tab cell");
    assert_eq!(activity.bg, Color::Rgb(33, 21, 0));
    assert_eq!(activity.fg, Color::Rgb(255, 210, 122));
}

#[test]
fn future_theme_applies_the_web_future_palette_to_navigation_tabs() {
    let mut app = app();
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("default navigation renders");
    let buffer = terminal.backend().buffer();
    let row = (0..100)
        .map(|x| buffer.cell((x, 1)).expect("navigation row cell").symbol())
        .collect::<String>();
    let activity_x = row.find("Activity").expect("Activity tab") as u16;
    let activity = buffer.cell((activity_x, 1)).expect("Activity tab cell");
    assert_eq!(activity.bg, Color::Rgb(11, 25, 30));
    assert_eq!(activity.fg, Color::Rgb(154, 220, 227));

    let border = buffer.cell((0, 0)).expect("navigation border");
    assert_eq!(border.bg, Color::Rgb(11, 25, 30));
    assert_eq!(border.fg, Color::Rgb(52, 70, 74));
}

#[test]
fn amber_theme_applies_to_the_running_app_tabs() {
    let mut app = app();
    app.settings.theme = AppTheme::Amber;
    app.open_app_view(
        "t4e-themed-apps".to_string(),
        vec![
            ManagedApp {
                pane_id: "%theme-1".to_string(),
                window_index: 0,
                window_name: "video".to_string(),
                pane_index: 0,
                process: "mpv".to_string(),
            },
            ManagedApp {
                pane_id: "%theme-2".to_string(),
                window_index: 1,
                window_name: "files".to_string(),
                pane_index: 0,
                process: "yazi".to_string(),
            },
        ],
    );
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("amber app tabs render");
    let buffer = terminal.backend().buffer();

    let border = buffer.cell((0, 0)).expect("app tabs border");
    assert_eq!(border.bg, Color::Rgb(33, 21, 0));
    assert_eq!(border.fg, Color::Rgb(116, 80, 0));

    let row = (0..100)
        .map(|x| buffer.cell((x, 1)).expect("app tabs row cell").symbol())
        .collect::<String>();
    let files_x = row.find("files").expect("files tab") as u16;
    let files = buffer.cell((files_x, 1)).expect("files tab cell");
    assert_eq!(files.bg, Color::Rgb(33, 21, 0));
    assert_eq!(files.fg, Color::Rgb(255, 210, 122));
}

#[test]
fn api_provider_setup_keeps_keys_out_of_persistent_settings() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('7')));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Right));
    let backend = TestBackend::new(60, 16);
    let mut terminal = Terminal::new(backend).expect("compact terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("compact unified provider setup renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Unified AI connection"));
    assert!(!rendered.contains("OpenAI-compatible API provider"));
    let setup = app
        .api_provider_setup
        .as_mut()
        .expect("provider setup opens");
    setup.field = 5;
    setup.api_key = "session-secret-key".to_string();
    assert!(!format!("{setup:?}").contains("session-secret-key"));
    setup.field = 6;
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::ConfigureApiProvider {
        provider,
        profile,
        api_key,
    }) = app.take_effect()
    else {
        panic!("provider configuration effect expected");
    };
    assert_eq!(provider, AiProvider::Codex);
    assert_eq!(profile.api_key_env, "OPENAI_API_KEY");
    assert_eq!(profile.auth_mode.label(), "API key");
    assert!(!format!("{api_key:?}").contains("session-secret-key"));
    assert_eq!(api_key.into_inner(), "session-secret-key");
    let persisted = serde_json::to_string(&app.settings).expect("settings serialize");
    assert!(!persisted.contains("session-secret-key"));
}

#[test]
fn queue_run_schedules_multiple_app_jobs_sequentially() {
    let mut app = app();
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('I')));
    app.handle_key(key(KeyCode::Down));
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
    app.apply_codex_event(CodexEvent::Ready {
        account: "chatgpt".to_string(),
    });
    app.handle_key(key(KeyCode::Char('4')));
    for ch in "show workspaces".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let Some(AppEffect::CodexPrompt(prompt)) = app.take_effect() else {
        panic!("Codex prompt expected");
    };
    assert_eq!(prompt, "show workspaces");
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
fn ai_context_describes_the_catalog_and_queue_without_legacy_workspaces() {
    let mut app = app();
    let tool = app
        .catalog
        .tools
        .iter()
        .find(|tool| tool.id == "yazi")
        .expect("yazi exists")
        .clone();
    let installer = tool
        .installers
        .iter()
        .find(|installer| installer.platform == Platform::Linux)
        .expect("linux installer");
    let task = build_install_task(&tool, installer, &InstallPolicy::default()).expect("task");
    app.queue.push(InstallJob::new(task, "apt"));
    app.apply_managed_sessions(vec![t4e::mux::runtime::ManagedSession {
        name: "t4e-video".to_string(),
        workspace_id: "video-desk".to_string(),
        attached_clients: 0,
        windows: 1,
    }]);

    let context = app.ai_environment_context();

    assert!(context.contains(&format!(
        "platform: {}",
        t4e::catalog::models::Platform::current().as_str()
    )));
    assert!(context.contains("AI permission mode: Auto"));
    assert!(context.contains("yazi=Yazi (not installed; run: yazi)"));
    assert!(context.contains("yazi:Queued"));
    assert!(!context.contains("video-desk=Video Desk"));
    assert!(!context.contains("workspaces:"));
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
fn ai_rejects_legacy_workspace_actions_hidden_from_the_main_product() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('4')));
    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "workspace_launch".to_string(),
        target: "video-desk".to_string(),
    });
    assert!(app.ai_confirmation.is_none());
    assert!(app.take_effect().is_none());
    assert!(app.ai_status.contains("Rejected unsupported AI action"));
    assert_eq!(app.screen, Screen::Home);
}

#[test]
fn ai_catalog_search_is_bounded_to_local_navigation() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Ask;
    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "catalog_search".to_string(),
        target: "yazi".to_string(),
    });
    assert_eq!(app.screen, Screen::Home);
    assert!(app.ai_confirmation.is_some());
    app.handle_key(key(KeyCode::Char('y')));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.search_query, "yazi");
    assert!(app.take_effect().is_none());
}

#[test]
fn auto_ai_permission_executes_validated_actions_without_review_input() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Auto;

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "catalog_search".to_string(),
        target: "yazi".to_string(),
    });
    assert_eq!(app.search_query, "yazi");
    assert!(app.ai_confirmation.is_none());
    assert!(app.ai_status.contains("auto-approved"));

    app.apply_installed_tools(["yazi".to_string()].into_iter().collect());
    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_app".to_string(),
        target: "yazi".to_string(),
    });
    assert!(app.ai_confirmation.is_none());
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request)) if request.tool_id == "yazi"
    ));
}

#[test]
fn auto_ai_permission_launches_a_validated_multi_app_pipeline() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Auto;

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_pipeline".to_string(),
        target: "fortune | figlet | lolcat".to_string(),
    });

    assert!(app.ai_confirmation.is_none());
    let Some(AppEffect::LaunchPipeline(request)) = app.take_effect() else {
        panic!("validated pipeline should launch");
    };
    assert_eq!(request.pipeline_id, "fortune-to-figlet-to-lolcat");
    assert_eq!(
        request
            .stages
            .iter()
            .map(|stage| (stage.tool_id.as_str(), stage.command.as_str()))
            .collect::<Vec<_>>(),
        [
            ("fortune", "fortune"),
            ("figlet", "figlet"),
            ("lolcat", "lolcat"),
        ]
    );
    assert!(request.keep_open);
}

#[test]
fn bypass_ai_pipeline_prepares_missing_stages_then_launches_once() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Bypass;
    app.apply_installed_tools(Default::default());

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_pipeline".to_string(),
        target: "fortune | figlet | lolcat".to_string(),
    });

    for expected in ["fortune", "figlet", "lolcat"] {
        let Some(AppEffect::Execute(job)) = app.take_effect() else {
            panic!("{expected} should be installed before pipeline launch");
        };
        assert_eq!(job.item.tool_id, expected);
        let mut completed = *job;
        completed
            .item
            .transition(QueueState::Installing)
            .expect("installation starts");
        completed
            .item
            .transition(QueueState::Success)
            .expect("installation succeeds");
        app.apply_execution(completed);
    }

    let Some(AppEffect::LaunchPipeline(request)) = app.take_effect() else {
        panic!("pipeline should launch after every missing stage is ready");
    };
    assert_eq!(request.pipeline_id, "fortune-to-figlet-to-lolcat");
    assert!(app.take_effect().is_none());
}

#[test]
fn auto_ai_permission_searches_youtube_and_launches_tplay_without_input() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Auto;
    app.apply_installed_tools(["tplay".to_string()].into_iter().collect());

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_tplay_search".to_string(),
        target: "jellyfish 4K ambient".to_string(),
    });

    assert!(app.ai_confirmation.is_none());
    assert!(app.launch_argument.is_none());
    let Some(AppEffect::LaunchTool(request)) = app.take_effect() else {
        panic!("bounded YouTube search should launch tplay");
    };
    assert_eq!(request.tool_id, "tplay");
    assert_eq!(
        request.command,
        "t4e tplay-search 'jellyfish%204K%20ambient'"
    );
}

#[test]
fn ai_tplay_search_encodes_shell_syntax_instead_of_executing_it() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Auto;
    app.apply_installed_tools(["tplay".to_string()].into_iter().collect());

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_tplay_search".to_string(),
        target: "ink; $(touch nope) | night".to_string(),
    });

    let Some(AppEffect::LaunchTool(request)) = app.take_effect() else {
        panic!("encoded YouTube search should launch tplay");
    };
    assert!(!request.command.contains(';'));
    assert!(!request.command.contains("$("));
    assert!(!request.command.contains('|'));
    assert!(
        request
            .command
            .contains("ink%3B%20%24%28touch%20nope%29%20%7C%20night")
    );
}

#[test]
fn ai_tplay_search_rejects_empty_or_unbounded_queries() {
    for target in ["   ".to_string(), "x".repeat(161)] {
        let mut app = app();
        app.settings.ai_approval_mode = AiApprovalMode::Auto;

        app.apply_codex_event(CodexEvent::ActionProposed {
            kind: "launch_tplay_search".to_string(),
            target,
        });

        assert!(app.ai_confirmation.is_none());
        assert!(app.take_effect().is_none());
        assert!(app.ai_status.contains("Rejected unsupported AI action"));
    }
}

#[test]
fn bypass_ai_tplay_search_installs_then_launches_without_intermediate_input() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Bypass;
    app.apply_installed_tools(Default::default());

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_tplay_search".to_string(),
        target: "Tokyo night drive 4K".to_string(),
    });

    let Some(AppEffect::Execute(mut install)) = app.take_effect() else {
        panic!("missing tplay should install immediately");
    };
    assert_eq!(install.item.tool_id, "tplay");
    assert!(app.confirmation.is_none());
    install
        .item
        .transition(QueueState::Installing)
        .expect("installation starts");
    install
        .item
        .transition(QueueState::Success)
        .expect("installation succeeds");

    app.apply_execution(*install);

    assert!(app.launch_argument.is_none());
    let Some(AppEffect::LaunchTool(request)) = app.take_effect() else {
        panic!("tplay should launch with the retained search after installation");
    };
    assert!(request.command.contains("Tokyo%20night%20drive%204K"));
}

#[test]
fn ai_pipeline_rejects_shell_syntax_and_fewer_than_two_stages() {
    for target in ["fortune --help | figlet", "fortune"] {
        let mut app = app();
        app.settings.ai_approval_mode = AiApprovalMode::Auto;

        app.apply_codex_event(CodexEvent::ActionProposed {
            kind: "launch_pipeline".to_string(),
            target: target.to_string(),
        });

        assert!(app.ai_confirmation.is_none());
        assert!(app.take_effect().is_none());
        assert!(app.ai_status.contains("Rejected unsupported AI action"));
    }
}

#[test]
fn auto_ai_install_plan_starts_safe_install_without_queue_input() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Auto;

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "install_plan".to_string(),
        target: "ripgrep".to_string(),
    });

    assert!(app.ai_confirmation.is_none());
    assert!(app.confirmation.is_none());
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::Execute(job)) if job.item.tool_id == "ripgrep"
    ));
}

#[test]
fn bypass_ai_permission_skips_install_and_camera_approvals() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Bypass;

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "install_plan".to_string(),
        target: "claude-code".to_string(),
    });
    assert!(app.confirmation.is_none());
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::Execute(job)) if job.item.tool_id == "claude-code"
    ));

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_app".to_string(),
        target: "ascii-camera".to_string(),
    });
    assert!(app.launch_approval.is_none());
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request)) if request.tool_id == "ascii-camera"
    ));
}

#[test]
fn bypass_ai_launch_installs_then_starts_without_intermediate_input() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Bypass;
    app.apply_installed_tools(Default::default());

    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_app".to_string(),
        target: "yazi".to_string(),
    });
    let Some(AppEffect::Execute(mut install)) = app.take_effect() else {
        panic!("missing app installation should start immediately");
    };
    assert!(app.confirmation.is_none());
    install
        .item
        .transition(QueueState::Installing)
        .expect("installation starts");
    install
        .item
        .transition(QueueState::Success)
        .expect("installation succeeds");

    app.apply_execution(*install);

    assert!(app.launch_options.is_none());
    assert!(app.launch_approval.is_none());
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request)) if request.tool_id == "yazi"
    ));
}

#[test]
fn ai_action_can_be_denied_without_typing_a_phrase() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Ask;
    app.apply_codex_event(CodexEvent::ActionProposed {
        kind: "launch_app".to_string(),
        target: "yazi".to_string(),
    });

    app.handle_key(key(KeyCode::Char('n')));

    assert!(app.ai_confirmation.is_none());
    assert!(app.take_effect().is_none());
    assert!(app.ai_status.contains("denied"));
}

#[test]
fn ai_launch_resolves_a_catalog_display_name_to_its_exact_id() {
    let mut app = app();
    app.settings.ai_approval_mode = AiApprovalMode::Ask;
    app.apply_installed_tools(["yazi".to_string()].into_iter().collect());
    app.apply_ai_event(AiEvent::ProviderReady(ProviderReadiness {
        provider: AiProvider::Claude,
        account: "max subscription".to_string(),
    }));
    app.apply_ai_event(AiEvent::ActionProposed {
        provider: AiProvider::Claude,
        kind: "launch_app".to_string(),
        target: "Yazi".to_string(),
    });
    assert_eq!(
        app.ai_confirmation
            .as_ref()
            .map(|confirmation| confirmation.action.target.as_str()),
        Some("yazi")
    );

    app.handle_key(key(KeyCode::Enter));

    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request)) if request.tool_id == "yazi"
    ));
}

#[test]
fn home_ai_is_disabled_without_a_ready_provider_and_enables_after_login() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(app.home_focus, HomeFocus::Assistant);
    assert!(!app.ai_input_mode);
    assert!(app.ai_status.contains("unavailable"));

    app.apply_ai_event(AiEvent::ProviderReady(ProviderReadiness {
        provider: AiProvider::Claude,
        account: "max subscription".to_string(),
    }));
    app.handle_key(key(KeyCode::Char('a')));
    assert!(app.ai_input_mode);
    assert_eq!(app.ai_provider, AiProvider::Claude);
}

#[test]
fn assistant_focus_starts_conversation_input_on_the_first_character() {
    let mut app = app();
    app.apply_ai_event(AiEvent::ProviderReady(ProviderReadiness {
        provider: AiProvider::Claude,
        account: "max subscription".to_string(),
    }));
    app.home_focus = HomeFocus::Assistant;
    assert!(!app.ai_input_mode);

    for ch in "안녕하세요".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert!(app.ai_input_mode);
    assert_eq!(app.ai_input, "안녕하세요");
    assert!(app.take_effect().is_none());
}

#[test]
fn slash_stays_in_the_assistant_instead_of_opening_home_search() {
    let mut app = app();
    app.home_focus = HomeFocus::Assistant;

    app.handle_key(key(KeyCode::Char('/')));
    assert!(!app.search_mode);
    assert_eq!(app.home_focus, HomeFocus::Assistant);

    app.apply_ai_event(AiEvent::ProviderReady(ProviderReadiness {
        provider: AiProvider::Claude,
        account: "max subscription".to_string(),
    }));
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.ai_input_mode);
    assert_eq!(app.ai_input, "/");
    app.handle_key(key(KeyCode::Char('/')));
    assert_eq!(app.ai_input, "//");
    assert!(!app.search_mode);
}

#[test]
fn multiple_detected_ai_providers_are_selected_and_saved_in_settings() {
    let mut app = app();
    app.apply_ai_event(AiEvent::ProviderReady(ProviderReadiness {
        provider: AiProvider::Claude,
        account: "max subscription".to_string(),
    }));
    app.apply_ai_event(AiEvent::ProviderReady(ProviderReadiness {
        provider: AiProvider::Gemini,
        account: "configured credentials".to_string(),
    }));
    assert_eq!(app.ai_provider, AiProvider::Claude);

    app.handle_key(key(KeyCode::Char('7')));
    app.settings_index = 3;
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.ai_provider, AiProvider::Gemini);
    assert_eq!(app.settings.preferred_ai_provider, "gemini");

    app.handle_key(key(KeyCode::Char('1')));
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("HOME renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("2 providers ready"));
    assert!(rendered.contains("change in Settings"));
    assert!(!rendered.contains("prev"));
}

#[test]
fn termleaf_update_queues_only_the_t4e_verified_version() {
    let mut app = app();
    open_catalog_search(&mut app, "termleaf");
    app.installed_tools.insert("termleaf".to_string());
    app.apply_update_probe("termleaf", Ok("0.3.0".to_string()), "0.3.5".to_string());

    app.handle_key(key(KeyCode::Char('u')));

    let job = app.queue.last().expect("verified update queued");
    assert_eq!(job.task.expected_version.as_deref(), Some("0.3.5"));
    assert!(job.task.command.contains("v0.3.5"));
    assert!(!job.task.command.contains("/latest"));
    assert!(app.confirmation.is_some());
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
    app.handle_key(key(KeyCode::Char('I')));
    assert!(app.take_effect().is_none());

    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(matches!(app.take_effect(), Some(AppEffect::Execute(_))));
}

#[test]
fn confirm_all_uses_one_key_approval_for_safe_tools() {
    let mut app = app();
    app.settings.confirm_all_installs = true;
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('/')));
    for ch in "ripgrep".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('I')));
    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('x')));

    let confirmation = app.confirmation.as_ref().expect("confirmation shown");
    assert_eq!(confirmation.tool_id, "ripgrep");
    assert!(app.take_effect().is_none());
    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(app.take_effect(), Some(AppEffect::Execute(_))));
}

#[test]
fn danger_tool_warns_clearly_and_uses_single_enter_approval() {
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
        .find(|tool| tool.id == "btop")
        .expect("btop exists");
    let installer = tool
        .installers
        .iter()
        .find(|installer| installer.platform == platform)
        .expect("platform installer");
    let task = build_install_task(tool, installer, &InstallPolicy::default()).expect("task builds");
    app.queue.push(InstallJob::new(task, "danger-test"));
    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.confirmation.is_some());
    assert!(app.take_effect().is_none());

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("confirmation renders");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("DANGER"));
    assert!(rendered.contains("SYSTEM"));
    assert!(rendered.contains("Enter confirm"));

    app.handle_key(key(KeyCode::Enter));

    assert!(app.confirmation.is_none());
    assert!(matches!(app.take_effect(), Some(AppEffect::Execute(_))));
}

#[test]
fn stale_persisted_command_is_not_executed() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Char('I')));
    app.queue[0].task.command = "echo tampered".to_string();
    app.handle_key(key(KeyCode::Char('3')));
    app.handle_key(key(KeyCode::Char('x')));

    assert!(app.status.contains("stale"));
    assert!(app.take_effect().is_none());
}

#[test]
fn stale_saved_install_plan_is_refreshed_from_the_current_registry() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces load");
    let tool = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "asciiquarium")
        .expect("asciiquarium exists");
    let installer = tool
        .installers
        .iter()
        .find(|installer| installer.platform == Platform::Linux)
        .expect("Linux installer");
    let mut stale_task =
        build_install_task(tool, installer, &InstallPolicy::default()).expect("task builds");
    stale_task.method = InstallMethod::Apt;
    stale_task.command =
        "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get install -y asciiquarium".to_string();
    let mut stale_job = InstallJob::new(stale_task, "apt");
    stale_job.item.attempts = 3;
    stale_job.diagnostics = Some(
        t4e::installer::diagnostics::FailureDiagnostics::from_stderr(
            Some(100),
            "Unable to locate package asciiquarium",
            "/tmp/old.log",
        ),
    );
    let path = temp_state_file();
    save_state(
        &path,
        &PersistentState {
            queue: vec![stale_job],
            ..PersistentState::default()
        },
    )
    .expect("state saves");

    let app = AppState::persistent_with_install_environment(
        catalog,
        workspaces,
        path.clone(),
        InstallEnvironment::with_commands(
            Platform::current(),
            std::env::consts::ARCH,
            [
                "apt-get", "brew", "cargo", "curl", "npm", "pipx", "pkg", "snap",
            ],
        ),
    )
    .expect("state loads");
    let refreshed = &app.queue[0];
    let expected_method = if cfg!(target_os = "macos") {
        InstallMethod::Brew
    } else {
        InstallMethod::Snap
    };
    assert_eq!(refreshed.task.method, expected_method);
    assert_eq!(
        refreshed.item.channel,
        if cfg!(target_os = "macos") {
            "brew"
        } else {
            "snap"
        }
    );
    assert_eq!(refreshed.item.attempts, 0);
    assert!(refreshed.attempts.is_empty());
    assert!(refreshed.diagnostics.is_none());
    assert!(
        app.logs
            .iter()
            .any(|line| line.contains("refreshed stale install plan for asciiquarium"))
    );

    let _ = fs::remove_dir_all(path.parent().expect("state parent"));
}
