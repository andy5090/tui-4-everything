use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::json;
#[cfg(unix)]
use t4e::adapters::MpvAdapter;
use t4e::adapters::{AppAction, AppAdapter, TmuxAppAdapter, TmuxAppKind, audited_execute};
use t4e::mux::runtime::{TmuxOutput, TmuxRunner};

struct MockTmux {
    outputs: Mutex<VecDeque<TmuxOutput>>,
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl TmuxRunner for MockTmux {
    fn run(&self, args: &[String]) -> anyhow::Result<TmuxOutput> {
        self.calls.lock().expect("calls").push(args.to_vec());
        Ok(self
            .outputs
            .lock()
            .expect("outputs")
            .pop_front()
            .expect("mock output"))
    }
}

fn output(stdout: &str) -> TmuxOutput {
    TmuxOutput {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

#[test]
fn yazi_adapter_observes_verified_pane_and_sends_only_allowlisted_keys() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let runner = MockTmux {
        outputs: Mutex::new(VecDeque::from([
            output("yazi\n"),
            output("file-a\nfile-b\n"),
            output("yazi\n"),
            output(""),
        ])),
        calls: Arc::clone(&calls),
    };
    let adapter = TmuxAppAdapter::new(runner, TmuxAppKind::Yazi, "%7");

    let observation = adapter.observe().expect("observes yazi");
    assert!(
        observation.state["screen"]
            .as_str()
            .expect("screen")
            .contains("file-b")
    );
    let result = adapter
        .execute(&AppAction {
            name: "down".to_string(),
            arguments: json!({}),
        })
        .expect("moves down");
    assert_eq!(result.compensating_action.expect("undo").name, "up");
    assert_eq!(
        calls.lock().expect("calls").last().expect("send keys"),
        &["send-keys", "-t", "%7", "j"]
    );
}

#[test]
fn newsboat_adapter_fails_closed_on_wrong_process_and_audits_failure() {
    let runner = MockTmux {
        outputs: Mutex::new(VecDeque::from([output("bash\n")])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let adapter = TmuxAppAdapter::new(runner, TmuxAppKind::Newsboat, "%8");
    let mut audit = Vec::new();

    let result = audited_execute(
        &adapter,
        &AppAction {
            name: "reload".to_string(),
            arguments: json!({}),
        },
        &mut audit,
    );

    assert!(result.is_err());
    assert_eq!(audit.len(), 1);
    assert!(!audit[0].success);
    assert!(
        audit[0]
            .error
            .as_deref()
            .expect("error")
            .contains("expected newsboat")
    );
}

#[cfg(unix)]
#[test]
fn mpv_adapter_uses_json_ipc_and_returns_compensating_seek() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let socket = std::env::temp_dir().join(format!("t4e-mpv-{nonce}.sock"));
    let listener = UnixListener::bind(&socket).expect("socket binds");
    let received = Arc::new(Mutex::new(Vec::new()));
    let server_received = Arc::clone(&received);
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("connection");
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("clone"))
            .read_line(&mut request)
            .expect("request");
        server_received.lock().expect("received").push(request);
        writeln!(stream, "{}", json!({ "error": "success", "data": null })).expect("response");
    });
    let adapter = MpvAdapter::new(socket.display().to_string());

    let result = adapter
        .execute(&AppAction {
            name: "seek_relative".to_string(),
            arguments: json!({ "seconds": 15.0 }),
        })
        .expect("seek executes");
    server.join().expect("server joins");

    assert_eq!(
        result.compensating_action.expect("undo").arguments["seconds"],
        -15.0
    );
    let command: serde_json::Value =
        serde_json::from_str(&received.lock().expect("received")[0]).expect("request JSON");
    assert_eq!(command["command"], json!(["seek", 15.0, "relative"]));
    let _ = std::fs::remove_file(socket);
}
