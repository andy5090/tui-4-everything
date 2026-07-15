use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct CodexAppServer {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    backlog: VecDeque<Value>,
    next_id: u64,
    initialized: bool,
}

impl CodexAppServer {
    pub fn spawn() -> Result<Self> {
        Self::spawn_command("codex", &["app-server", "--listen", "stdio://"])
    }

    pub fn spawn_command(program: &str, args: &[&str]) -> Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to start {program}"))?;
        let stdin = child.stdin.take().context("app-server stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("app-server stdout unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("app-server stderr unavailable")?;
        let (sender, messages) = mpsc::channel();
        let stdout_sender = sender.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let message = serde_json::from_str::<Value>(&line).unwrap_or_else(|error| {
                    json!({
                        "method": "t4e/protocolError",
                        "params": { "message": error.to_string(), "line": line }
                    })
                });
                if stdout_sender.send(message).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if sender
                    .send(json!({ "method": "t4e/stderr", "params": { "message": line } }))
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            messages,
            backlog: VecDeque::new(),
            next_id: 0,
            initialized: false,
        })
    }

    pub fn initialize(&mut self) -> Result<Value> {
        if self.initialized {
            bail!("Codex app-server connection is already initialized");
        }
        let result = self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "t4e",
                    "title": "t4e Terminal Environment",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": false }
            }),
            REQUEST_TIMEOUT,
        )?;
        self.notify("initialized", json!({}))?;
        self.initialized = true;
        Ok(result)
    }

    pub fn account_read(&mut self) -> Result<Value> {
        self.ensure_initialized()?;
        self.request("account/read", json!({}), REQUEST_TIMEOUT)
    }

    pub fn start_thread(&mut self, cwd: &str) -> Result<String> {
        self.ensure_initialized()?;
        let result = self.request(
            "thread/start",
            json!({
                "cwd": cwd,
                "sandbox": "read-only",
                "approvalPolicy": "never",
                "ephemeral": false
            }),
            REQUEST_TIMEOUT,
        )?;
        result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("thread/start response did not contain thread.id")
    }

    pub fn start_turn(&mut self, thread_id: &str, prompt: &str) -> Result<String> {
        self.start_turn_inner(thread_id, prompt, None, None)
    }

    pub fn start_turn_structured(
        &mut self,
        thread_id: &str,
        prompt: &str,
        output_schema: Value,
    ) -> Result<String> {
        self.start_turn_inner(thread_id, prompt, Some(output_schema), None)
    }

    pub fn start_turn_structured_with_model(
        &mut self,
        thread_id: &str,
        prompt: &str,
        output_schema: Value,
        model: &str,
    ) -> Result<String> {
        self.start_turn_inner(thread_id, prompt, Some(output_schema), Some(model))
    }

    fn start_turn_inner(
        &mut self,
        thread_id: &str,
        prompt: &str,
        output_schema: Option<Value>,
        model: Option<&str>,
    ) -> Result<String> {
        self.ensure_initialized()?;
        let mut params = json!({
            "threadId": thread_id,
            "input": [{ "type": "text", "text": prompt }],
            "approvalPolicy": "never"
        });
        if let Some(output_schema) = output_schema {
            params["outputSchema"] = output_schema;
        }
        if let Some(model) = model {
            params["model"] = Value::String(model.to_string());
        }
        let result = self.request("turn/start", params, REQUEST_TIMEOUT)?;
        result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("turn/start response did not contain turn.id")
    }

    pub fn interrupt_turn(&mut self, thread_id: &str, turn_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        self.request(
            "turn/interrupt",
            json!({ "threadId": thread_id, "turnId": turn_id }),
            REQUEST_TIMEOUT,
        )?;
        Ok(())
    }

    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<Value>> {
        if let Some(message) = self.backlog.pop_front() {
            return Ok(Some(message));
        }
        match self.messages.recv_timeout(timeout) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                let status = self.child.try_wait()?;
                bail!("Codex app-server disconnected (status: {status:?})")
            }
        }
    }

    pub fn respond(&mut self, id: Value, result: Value) -> Result<()> {
        self.write_message(&json!({ "id": id, "result": result }))
    }

    pub fn respond_error(&mut self, id: Value, code: i32, message: &str) -> Result<()> {
        self.write_message(&json!({
            "id": id,
            "error": { "code": code, "message": message }
        }))
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&json!({ "method": method, "id": id, "params": params }))?;
        let deadline = Instant::now() + timeout;
        let mut deferred = VecDeque::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.backlog.extend(deferred);
                bail!("Codex app-server request {method} timed out");
            }
            let message = match self.messages.recv_timeout(remaining) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    self.backlog.extend(deferred);
                    bail!("Codex app-server request {method} timed out");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.backlog.extend(deferred);
                    bail!("Codex app-server disconnected during {method}");
                }
            };
            if message.get("id") == Some(&json!(id)) {
                self.backlog.extend(deferred);
                if let Some(error) = message.get("error") {
                    bail!("Codex app-server {method} error: {error}");
                }
                return message
                    .get("result")
                    .cloned()
                    .context("app-server response has no result");
            }
            deferred.push_back(message);
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write_message(&json!({ "method": method, "params": params }))
    }

    fn write_message(&mut self, message: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.stdin, message)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn ensure_initialized(&self) -> Result<()> {
        if !self.initialized {
            bail!("Codex app-server is not initialized");
        }
        Ok(())
    }
}

impl Drop for CodexAppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
