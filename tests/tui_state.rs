use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use t4e::app::events::Screen;
use t4e::app::state::{AppEffect, AppState};
use t4e::app::ui::render;
use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::models::{InstallMethod, Platform};
use t4e::codex::service::CodexEvent;
use t4e::installer::checks::CheckResult;
use t4e::installer::engine::{InstallPolicy, build_install_task};
use t4e::installer::execution::{InstallAttempt, InstallJob};
use t4e::installer::queue::QueueState;
use t4e::mux::runtime::ManagedApp;
use t4e::storage::{PersistentState, save_state};

fn app() -> AppState {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces load");
    AppState::new(catalog, workspaces)
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

#[test]
fn tab_switches_header_sections_outside_running_apps() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.screen, Screen::Catalog);
    assert_eq!(app.catalog_index, 1);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.screen, Screen::Workspace);
    assert_eq!(app.catalog_index, 1);

    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Home);
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
    app.pack_index = app
        .catalog
        .packs
        .iter()
        .position(|pack| pack.id == "fun-pack")
        .expect("fun pack exists");
    app.handle_key(key(KeyCode::Enter));
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

    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::LaunchTool(request))
            if request.tool_id == "tplay"
                && request.command == "t4e-tplay 'https://example.com/watch?q=a'\"'\"'b;echo unsafe'"
    ));
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
    assert_eq!(request.method, InstallMethod::Tplay);
    assert!(request.command.contains("t4e-tplay"));
    assert!(request.command.contains("/t4e/tplay"));
    assert!(request.command.contains("cargo uninstall tplay"));
    assert_eq!(request.check_command, "t4e-tplay");
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
    assert!(
        confirmation
            .command
            .contains("cargo uninstall \"$package\"")
    );
    assert!(confirmation.command.contains("termusic termusic-server"));

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
    for (tool_id, installed_probe) in [
        ("lynx", "dpkg-query"),
        ("asciiquarium", "snap list"),
        ("yewtube", "pipx list --short"),
        ("youtube-tui", "cargo uninstall"),
    ] {
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
    assert_eq!(
        request.command,
        "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 remove -y ripgrep"
    );
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
            ("yewtube".to_string(), "yt".to_string()),
            ("mpv".to_string(), "mpv".to_string()),
            ("yazi".to_string(), "yazi".to_string()),
        ]
    );
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
    assert!(rendered.contains("[Alt-Q] Close"));
    assert!(rendered.contains("[Alt-BS] Background"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Red),
        "ANSI red should be preserved in App View"
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
    app.app_view.as_mut().expect("app view").content = format!(
        "Using redirect URI: http://127.0.0.1:8989/login\nOpen this URL:\n{auth_url}\nNew URL:\n{latest_url}\nWaiting..."
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT));
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
    assert_eq!(app.screen, Screen::Catalog);
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

    assert_eq!(app.screen, Screen::Catalog);
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
    app.pack_index = app
        .catalog
        .packs
        .iter()
        .position(|pack| pack.id == "fun-pack")
        .expect("fun pack exists");
    app.handle_key(key(KeyCode::Enter));

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.screen, Screen::AppView);
    assert_eq!(app.app_view.as_ref().expect("app view").selected, 0);
    assert!(app.launch_options.is_none());
    assert!(app.take_effect().is_none());
}

#[test]
fn mouse_selects_lists_switches_tabs_and_closes_from_the_footer() {
    let mut app = app();
    app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT));
    assert!(matches!(
        app.take_effect(),
        Some(AppEffect::SetMouseCapture(true))
    ));
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 6,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.pack_index, 2);
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 3,
            row: 6,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.pack_index, 3);

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
            column: 70,
            row: 23,
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
    app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT));
    app.take_effect();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.screen, Screen::Workspace);

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
            column: 50,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        24,
    );
    assert_eq!(app.screen, Screen::Help);
}

#[test]
fn closing_an_app_returns_to_the_pack_it_was_launched_from() {
    let mut app = app();
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Catalog);
    let pack = app.active_pack.clone();
    app.open_app_view(
        "t4e-pack-return".to_string(),
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

    assert_eq!(app.screen, Screen::Catalog);
    assert_eq!(app.active_pack, pack);
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
    assert!(rendered.contains("Packs"));
    assert!(rendered.contains("Workspaces"));
    assert!(rendered.contains("Activity"));
    assert!(rendered.contains("Settings"));
    assert!(rendered.contains("Help"));
    assert!(rendered.contains("Backspace back"));
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

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.screen, Screen::Home);
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.screen, Screen::Help);
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
    assert!(
        app.visible_catalog_tools()
            .iter()
            .all(|tool| tool.is_launchable_app())
    );
    assert!(
        !app.visible_catalog_tools()
            .iter()
            .any(|tool| tool.id == "mpv")
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
fn agents_pack_opens_its_three_agent_apps() {
    let mut app = app();
    app.pack_index = app
        .catalog
        .packs
        .iter()
        .position(|pack| pack.id == "agents-pack")
        .expect("agents pack exists");

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.active_pack.as_deref(), Some("agents-pack"));
    let visible = app.visible_catalog_tools();
    assert_eq!(visible.len(), 3);
    assert!(
        visible
            .iter()
            .all(|tool| tool.category == t4e::catalog::models::ToolCategory::Agents)
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
            .any(|line| line == "install: ripgrep already installed")
    );
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
fn ai_context_describes_catalog_queue_and_live_workspace_state() {
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

    assert!(context.contains("platform: linux"));
    assert!(context.contains("yazi=Yazi (run: yazi)"));
    assert!(context.contains("yazi:Queued"));
    assert!(context.contains("video-desk=Video Desk"));
    assert!(context.contains("state: running as t4e-video"));
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
    assert!(!confirmation.typed);
    assert!(app.take_effect().is_none());
    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(app.take_effect(), Some(AppEffect::Execute(_))));
}

#[test]
fn danger_tool_requires_typed_confirmation() {
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
    assert!(app.confirmation.as_ref().is_some_and(|value| value.typed));
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

    let app = AppState::persistent(catalog, workspaces, path.clone()).expect("state loads");
    let refreshed = &app.queue[0];
    assert_eq!(refreshed.task.method, InstallMethod::Snap);
    assert_eq!(refreshed.item.channel, "snap");
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
