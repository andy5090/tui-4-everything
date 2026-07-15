use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use t4e::catalog::loader::load_workspaces;
use t4e::mux::runtime::{SystemTmuxRunner, TmuxOutput, TmuxRunner, TmuxRuntime};
use t4e::mux::tmux::reproducibility_hash;
use t4e::mux::workspace::{Layout, MuxBackend, Pane, SplitDirection, Workspace};

struct MockRunner {
    outputs: Mutex<VecDeque<TmuxOutput>>,
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl TmuxRunner for MockRunner {
    fn run(&self, args: &[String]) -> anyhow::Result<TmuxOutput> {
        self.calls.lock().expect("calls lock").push(args.to_vec());
        Ok(self
            .outputs
            .lock()
            .expect("outputs lock")
            .pop_front()
            .expect("mock output"))
    }
}

fn output(success: bool, stdout: &str, stderr: &str) -> TmuxOutput {
    TmuxOutput {
        success,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn workspace(session_name: &str) -> Workspace {
    Workspace {
        id: "runtime-test".to_string(),
        title: "Runtime Test".to_string(),
        mux: MuxBackend::Tmux,
        session_name: Some(session_name.to_string()),
        recommended_tools: Vec::new(),
        layout: Layout {
            panes: vec![
                Pane {
                    id: "right".to_string(),
                    split: "root".to_string(),
                    direction: SplitDirection::Right,
                    size: "50%".to_string(),
                    cmd: "printf right".to_string(),
                },
                Pane {
                    id: "lower".to_string(),
                    split: "right".to_string(),
                    direction: SplitDirection::Down,
                    size: "50%".to_string(),
                    cmd: "printf lower".to_string(),
                },
            ],
        },
    }
}

#[test]
fn launch_uses_structured_tmux_arguments() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(false, "", "missing"),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "%1\n", ""),
            output(true, "", ""),
            output(true, "%2\n", ""),
            output(true, "", ""),
            output(true, "", ""),
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let calls = Arc::clone(&runner.calls);
    let runtime = TmuxRuntime::new(runner);

    let launched = runtime
        .launch(&workspace("t4e-runtime-contract"))
        .expect("workspace launches");

    assert!(launched.created);
    assert_eq!(launched.pane_ids, ["%1", "%2"]);
    let calls = calls.lock().expect("calls lock");
    assert!(
        calls
            .iter()
            .any(|args| { args == &["send-keys", "-t", "%1", "--", "printf right", "C-m",] })
    );
    assert!(
        calls
            .iter()
            .any(|args| args.windows(2).any(|pair| pair == ["-l", "50%"]))
    );
    assert!(!calls.iter().flatten().any(|arg| arg == "sh" || arg == "-c"));
}

#[test]
fn launch_failure_removes_partial_session() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(false, "", "missing"),
            output(true, "", ""),
            output(true, "", ""),
            output(false, "", "split failed"),
            output(true, "", ""),
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let calls = Arc::clone(&runner.calls);
    let runtime = TmuxRuntime::new(runner);

    let error = runtime
        .launch(&workspace("t4e-runtime-failure"))
        .expect_err("launch should fail");

    assert!(error.to_string().contains("split failed"));
    assert_eq!(
        calls
            .lock()
            .expect("calls lock")
            .last()
            .expect("cleanup call"),
        &["kill-session", "-t", "t4e-runtime-failure"]
    );
}

#[test]
fn real_tmux_workspace_lifecycle_is_reproducible_when_available() {
    if Command::new("tmux").arg("-V").output().is_err() {
        return;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let session = format!("t4e-test-{nonce}");
    let workspace = workspace(&session);
    let runtime = TmuxRuntime::new(SystemTmuxRunner);
    let _ = runtime.stop(&session);

    runtime.launch(&workspace).expect("first launch");
    assert!(
        runtime
            .list_managed()
            .expect("managed sessions")
            .iter()
            .any(|managed| managed.name == session && managed.workspace_id == "runtime-test")
    );
    let first = runtime.snapshot(&workspace).expect("first snapshot");
    runtime.stop(&session).expect("first stop");
    runtime.launch(&workspace).expect("second launch");
    let second = runtime.snapshot(&workspace).expect("second snapshot");
    runtime.stop(&session).expect("second stop");

    assert_eq!(first.windows.len(), 1);
    assert_eq!(first.panes.len(), 3);
    assert_eq!(
        reproducibility_hash(&first, "/workspace"),
        reproducibility_hash(&second, "/workspace")
    );
}

#[test]
fn three_registry_tmux_layouts_relaunch_with_matching_live_snapshots() {
    if Command::new("tmux").arg("-V").output().is_err() {
        return;
    }
    let registry = load_workspaces(std::path::Path::new("registry/workspaces.yaml"))
        .expect("workspace registry");
    let runtime = TmuxRuntime::new(SystemTmuxRunner);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    for id in ["video-desk", "music-desk", "fun-desk"] {
        let mut workspace = registry
            .workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .expect("registry workspace")
            .clone();
        let session = format!("t4e-{id}-{nonce}");
        workspace.session_name = Some(session.clone());
        for pane in &mut workspace.layout.panes {
            pane.cmd = format!("printf 't4e {}'", pane.id);
        }
        let _ = runtime.stop(&session);

        runtime.launch(&workspace).expect("first registry launch");
        let first = runtime.snapshot(&workspace).expect("first live snapshot");
        runtime.stop(&session).expect("first registry stop");
        runtime.launch(&workspace).expect("second registry launch");
        let second = runtime.snapshot(&workspace).expect("second live snapshot");
        runtime.stop(&session).expect("second registry stop");

        assert_eq!(
            reproducibility_hash(&first, "/workspace"),
            reproducibility_hash(&second, "/workspace"),
            "live layout drifted for {id}"
        );
    }
}
