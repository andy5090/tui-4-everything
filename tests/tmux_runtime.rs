use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use t4e::catalog::loader::load_workspaces;
use t4e::catalog::models::OutputFilter;
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
        .launch_app_at_size("t4e-apps", "app-launcher", "cmatrix", "cmatrix -b", 132, 36)
        .expect("app launches");

    assert!(outcome.created);
    assert_eq!(outcome.pane_ids, ["%7"]);
    let calls = calls.lock().expect("calls lock");
    assert!(calls.iter().any(|args| {
        args == &[
            "new-session",
            "-d",
            "-x",
            "132",
            "-y",
            "36",
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
            .any(|args| { args == &["respawn-pane", "-k", "-t", "%7", "exec cmatrix -b"] })
    );
}

#[test]
fn filtered_one_shot_app_uses_a_live_output_holder() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(false, "", "missing"),
            output(true, "%8\n", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let calls = Arc::clone(&runner.calls);
    let runtime = TmuxRuntime::new(runner);

    runtime
        .launch_app_at_size_with_mode_and_filter(
            "t4e-apps",
            "app-launcher",
            "fortune",
            "fortune",
            80,
            24,
            true,
            Some(OutputFilter::Lolcat),
        )
        .expect("one-shot app launches");

    let calls = calls.lock().expect("calls lock");
    assert!(!calls.iter().flatten().any(|arg| arg == "remain-on-exit"));
    let launch = calls
        .iter()
        .find(|args| args.first().is_some_and(|arg| arg == "respawn-pane"))
        .expect("launch call");
    assert!(launch[4].contains("exec bash -c"));
    assert!(launch[4].contains("fortune | lolcat; status=$?"));
    assert!(launch[4].contains("while :; do sleep 86400; done"));
}

#[test]
fn multi_app_pipeline_validates_stages_before_composing_the_pipe() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(false, "", "missing"),
            output(true, "%9\n", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let calls = Arc::clone(&runner.calls);
    let runtime = TmuxRuntime::new(runner);

    runtime
        .launch_pipeline_at_size_with_mode(
            "t4e-apps",
            "app-launcher",
            "fortune-to-figlet-to-lolcat",
            &["fortune", "figlet", "lolcat"],
            80,
            24,
            true,
        )
        .expect("pipeline launches");

    let calls = calls.lock().expect("calls lock");
    let launch = calls
        .iter()
        .find(|args| args.first().is_some_and(|arg| arg == "respawn-pane"))
        .expect("launch call");
    assert!(
        launch[4].contains("fortune | figlet | lolcat; status=$?"),
        "{}",
        launch[4]
    );
}

#[test]
fn app_pipeline_rejects_shell_syntax_in_every_stage() {
    for commands in [
        ["fortune; rm -rf /", "figlet"],
        ["fortune", "figlet > /tmp/output"],
    ] {
        let runner = MockRunner {
            outputs: Mutex::new(VecDeque::new()),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let runtime = TmuxRuntime::new(runner);

        assert!(
            runtime
                .launch_pipeline_at_size_with_mode(
                    "t4e-apps",
                    "app-launcher",
                    "fortune-to-figlet",
                    &commands,
                    80,
                    24,
                    true,
                )
                .is_err()
        );
    }
}

#[test]
fn relaunch_reuses_and_revives_a_legacy_dead_pane() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(true, "", ""),
            output(true, "t4e-apps\t0\t1\tapp-launcher\n", ""),
            output(true, "t4e-apps\t0\t1\tapp-launcher\n", ""),
            output(true, "%8\t0\tlolcat\t0\tlolcat\t1\n", ""),
            output(true, "", ""),
            output(true, "", ""),
            output(true, "", ""),
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let calls = Arc::clone(&runner.calls);
    let runtime = TmuxRuntime::new(runner);

    let outcome = runtime
        .launch_app_at_size_with_mode(
            "t4e-apps",
            "app-launcher",
            "lolcat",
            "lolcat --help",
            80,
            24,
            true,
        )
        .expect("dead pane is revived");

    assert!(outcome.created);
    assert_eq!(outcome.pane_ids, ["%8"]);
    let calls = calls.lock().expect("calls lock");
    assert!(!calls.iter().flatten().any(|arg| arg == "new-window"));
    assert!(calls.iter().any(|args| {
        args.first().is_some_and(|arg| arg == "respawn-pane")
            && args.get(3).is_some_and(|target| target == "%8")
    }));
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
    let apps = runtime.list_apps(&session).expect("real apps list");
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].window_name, "cmatrix");
    let mut content = String::new();
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(50));
        content = runtime
            .capture_app(&apps[0].pane_id)
            .expect("real ANSI capture");
        if !content.is_empty() {
            break;
        }
    }
    assert!(!content.is_empty());
    runtime
        .send_app_key(&apps[0].pane_id, "C-c")
        .expect("Ctrl+C reaches app");
    for _ in 0..20 {
        if !runtime.session_exists(&session).expect("session probe") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
        let _ = runtime.send_app_key(&apps[0].pane_id, "C-c");
    }
    assert!(!runtime.session_exists(&session).expect("session probe"));
}

#[test]
fn real_one_shot_fun_apps_leave_visible_output_when_available() {
    let _guard = TMUX_TEST_LOCK.lock().expect("tmux test lock");
    let apps = [
        ("cowsay", "cowsay T4E", "T4E", None),
        ("fortune-rainbow", "fortune", "", Some(OutputFilter::Lolcat)),
    ];
    if Command::new("tmux").arg("-V").output().is_err()
        || ["cowsay", "fortune", "lolcat"].iter().any(|app| {
            !Command::new("which")
                .arg(app)
                .output()
                .is_ok_and(|output| output.status.success())
        })
    {
        return;
    }

    for (app_id, command, expected, filter) in apps {
        let session = unique_session(app_id);
        let runtime = TmuxRuntime::new(SystemTmuxRunner);
        let outcome = runtime
            .launch_app_at_size_with_mode_and_filter(
                &session,
                "app-launcher",
                app_id,
                command,
                80,
                24,
                true,
                filter,
            )
            .expect("one-shot app launches");
        let pane_id = outcome.pane_ids[0].clone();
        runtime.list_apps(&session).expect("managed app refresh");
        let mut content = String::new();
        let mut plain_content = String::new();
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(50));
            content = runtime
                .capture_app(&pane_id)
                .expect("one-shot output capture");
            plain_content = Command::new("tmux")
                .args(["capture-pane", "-p", "-t", &pane_id])
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
                .unwrap_or_default();
            if !content.trim().is_empty()
                && (expected.is_empty() || plain_content.contains(expected))
            {
                break;
            }
        }
        let pane_state = Command::new("tmux")
            .args(["display-message", "-p", "-t", &pane_id, "#{pane_dead}"])
            .output()
            .expect("pane state is readable");
        runtime.close_app(&pane_id).expect("one-shot app closes");

        assert!(!content.trim().is_empty(), "{app_id} output is visible");
        assert!(!content.contains("Pane is dead"));
        assert!(!plain_content.contains("[T4E] command exited"));
        if filter.is_some() && std::env::var_os("NO_COLOR").is_none() {
            assert!(content.contains("\u{1b}["), "lolcat emits ANSI colors");
        }
        assert_eq!(String::from_utf8_lossy(&pane_state.stdout).trim(), "0");
        assert!(
            expected.is_empty() || plain_content.contains(expected),
            "{app_id} output contains {expected}"
        );
    }
}

#[test]
fn real_curses_capture_uses_unicode_box_drawing_when_available() {
    let _guard = TMUX_TEST_LOCK.lock().expect("tmux test lock");
    if Command::new("tmux").arg("-V").output().is_err()
        || !Command::new("which")
            .arg("bastet")
            .output()
            .is_ok_and(|output| output.status.success())
    {
        return;
    }

    let session = unique_session("bastet-boxes");
    let runtime = TmuxRuntime::new(SystemTmuxRunner);
    let outcome = runtime
        .launch_app(&session, "app-launcher", "bastet", "bastet")
        .expect("bastet launches");
    let pane_id = outcome.pane_ids[0].clone();
    runtime.list_apps(&session).expect("managed app refresh");
    let mut content = String::new();
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(50));
        content = runtime
            .capture_app(&pane_id)
            .expect("curses output capture");
        if content.contains('┌') {
            break;
        }
    }
    runtime.close_app(&pane_id).expect("bastet closes");

    assert!(content.contains('┌'));
    assert!(!content.contains("lqqqq"));
}

#[test]
fn embedded_app_controls_use_structured_tmux_arguments() {
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(true, "t4e-apps\t0\t2\truntime-test\n", ""),
            output(true, "%3\t0\tright\t0\tprintf\t0\n", ""),
            output(true, "\u{1b}[31mrow one\u{1b}[0m\nrow two\n", ""),
            output(true, "joined app output\n", ""),
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
        "\u{1b}[31mrow one\u{1b}[0m\nrow two"
    );
    assert_eq!(
        runtime.capture_app_joined("%3").expect("joined capture"),
        "joined app output"
    );
    runtime.resize_app("%3", 100, 30).expect("resize app");
    runtime.send_app_text("%3", "hello").expect("send text");
    runtime.send_app_key("%3", "Enter").expect("send key");
    runtime.close_app("%3").expect("close app");

    let calls = calls.lock().expect("calls lock");
    assert!(
        calls
            .iter()
            .any(|args| { args == &["capture-pane", "-p", "-e", "-t", "%3"] })
    );
    assert!(
        calls
            .iter()
            .any(|args| { args == &["capture-pane", "-p", "-e", "-J", "-t", "%3"] })
    );
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
