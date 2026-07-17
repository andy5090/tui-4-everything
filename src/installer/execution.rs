use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::checks::{CheckResult, InstallChecker, SystemInstallChecker};
use super::diagnostics::FailureDiagnostics;
use super::engine::InstallTask;
use super::queue::{QueueItem, QueueState};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputChunk {
    pub stream: OutputStream,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub cancelled: bool,
}

pub trait CommandRunner: Send + Sync + 'static {
    fn run(
        &self,
        command: &str,
        timeout: Duration,
        cancel: &AtomicBool,
        on_output: &mut dyn FnMut(OutputChunk),
    ) -> Result<CommandOutput>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(
        &self,
        command: &str,
        timeout: Duration,
        cancel: &AtomicBool,
        on_output: &mut dyn FnMut(OutputChunk),
    ) -> Result<CommandOutput> {
        let started = Instant::now();
        let mut process = Command::new("sh");
        process
            .args(["-c", command])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        process.process_group(0);
        let mut child = process
            .spawn()
            .with_context(|| format!("failed to start install command: {command}"))?;

        let stdout = child.stdout.take().context("missing child stdout")?;
        let stderr = child.stderr.take().context("missing child stderr")?;
        let (sender, receiver) = mpsc::channel();
        let stdout_reader = spawn_reader(stdout, OutputStream::Stdout, sender.clone());
        let stderr_reader = spawn_reader(stderr, OutputStream::Stderr, sender);

        let mut stdout_text = String::new();
        let mut stderr_text = String::new();
        let mut timed_out = false;
        let mut cancelled = false;
        let exit_status;

        loop {
            drain_output(&receiver, &mut stdout_text, &mut stderr_text, on_output);

            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                terminate_process_tree(&mut child);
                exit_status = child.wait()?;
                break;
            }
            if started.elapsed() >= timeout {
                timed_out = true;
                terminate_process_tree(&mut child);
                exit_status = child.wait()?;
                break;
            }
            if let Some(status) = child.try_wait()? {
                exit_status = status;
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }

        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        drain_output(&receiver, &mut stdout_text, &mut stderr_text, on_output);

        Ok(CommandOutput {
            exit_code: exit_status.code(),
            stdout: stdout_text,
            stderr: stderr_text,
            duration_ms: started.elapsed().as_millis() as u64,
            timed_out,
            cancelled,
        })
    }
}

#[cfg(unix)]
fn terminate_process_tree(child: &mut std::process::Child) {
    let process_group = -(child.id() as i32);
    // The child starts in its own process group, so this cannot target t4e itself.
    let result = unsafe { libc::kill(process_group, libc::SIGKILL) };
    if result != 0 {
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn terminate_process_tree(child: &mut std::process::Child) {
    let _ = child.kill();
}

fn spawn_reader<R: Read + Send + 'static>(
    reader: R,
    stream: OutputStream,
    sender: mpsc::Sender<OutputChunk>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(mut text) => {
                    text.push('\n');
                    if sender.send(OutputChunk { stream, text }).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn drain_output(
    receiver: &mpsc::Receiver<OutputChunk>,
    stdout: &mut String,
    stderr: &mut String,
    on_output: &mut dyn FnMut(OutputChunk),
) {
    while let Ok(chunk) = receiver.try_recv() {
        match chunk.stream {
            OutputStream::Stdout => stdout.push_str(&chunk.text),
            OutputStream::Stderr => stderr.push_str(&chunk.text),
        }
        on_output(chunk);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallAttempt {
    pub attempt: u32,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub cancelled: bool,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallJob {
    pub item: QueueItem,
    pub task: InstallTask,
    #[serde(default)]
    pub attempts: Vec<InstallAttempt>,
    #[serde(default)]
    pub preflight: Option<CheckResult>,
    #[serde(default)]
    pub postflight: Option<CheckResult>,
    pub diagnostics: Option<FailureDiagnostics>,
}

impl InstallJob {
    pub fn new(task: InstallTask, channel: impl Into<String>) -> Self {
        Self {
            item: QueueItem::new(task.tool_id.clone(), channel),
            task,
            attempts: Vec::new(),
            preflight: None,
            postflight: None,
            diagnostics: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub timeout: Duration,
    pub max_attempts: u32,
    pub log_dir: PathBuf,
}

impl ExecutionPolicy {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            timeout: Duration::from_secs(600),
            max_attempts: 2,
            log_dir,
        }
    }
}

pub struct InstallExecutor<R, C = SystemInstallChecker> {
    runner: R,
    checker: C,
    policy: ExecutionPolicy,
}

impl<R: CommandRunner> InstallExecutor<R, SystemInstallChecker> {
    pub fn new(runner: R, policy: ExecutionPolicy) -> Self {
        Self {
            runner,
            checker: SystemInstallChecker,
            policy,
        }
    }
}

impl<R: CommandRunner, C: InstallChecker> InstallExecutor<R, C> {
    pub fn with_checker(runner: R, checker: C, policy: ExecutionPolicy) -> Self {
        Self {
            runner,
            checker,
            policy,
        }
    }

    fn check_install(&self, task: &InstallTask) -> Result<CheckResult> {
        let mut commands = Vec::new();
        let mut resolved_paths = Vec::new();
        let mut installed = true;
        for command in task.check_commands() {
            let result = self.checker.check(command)?;
            commands.push(result.command);
            installed &= result.installed;
            if let Some(path) = result.resolved_path {
                resolved_paths.push(path);
            }
        }
        Ok(CheckResult {
            command: commands.join(", "),
            installed,
            resolved_path: installed.then(|| resolved_paths.join(", ")),
        })
    }

    pub fn execute(
        &self,
        mut job: InstallJob,
        cancel: Arc<AtomicBool>,
        mut on_output: impl FnMut(OutputChunk),
    ) -> InstallJob {
        if job.task.check_command.is_some() {
            match self.check_install(&job.task) {
                Ok(result) => {
                    let installed = result.installed;
                    job.preflight = Some(result);
                    if installed {
                        let _ = job.item.transition(QueueState::Installing);
                        let _ = job.item.transition(QueueState::Success);
                        job.diagnostics = None;
                        return job;
                    }
                }
                Err(error) => {
                    job.diagnostics = Some(FailureDiagnostics::from_stderr(
                        None,
                        &format!("preflight check failed: {error}"),
                        "",
                    ));
                }
            }
        }

        let max_attempts = self.policy.max_attempts.max(1);
        for attempt in 1..=max_attempts {
            if job.item.state == QueueState::Failed {
                let _ = job.item.transition(QueueState::Queued);
            }
            if job.item.state == QueueState::Queued {
                let _ = job.item.transition(QueueState::Installing);
            }
            job.item.mark_attempt();

            let output = self
                .runner
                .run(
                    &job.task.command,
                    self.policy.timeout,
                    &cancel,
                    &mut on_output,
                )
                .unwrap_or_else(|error| CommandOutput {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    duration_ms: 0,
                    timed_out: false,
                    cancelled: cancel.load(Ordering::Relaxed),
                });
            let log_path = write_attempt_log(
                &self.policy.log_dir,
                &job.task.tool_id,
                attempt,
                &job.task.command,
                &output,
            )
            .unwrap_or_else(|error| format!("<log write failed: {error}>"));
            job.attempts.push(InstallAttempt {
                attempt,
                exit_code: output.exit_code,
                duration_ms: output.duration_ms,
                timed_out: output.timed_out,
                cancelled: output.cancelled,
                log_path: log_path.clone(),
            });

            if output.exit_code == Some(0) && !output.timed_out && !output.cancelled {
                let verified = if job.task.check_command.is_some() {
                    match self.check_install(&job.task) {
                        Ok(result) => {
                            let installed = result.installed;
                            job.postflight = Some(result);
                            installed
                        }
                        Err(error) => {
                            job.diagnostics = Some(FailureDiagnostics::from_stderr(
                                None,
                                &format!("postflight check failed: {error}"),
                                log_path.clone(),
                            ));
                            false
                        }
                    }
                } else {
                    true
                };
                if verified {
                    let _ = job.item.transition(QueueState::Success);
                    job.diagnostics = None;
                    return job;
                }
            }

            let _ = job.item.transition(QueueState::Failed);
            let stderr = if output.exit_code == Some(0)
                && !output.timed_out
                && !output.cancelled
                && job
                    .postflight
                    .as_ref()
                    .is_some_and(|check| !check.installed)
            {
                "install command succeeded but postflight executable check failed".to_string()
            } else {
                failure_message(&output)
            };
            job.diagnostics = Some(FailureDiagnostics::from_stderr(
                output.exit_code,
                &stderr,
                log_path,
            ));
            if output.cancelled {
                return job;
            }

            if attempt < max_attempts && !cancel.load(Ordering::Relaxed) {
                let _ = job.item.transition(QueueState::Queued);
            }
        }
        job
    }
}

fn failure_message(output: &CommandOutput) -> String {
    if output.cancelled {
        "installation cancelled by user".to_string()
    } else if output.timed_out {
        "installation timed out".to_string()
    } else if output.stderr.trim().is_empty() {
        format!("installation exited with status {:?}", output.exit_code)
    } else {
        output.stderr.clone()
    }
}

fn write_attempt_log(
    log_dir: &Path,
    tool_id: &str,
    attempt: u32,
    command: &str,
    output: &CommandOutput,
) -> Result<String> {
    fs::create_dir_all(log_dir)?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let safe_id = tool_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let path = log_dir.join(format!("{safe_id}-{stamp}-attempt-{attempt}.log"));
    let content = format!(
        "command: {command}\nexit_code: {:?}\nduration_ms: {}\ntimed_out: {}\ncancelled: {}\n\n[stdout]\n{}\n[stderr]\n{}",
        output.exit_code,
        output.duration_ms,
        output.timed_out,
        output.cancelled,
        output.stdout,
        output.stderr
    );
    fs::write(&path, content)?;
    Ok(path.display().to_string())
}
