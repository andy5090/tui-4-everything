use std::path::Path;

use t4e::catalog::loader::load_workspaces;
use t4e::mux::tmux::{
    CommandLog, PaneSnapshot, ReproSnapshot, WindowSnapshot, compile_workspace,
    reproducibility_hash,
};
use t4e::mux::workspace::MuxBackend;

#[test]
fn three_tmux_workspaces_compile_reproducibly_twice() {
    let model =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("load workspace registry");
    let target_ids = ["video-desk", "music-desk", "fun-desk"];

    for id in target_ids {
        let ws = model
            .workspaces
            .iter()
            .find(|ws| ws.id == id)
            .expect("workspace exists");
        assert!(matches!(ws.mux, MuxBackend::Tmux));

        let run1 = compile_workspace(ws, ws.session_name.as_deref().unwrap_or("t4e"), "main")
            .expect("compile 1");
        let run2 = compile_workspace(ws, ws.session_name.as_deref().unwrap_or("t4e"), "main")
            .expect("compile 2");

        assert_eq!(
            run1.commands, run2.commands,
            "workspace {} compile output drifted",
            id
        );

        let snapshot1 = ReproSnapshot {
            windows: vec![WindowSnapshot {
                window_index: 0,
                window_name: "main".to_string(),
                window_layout: "layout".to_string(),
            }],
            panes: ws
                .layout
                .panes
                .iter()
                .enumerate()
                .map(|(idx, pane)| PaneSnapshot {
                    window_index: 0,
                    pane_index: idx,
                    pane_width: 80,
                    pane_height: 24,
                    pane_start_command: pane.cmd.clone(),
                })
                .collect(),
            commands: run1
                .commands
                .iter()
                .enumerate()
                .map(|(idx, command)| CommandLog {
                    window_index: 0,
                    pane_index: 0,
                    sequence: idx,
                    command: command.clone(),
                })
                .collect(),
        };
        let snapshot2 = ReproSnapshot {
            windows: snapshot1.windows.clone(),
            panes: snapshot1.panes.clone(),
            commands: run2
                .commands
                .iter()
                .enumerate()
                .map(|(idx, command)| CommandLog {
                    window_index: 0,
                    pane_index: 0,
                    sequence: idx,
                    command: command.clone(),
                })
                .collect(),
        };
        let hash1 = reproducibility_hash(&snapshot1, "/workspace");
        let hash2 = reproducibility_hash(&snapshot2, "/workspace");
        assert_eq!(hash1, hash2, "workspace {} repro hash drifted", id);
    }
}
