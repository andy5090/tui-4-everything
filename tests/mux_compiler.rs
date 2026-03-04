use t4e::mux::tmux::{
    CommandLog, PaneSnapshot, ReproSnapshot, WindowSnapshot, compile_workspace, reproducibility_hash,
};
use t4e::mux::workspace::{Layout, MuxBackend, Pane, SplitDirection, Workspace};

fn fake_workspace() -> Workspace {
    Workspace {
        id: "video-desk".to_string(),
        title: "Video Desk".to_string(),
        mux: MuxBackend::Tmux,
        session_name: Some("t4e-video".to_string()),
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

    assert!(output
        .commands
        .iter()
        .any(|cmd| cmd.contains("split-window -h") && cmd.contains("-P -F \"#{pane_id}\"")));
    assert!(output
        .commands
        .iter()
        .any(|cmd| cmd.contains("split-window -v") && cmd.contains("-P -F \"#{pane_id}\"")));
    assert!(output
        .commands
        .iter()
        .any(|cmd| cmd.contains("send-keys") && cmd.contains("yewtube")));
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
    assert!(output
        .commands
        .iter()
        .any(|cmd| cmd.contains("split-window -h -b")));
    assert!(output
        .commands
        .iter()
        .any(|cmd| cmd.contains("split-window -v -b")));
}
