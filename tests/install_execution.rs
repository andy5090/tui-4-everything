use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use t4e::catalog::models::InstallMethod;
use t4e::installer::checks::{CheckResult, InstallChecker};
use t4e::installer::engine::InstallTask;
use t4e::installer::execution::{
    CommandOutput, CommandRunner, ExecutionPolicy, InstallExecutor, InstallJob, OutputChunk,
    OutputStream, SystemCommandRunner,
};
use t4e::installer::queue::QueueState;

struct MockRunner {
    outputs: Mutex<VecDeque<CommandOutput>>,
}

struct MockChecker {
    results: Mutex<VecDeque<bool>>,
}

impl InstallChecker for MockChecker {
    fn check(&self, command: &str) -> anyhow::Result<CheckResult> {
        let installed = self
            .results
            .lock()
            .expect("mock check lock")
            .pop_front()
            .expect("mock check result");
        Ok(CheckResult {
            command: command.to_string(),
            installed,
            resolved_path: installed.then(|| format!("/mock/bin/{command}")),
        })
    }
}

impl CommandRunner for MockRunner {
    fn run(
        &self,
        _command: &str,
        _timeout: Duration,
        _cancel: &AtomicBool,
        on_output: &mut dyn FnMut(OutputChunk),
    ) -> anyhow::Result<CommandOutput> {
        let output = self
            .outputs
            .lock()
            .expect("mock lock")
            .pop_front()
            .expect("mock output");
        if !output.stdout.is_empty() {
            on_output(OutputChunk {
                stream: OutputStream::Stdout,
                text: output.stdout.clone(),
            });
        }
        if !output.stderr.is_empty() {
            on_output(OutputChunk {
                stream: OutputStream::Stderr,
                text: output.stderr.clone(),
            });
        }
        Ok(output)
    }
}

fn task() -> InstallTask {
    InstallTask {
        tool_id: "test-tool".to_string(),
        method: InstallMethod::Apt,
        command: "install test-tool".to_string(),
        check_command: None,
        requires_privileges: false,
        requires_confirmation: false,
        queued_at: Utc::now(),
    }
}

fn output(exit_code: i32, stdout: &str, stderr: &str) -> CommandOutput {
    CommandOutput {
        exit_code: Some(exit_code),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
        duration_ms: 12,
        timed_out: false,
        cancelled: false,
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("t4e-{label}-{}-{nonce}", std::process::id()))
}

#[test]
fn executor_retries_failure_and_persists_attempt_logs() {
    let log_dir = temp_dir("retry");
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([
            output(1, "", "temporary failure\n"),
            output(0, "installed\n", ""),
        ])),
    };
    let executor = InstallExecutor::new(
        runner,
        ExecutionPolicy {
            timeout: Duration::from_secs(1),
            max_attempts: 2,
            log_dir: log_dir.clone(),
        },
    );
    let mut streamed = Vec::new();

    let completed = executor.execute(
        InstallJob::new(task(), "apt"),
        Arc::new(AtomicBool::new(false)),
        |chunk| streamed.push(chunk),
    );

    assert_eq!(completed.item.state, QueueState::Success);
    assert_eq!(completed.item.attempts, 2);
    assert_eq!(completed.attempts.len(), 2);
    assert!(completed.diagnostics.is_none());
    assert!(
        streamed
            .iter()
            .any(|chunk| chunk.text.contains("installed"))
    );
    assert!(
        completed
            .attempts
            .iter()
            .all(|attempt| PathBuf::from(&attempt.log_path).exists())
    );
    let _ = fs::remove_dir_all(log_dir);
}

#[test]
fn executor_returns_diagnostics_after_retry_budget() {
    let log_dir = temp_dir("failure");
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([output(
            7,
            "",
            "package was not found\ncheck repository\n",
        )])),
    };
    let executor = InstallExecutor::new(
        runner,
        ExecutionPolicy {
            timeout: Duration::from_secs(1),
            max_attempts: 1,
            log_dir: log_dir.clone(),
        },
    );

    let completed = executor.execute(
        InstallJob::new(task(), "apt"),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    );

    assert_eq!(completed.item.state, QueueState::Failed);
    let diagnostics = completed.diagnostics.expect("failure diagnostics");
    assert_eq!(diagnostics.exit_code, Some(7));
    assert!(diagnostics.stderr_summary.contains("package was not found"));
    assert!(PathBuf::from(diagnostics.full_log_path).exists());
    let _ = fs::remove_dir_all(log_dir);
}

#[test]
fn system_runner_enforces_timeout_and_cancellation() {
    let runner = SystemCommandRunner;
    let mut chunks = Vec::new();
    let timeout_result = runner
        .run(
            "while :; do :; done",
            Duration::from_millis(50),
            &AtomicBool::new(false),
            &mut |chunk| chunks.push(chunk),
        )
        .expect("timeout command returns");
    assert!(timeout_result.timed_out);

    let cancel = AtomicBool::new(true);
    let cancelled_result = runner
        .run(
            "while :; do :; done",
            Duration::from_secs(1),
            &cancel,
            &mut |_| {},
        )
        .expect("cancelled command returns");
    assert!(cancelled_result.cancelled);
}

#[test]
fn preflight_skips_install_when_executable_is_already_present() {
    let log_dir = temp_dir("preflight");
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::new()),
    };
    let checker = MockChecker {
        results: Mutex::new(VecDeque::from([true])),
    };
    let executor = InstallExecutor::with_checker(
        runner,
        checker,
        ExecutionPolicy {
            timeout: Duration::from_secs(1),
            max_attempts: 1,
            log_dir,
        },
    );
    let mut install_task = task();
    install_task.check_command = Some("test-tool".to_string());

    let completed = executor.execute(
        InstallJob::new(install_task, "apt"),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    );

    assert_eq!(completed.item.state, QueueState::Success);
    assert_eq!(completed.item.attempts, 0);
    assert!(completed.attempts.is_empty());
    assert!(completed.preflight.expect("preflight result").installed);
}

#[test]
fn successful_command_fails_when_postflight_executable_is_missing() {
    let log_dir = temp_dir("postflight");
    let runner = MockRunner {
        outputs: Mutex::new(VecDeque::from([output(0, "installed\n", "")])),
    };
    let checker = MockChecker {
        results: Mutex::new(VecDeque::from([false, false])),
    };
    let executor = InstallExecutor::with_checker(
        runner,
        checker,
        ExecutionPolicy {
            timeout: Duration::from_secs(1),
            max_attempts: 1,
            log_dir: log_dir.clone(),
        },
    );
    let mut install_task = task();
    install_task.check_command = Some("test-tool".to_string());

    let completed = executor.execute(
        InstallJob::new(install_task, "apt"),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    );

    assert_eq!(completed.item.state, QueueState::Failed);
    assert!(!completed.postflight.expect("postflight result").installed);
    assert!(
        completed
            .diagnostics
            .expect("diagnostics")
            .stderr_summary
            .contains("postflight")
    );
    let _ = fs::remove_dir_all(log_dir);
}
