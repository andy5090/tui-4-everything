use t4e::mux::tmux::{
    CommandLog, PaneSnapshot, ReproSnapshot, WindowSnapshot, compile_workspace,
    reproducibility_hash,
};
use t4e::mux::workspace::{Layout, MuxBackend, Pane, SplitDirection, TmuxView, Workspace};
use t4e::mux::zellij::render_layout_kdl;

fn fake_workspace() -> Workspace {
    Workspace {
        id: "video-desk".to_string(),
        title: "Video Desk".to_string(),
        mux: MuxBackend::Tmux,
        session_name: Some("t4e-video".to_string()),
        tmux_view: TmuxView::Panes,
        recommended_tools: vec!["yewtube".to_string()],
        layout: Layout {
            panes: vec![
                Pane {
                    id: "left".to_string(),
                    split: "root".to_string(),
                    direction: SplitDirection::Right,
                    size: "40%".to_string(),
                    cmd: "yewtube".to_string(),
                },
                Pane {
                    id: "player".to_string(),
                    split: "left".to_string(),
                    direction: SplitDirection::Down,
                    size: "50%".to_string(),
                    cmd: "mpv".to_string(),
                },
            ],
        },
    }
}

#[test]
fn compiler_tracks_pane_ids_with_tmux_print_mode() {
    let workspace = fake_workspace();
    let output = compile_workspace(&workspace, "session", "win").expect("compile ok");

    assert!(
        output
            .commands
            .iter()
            .any(|cmd| cmd.contains("split-window -h")
                && cmd.contains("-l 40%")
                && cmd.contains("-P -F \"#{pane_id}\""))
    );
    assert!(
        output
            .commands
            .iter()
            .any(|cmd| cmd.contains("split-window -v") && cmd.contains("-P -F \"#{pane_id}\""))
    );
    assert!(
        output
            .commands
            .iter()
            .any(|cmd| cmd.contains("send-keys") && cmd.contains("yewtube"))
    );
    assert!(
        output
            .commands
            .iter()
            .any(|cmd| { cmd.contains("new-session") && cmd.ends_with("\"sh\"") })
    );
    assert!(
        output
            .commands
            .iter()
            .filter(|cmd| cmd.contains("split-window"))
            .all(|cmd| cmd.ends_with("\"sh\")"))
    );
    assert!(output.commands.iter().all(|cmd| !cmd.contains("\"bash\"")));
}

#[test]
fn default_window_view_compiles_one_full_screen_window_per_app() {
    let mut workspace = fake_workspace();
    workspace.tmux_view = TmuxView::Windows;
    let output = compile_workspace(&workspace, "session", "main").expect("compile ok");

    assert!(
        output
            .commands
            .iter()
            .any(|command| command.contains("rename-window") && command.contains("left"))
    );
    assert!(
        output
            .commands
            .iter()
            .any(|command| command.contains("new-window") && command.contains("player"))
    );
    assert!(
        output
            .commands
            .iter()
            .all(|command| !command.contains("split-window"))
    );
    assert_eq!(output.focus_target, "session:left");
}

#[test]
fn reproducibility_hash_is_stable_for_equivalent_snapshots() {
    let a = ReproSnapshot {
        windows: vec![WindowSnapshot {
            window_index: 0,
            window_name: "main".to_string(),
            window_layout: "abcd,120x40,0,0".to_string(),
        }],
        panes: vec![PaneSnapshot {
            window_index: 0,
            pane_index: 1,
            pane_width: 120,
            pane_height: 20,
            pane_start_command: "/Users/andy/Projects/tui-4-everything/bin/run".to_string(),
        }],
        commands: vec![CommandLog {
            window_index: 0,
            pane_index: 1,
            sequence: 1,
            command: "/Users/andy/Projects/tui-4-everything/script.sh".to_string(),
        }],
    };

    let b = ReproSnapshot {
        windows: vec![WindowSnapshot {
            window_index: 0,
            window_name: " main ".to_string(),
            window_layout: "abcd,120x40,0,0".to_string(),
        }],
        panes: vec![PaneSnapshot {
            window_index: 0,
            pane_index: 1,
            pane_width: 120,
            pane_height: 20,
            pane_start_command: " /Users/andy/Projects/tui-4-everything/bin/run ".to_string(),
        }],
        commands: vec![CommandLog {
            window_index: 0,
            pane_index: 1,
            sequence: 1,
            command: " /Users/andy/Projects/tui-4-everything/script.sh ".to_string(),
        }],
    };

    let hash_a = reproducibility_hash(&a, "/Users/andy/Projects/tui-4-everything");
    let hash_b = reproducibility_hash(&b, "/Users/andy/Projects/tui-4-everything");
    assert_eq!(hash_a, hash_b);
}

#[test]
fn compiler_preserves_left_up_semantics_with_before_flag() {
    let mut workspace = fake_workspace();
    workspace.layout.panes = vec![
        Pane {
            id: "left".to_string(),
            split: "root".to_string(),
            direction: SplitDirection::Left,
            size: "40%".to_string(),
            cmd: "echo left".to_string(),
        },
        Pane {
            id: "up".to_string(),
            split: "left".to_string(),
            direction: SplitDirection::Up,
            size: "50%".to_string(),
            cmd: "echo up".to_string(),
        },
    ];

    let output = compile_workspace(&workspace, "session", "win").expect("compile ok");
    assert!(
        output
            .commands
            .iter()
            .any(|cmd| cmd.contains("split-window -h -b"))
    );
    assert!(
        output
            .commands
            .iter()
            .any(|cmd| cmd.contains("split-window -v -b"))
    );
}

#[test]
fn compiler_rejects_non_percent_sizes() {
    let mut workspace = fake_workspace();
    workspace.layout.panes[0].size = "50".to_string();
    let result = compile_workspace(&workspace, "session", "win");
    assert!(result.is_err());
}

#[test]
fn compiler_escapes_shell_sensitive_pane_commands() {
    let mut workspace = fake_workspace();
    workspace.layout.panes[0].cmd = "echo 'unsafe' && touch /tmp/pwn".to_string();
    let output = compile_workspace(&workspace, "session", "win").expect("compile ok");
    assert!(
        output
            .commands
            .iter()
            .any(|cmd| cmd.contains("send-keys") && cmd.contains("'\"'\"'unsafe'\"'\"'"))
    );
}

#[test]
fn zellij_layout_keeps_split_direction_and_size_metadata() {
    let workspace = fake_workspace();
    let rendered = render_layout_kdl(&workspace).expect("render ok");
    assert!(rendered.contains("direction=\"Right\""));
    assert!(rendered.contains("size=\"40%\""));
    assert!(rendered.contains("split=\"root\""));
}
