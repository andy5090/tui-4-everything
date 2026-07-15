use std::io::{BufRead, BufReader, Write};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::mux::runtime::TmuxRunner;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterCapability {
    pub name: String,
    pub description: String,
    pub destructive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppObservation {
    pub adapter_id: String,
    pub target: String,
    pub state: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppAction {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppActionResult {
    pub observation: Option<AppObservation>,
    pub compensating_action: Option<AppAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdapterAuditEntry {
    pub timestamp: DateTime<Utc>,
    pub adapter_id: String,
    pub target: String,
    pub action: AppAction,
    pub success: bool,
    pub error: Option<String>,
}

pub trait AppAdapter {
    fn id(&self) -> &'static str;
    fn target(&self) -> &str;
    fn capabilities(&self) -> Vec<AdapterCapability>;
    fn observe(&self) -> Result<AppObservation>;
    fn execute(&self, action: &AppAction) -> Result<AppActionResult>;
}

#[cfg(unix)]
pub struct MpvAdapter {
    socket_path: String,
}

#[cfg(unix)]
impl MpvAdapter {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    fn request(&self, command: Value) -> Result<Value> {
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("failed to connect to mpv IPC {}", self.socket_path))?;
        serde_json::to_writer(&mut stream, &json!({ "command": command }))?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response)?;
        let value = serde_json::from_str::<Value>(&response)?;
        if value.get("error").and_then(Value::as_str) != Some("success") {
            bail!(
                "mpv IPC error: {}",
                value.get("error").unwrap_or(&Value::Null)
            );
        }
        Ok(value.get("data").cloned().unwrap_or(Value::Null))
    }

    fn property(&self, name: &str) -> Result<Value> {
        self.request(json!(["get_property", name]))
    }
}

#[cfg(unix)]
impl AppAdapter for MpvAdapter {
    fn id(&self) -> &'static str {
        "mpv"
    }

    fn target(&self) -> &str {
        &self.socket_path
    }

    fn capabilities(&self) -> Vec<AdapterCapability> {
        capabilities(&[
            ("set_pause", "Set playback pause state"),
            ("toggle_pause", "Toggle playback pause state"),
            ("seek_relative", "Seek by a bounded number of seconds"),
        ])
    }

    fn observe(&self) -> Result<AppObservation> {
        Ok(AppObservation {
            adapter_id: self.id().to_string(),
            target: self.target().to_string(),
            state: json!({
                "pause": self.property("pause")?,
                "timePosition": self.property("time-pos")?,
                "duration": self.property("duration")?,
                "mediaTitle": self.property("media-title")?
            }),
        })
    }

    fn execute(&self, action: &AppAction) -> Result<AppActionResult> {
        let compensating_action = match action.name.as_str() {
            "set_pause" => {
                let value = required_bool(&action.arguments, "value")?;
                let previous = self.property("pause")?.as_bool().unwrap_or(false);
                self.request(json!(["set_property", "pause", value]))?;
                Some(AppAction {
                    name: "set_pause".to_string(),
                    arguments: json!({ "value": previous }),
                })
            }
            "toggle_pause" => {
                self.request(json!(["cycle", "pause"]))?;
                Some(action.clone())
            }
            "seek_relative" => {
                let seconds = required_f64(&action.arguments, "seconds")?;
                if seconds.abs() > 600.0 {
                    bail!("seek_relative is limited to 600 seconds");
                }
                self.request(json!(["seek", seconds, "relative"]))?;
                Some(AppAction {
                    name: "seek_relative".to_string(),
                    arguments: json!({ "seconds": -seconds }),
                })
            }
            other => bail!("unsupported mpv action: {other}"),
        };
        Ok(AppActionResult {
            observation: None,
            compensating_action,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmuxAppKind {
    Yazi,
    Newsboat,
}

impl TmuxAppKind {
    fn id(self) -> &'static str {
        match self {
            Self::Yazi => "yazi",
            Self::Newsboat => "newsboat",
        }
    }

    fn executable(self) -> &'static str {
        self.id()
    }

    fn action_key(self, action: &str) -> Option<&'static str> {
        match (self, action) {
            (Self::Yazi, "up") | (Self::Newsboat, "up") => Some("k"),
            (Self::Yazi, "down") | (Self::Newsboat, "down") => Some("j"),
            (Self::Yazi, "open") | (Self::Newsboat, "open") => Some("Enter"),
            (Self::Yazi, "parent") => Some("h"),
            (Self::Newsboat, "reload") => Some("r"),
            _ => None,
        }
    }
}

pub struct TmuxAppAdapter<R> {
    runner: R,
    kind: TmuxAppKind,
    pane_target: String,
}

impl<R> TmuxAppAdapter<R> {
    pub fn new(runner: R, kind: TmuxAppKind, pane_target: impl Into<String>) -> Self {
        Self {
            runner,
            kind,
            pane_target: pane_target.into(),
        }
    }
}

impl<R: TmuxRunner> TmuxAppAdapter<R> {
    fn current_command(&self) -> Result<String> {
        let output = self.runner.run(&strings([
            "display-message",
            "-p",
            "-t",
            self.pane_target.as_str(),
            "#{pane_current_command}",
        ]))?;
        ensure_tmux_success(&output.stderr, output.success)?;
        Ok(output.stdout.trim().to_string())
    }

    fn ensure_target(&self) -> Result<()> {
        let command = self.current_command()?;
        if command != self.kind.executable() {
            bail!(
                "pane {} runs {}, expected {}",
                self.pane_target,
                command,
                self.kind.executable()
            );
        }
        Ok(())
    }
}

impl<R: TmuxRunner> AppAdapter for TmuxAppAdapter<R> {
    fn id(&self) -> &'static str {
        self.kind.id()
    }

    fn target(&self) -> &str {
        &self.pane_target
    }

    fn capabilities(&self) -> Vec<AdapterCapability> {
        match self.kind {
            TmuxAppKind::Yazi => capabilities(&[
                ("up", "Move selection up"),
                ("down", "Move selection down"),
                ("open", "Open selected entry"),
                ("parent", "Open parent directory"),
            ]),
            TmuxAppKind::Newsboat => capabilities(&[
                ("up", "Move selection up"),
                ("down", "Move selection down"),
                ("open", "Open selected entry"),
                ("reload", "Reload feeds"),
            ]),
        }
    }

    fn observe(&self) -> Result<AppObservation> {
        self.ensure_target()?;
        let output = self.runner.run(&strings([
            "capture-pane",
            "-p",
            "-t",
            self.pane_target.as_str(),
            "-S",
            "-200",
        ]))?;
        ensure_tmux_success(&output.stderr, output.success)?;
        Ok(AppObservation {
            adapter_id: self.id().to_string(),
            target: self.target().to_string(),
            state: json!({ "screen": output.stdout }),
        })
    }

    fn execute(&self, action: &AppAction) -> Result<AppActionResult> {
        self.ensure_target()?;
        let key = self
            .kind
            .action_key(&action.name)
            .ok_or_else(|| anyhow::anyhow!("unsupported {} action: {}", self.id(), action.name))?;
        let output = self.runner.run(&strings([
            "send-keys",
            "-t",
            self.pane_target.as_str(),
            key,
        ]))?;
        ensure_tmux_success(&output.stderr, output.success)?;
        let compensating_action = match action.name.as_str() {
            "up" => Some(AppAction {
                name: "down".to_string(),
                arguments: json!({}),
            }),
            "down" => Some(AppAction {
                name: "up".to_string(),
                arguments: json!({}),
            }),
            _ => None,
        };
        Ok(AppActionResult {
            observation: None,
            compensating_action,
        })
    }
}

pub fn audited_execute(
    adapter: &dyn AppAdapter,
    action: &AppAction,
    audit: &mut Vec<AdapterAuditEntry>,
) -> Result<AppActionResult> {
    let result = adapter.execute(action);
    audit.push(AdapterAuditEntry {
        timestamp: Utc::now(),
        adapter_id: adapter.id().to_string(),
        target: adapter.target().to_string(),
        action: action.clone(),
        success: result.is_ok(),
        error: result.as_ref().err().map(ToString::to_string),
    });
    result
}

fn capabilities(values: &[(&str, &str)]) -> Vec<AdapterCapability> {
    values
        .iter()
        .map(|(name, description)| AdapterCapability {
            name: (*name).to_string(),
            description: (*description).to_string(),
            destructive: false,
        })
        .collect()
}

fn required_bool(value: &Value, name: &str) -> Result<bool> {
    value
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| anyhow::anyhow!("missing boolean argument: {name}"))
}

fn required_f64(value: &Value, name: &str) -> Result<f64> {
    value
        .get(name)
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("missing numeric argument: {name}"))
}

fn ensure_tmux_success(stderr: &str, success: bool) -> Result<()> {
    if success {
        Ok(())
    } else {
        bail!("tmux adapter command failed: {}", stderr.trim())
    }
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(str::to_string).collect()
}
