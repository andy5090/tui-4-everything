use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use t4e::catalog::loader::load_workspaces;
use t4e::mux::runtime::{SystemTmuxRunner, TmuxOutput, TmuxRunner, TmuxRuntime};
use t4e::mux::tmux::reproducibility_hash;
use t4e::mux::workspace::{Layout, MuxBackend, Pane, SplitDirection, TmuxView, Workspace};

static TMUX_TEST_LOCK: Mutex<()> = Mutex::new(());

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
        tmux_view: TmuxView::Windows,
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

fn unique_session(label: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("t4e-{label}-{nonce}")
}

#[test]
fn launch_uses_structured_tmux_arguments() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(false, "", "missing"),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "%1\n", ""),
            output(true, "", ""),
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
    assert_eq!(launched.pane_ids, ["t4e-runtime-contract:right.0", "%1"]);
    let calls = calls.lock().expect("calls lock");
    assert!(calls.iter().any(|args| {
        args == &[
            "send-keys",
            "-t",
            "t4e-runtime-contract:right.0",
            "--",
            "printf right",
            "C-m",
        ]
    }));
    assert!(
        calls
            .iter()
            .any(|args| args.first().is_some_and(|arg| arg == "new-window"))
    );
    assert!(!calls.iter().flatten().any(|arg| arg == "split-window"));
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
fn single_app_launch_creates_a_managed_background_session() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(false, "", "missing"),
            output(true, "%7\n", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let calls = Arc::clone(&runner.calls);
    let runtime = TmuxRuntime::new(runner);

    let outcome = runtime
        .launch_app("t4e-apps", "app-launcher", "cmatrix", "cmatrix -b")
        .expect("app launches");

    assert!(outcome.created);
    assert_eq!(outcome.pane_ids, ["%7"]);
    let calls = calls.lock().expect("calls lock");
    assert!(calls.iter().any(|args| {
        args == &[
            "new-session",
            "-d",
            "-s",
            "t4e-apps",
            "-n",
            "cmatrix",
            "-P",
            "-F",
            "#{pane_id}",
            "bash",
        ]
    }));
    assert!(
        calls
            .iter()
            .any(|args| { args == &["send-keys", "-t", "%7", "--", "cmatrix -b", "C-m"] })
    );
}

#[test]
fn single_app_launch_rejects_shell_operators() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::new()),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let runtime = TmuxRuntime::new(runner);
    assert!(
        runtime
            .launch_app("t4e-apps", "app-launcher", "bad", "cmatrix; rm -rf /")
            .is_err()
    );
}

#[test]
fn real_single_app_lifecycle_works_when_tmux_and_cmatrix_are_available() {
    let _guard = TMUX_TEST_LOCK.lock().expect("tmux test lock");
    if Command::new("tmux").arg("-V").output().is_err()
        || Command::new("cmatrix").arg("-V").output().is_err()
    {
        return;
    }

    let session = unique_session("single-app");
    let runtime = TmuxRuntime::new(SystemTmuxRunner);
    let outcome = runtime
        .launch_app(&session, "app-launcher", "cmatrix", "cmatrix -b")
        .expect("real app launches");
    assert!(outcome.created);
    thread::sleep(Duration::from_millis(250));

    let apps = runtime.list_apps(&session).expect("real apps list");
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].window_name, "cmatrix");
    let content = runtime
        .capture_app(&apps[0].pane_id)
        .expect("real ANSI capture");
    assert!(!content.is_empty());
    runtime
        .close_app(&apps[0].pane_id)
        .expect("real app closes");
    assert!(!runtime.session_exists(&session).expect("session probe"));
}

#[test]
fn embedded_app_controls_use_structured_tmux_arguments() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(true, "t4e-apps\t0\t2\truntime-test\n", ""),
            output(true, "%3\t0\tright\t0\tprintf\n", ""),
            output(true, "\u{1b}[31mapp output\u{1b}[0m\n", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let calls = Arc::clone(&runner.calls);
    let runtime = TmuxRuntime::new(runner);

    assert!(runtime.capture_app("%3").is_err());
    let apps = runtime.list_apps("t4e-apps").expect("apps list");
    assert_eq!(apps[0].pane_id, "%3");
    assert_eq!(apps[0].window_name, "right");
    assert_eq!(
        runtime.capture_app("%3").expect("capture"),
        "\u{1b}[31mapp output\u{1b}[0m"
    );
    runtime.resize_app("%3", 100, 30).expect("resize app");
    runtime.send_app_text("%3", "hello").expect("send text");
    runtime.send_app_key("%3", "Enter").expect("send key");
    runtime.close_app("%3").expect("close app");

    let calls = calls.lock().expect("calls lock");
    assert!(
        calls
            .iter()
            .any(|args| { args == &["send-keys", "-l", "-t", "%3", "--", "hello"] })
    );
    assert!(
        calls
            .iter()
            .any(|args| args == &["send-keys", "-t", "%3", "Enter"])
    );
    assert!(calls.iter().any(|args| args == &["kill-pane", "-t", "%3"]));
    assert!(
        calls
            .iter()
            .any(|args| { args == &["resize-window", "-t", "%3", "-x", "100", "-y", "30"] })
    );
    assert!(!calls.iter().flatten().any(|arg| arg == "sh" || arg == "-c"));
}

#[test]
fn real_tmux_workspace_lifecycle_is_reproducible_when_available() {
    let _guard = TMUX_TEST_LOCK.lock().expect("tmux test lock");
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
    let apps = runtime.list_apps(&session).expect("embedded apps");
    assert_eq!(apps.len(), 2);
    runtime
        .send_app_text(&apps[0].pane_id, "printf '\\nembedded-control\\n'")
        .expect("send embedded text");
    runtime
        .send_app_key(&apps[0].pane_id, "Enter")
        .expect("submit embedded command");
    thread::sleep(Duration::from_millis(150));
    assert!(
        runtime
            .capture_app(&apps[0].pane_id)
            .expect("capture embedded app")
            .contains("embedded-control")
    );
    runtime
        .close_app(&apps[1].pane_id)
        .expect("close embedded app");
    assert_eq!(
        runtime.list_apps(&session).expect("remaining apps").len(),
        1
    );
    runtime.stop(&session).expect("second stop");

    assert_eq!(first.windows.len(), 2);
    assert_eq!(first.panes.len(), 2);
    assert!(
        first
            .windows
            .iter()
            .all(|window| window.window_name != "main")
    );
    assert_eq!(
        reproducibility_hash(&first, "/workspace"),
        reproducibility_hash(&second, "/workspace")
    );
}

#[test]
fn three_registry_tmux_layouts_relaunch_with_matching_live_snapshots() {
    let _guard = TMUX_TEST_LOCK.lock().expect("tmux test lock");
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

        assert_eq!(first.windows.len(), workspace.layout.panes.len());
        assert_eq!(first.panes.len(), workspace.layout.panes.len());
        assert!(
            first
                .windows
                .iter()
                .all(|window| window.window_name != "main")
        );
        for app in &workspace.layout.panes {
            assert!(
                first
                    .windows
                    .iter()
                    .any(|window| window.window_name == app.id),
                "missing app window {} for {id}",
                app.id
            );
        }

        assert_eq!(
            reproducibility_hash(&first, "/workspace"),
            reproducibility_hash(&second, "/workspace"),
            "live layout drifted for {id}"
        );
    }
}
