use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use serde_json::Value;

use crate::codex::service::{
    CodexCommand, CodexEvent, CodexService, bounded_action_schema, planner_prompt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AiProvider {
    Codex,
    Claude,
    Gemini,
}

impl AiProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderReadiness {
    pub provider: AiProvider,
    pub account: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiEvent {
    ProviderReady(ProviderReadiness),
    ProviderUnavailable {
        provider: AiProvider,
        reason: String,
    },
    ThreadStarted {
        provider: AiProvider,
        id: String,
    },
    TurnStarted {
        provider: AiProvider,
        id: String,
    },
    Delta {
        provider: AiProvider,
        text: String,
    },
    Message {
        provider: AiProvider,
        text: String,
    },
    ActionProposed {
        provider: AiProvider,
        kind: String,
        target: String,
    },
    Usage {
        provider: AiProvider,
        usage: String,
    },
    TurnCompleted {
        provider: AiProvider,
        status: String,
    },
    Diagnostic {
        provider: AiProvider,
        message: String,
    },
    Error {
        provider: AiProvider,
        message: String,
    },
}

pub struct AiService {
    codex: CodexService,
    external_events: Receiver<AiEvent>,
    external_sender: mpsc::Sender<AiEvent>,
    readiness: Vec<ProviderReadiness>,
}

impl AiService {
    pub fn spawn(cwd: String) -> Self {
        let codex = CodexService::spawn(cwd.clone());
        let (external_sender, external_events) = mpsc::channel();
        let mut readiness = Vec::new();
        if let Some(account) = claude_account() {
            readiness.push(ProviderReadiness {
                provider: AiProvider::Claude,
                account,
            });
        }
        if gemini_credentials_available() {
            readiness.push(ProviderReadiness {
                provider: AiProvider::Gemini,
                account: "configured credentials".to_string(),
            });
        }
        Self {
            codex,
            external_events,
            external_sender,
            readiness,
        }
    }

    pub fn ready_providers(&self) -> &[ProviderReadiness] {
        &self.readiness
    }

    pub fn prompt(
        &self,
        provider: AiProvider,
        text: String,
        environment_context: String,
    ) -> Result<(), String> {
        match provider {
            AiProvider::Codex => self
                .codex
                .send(CodexCommand::Prompt {
                    text,
                    environment_context,
                })
                .map_err(|_| "Codex service is not running".to_string()),
            AiProvider::Claude | AiProvider::Gemini => {
                let sender = self.external_sender.clone();
                thread::spawn(move || {
                    let _ = sender.send(AiEvent::TurnStarted {
                        provider,
                        id: "one-shot".to_string(),
                    });
                    match run_external_provider(provider, &text, &environment_context) {
                        Ok((message, action)) => {
                            let _ = sender.send(AiEvent::Message {
                                provider,
                                text: message,
                            });
                            if let Some((kind, target)) = action {
                                let _ = sender.send(AiEvent::ActionProposed {
                                    provider,
                                    kind,
                                    target,
                                });
                            }
                            let _ = sender.send(AiEvent::TurnCompleted {
                                provider,
                                status: "completed".to_string(),
                            });
                        }
                        Err(message) => {
                            let _ = sender.send(AiEvent::Error { provider, message });
                        }
                    }
                });
                Ok(())
            }
        }
    }

    pub fn interrupt(&self, provider: AiProvider) -> Result<(), String> {
        if provider == AiProvider::Codex {
            self.codex
                .send(CodexCommand::Interrupt)
                .map_err(|_| "Codex service is not running".to_string())
        } else {
            Err(format!(
                "{} one-shot requests cannot be interrupted yet",
                provider.label()
            ))
        }
    }

    pub fn try_recv(&self) -> Result<AiEvent, TryRecvError> {
        match self.external_events.try_recv() {
            Ok(event) => return Ok(event),
            Err(TryRecvError::Disconnected) => return Err(TryRecvError::Disconnected),
            Err(TryRecvError::Empty) => {}
        }
        self.codex.try_recv().map(map_codex_event)
    }
}

fn map_codex_event(event: CodexEvent) -> AiEvent {
    let provider = AiProvider::Codex;
    match event {
        CodexEvent::Ready { account } => {
            AiEvent::ProviderReady(ProviderReadiness { provider, account })
        }
        CodexEvent::ThreadStarted(id) => AiEvent::ThreadStarted { provider, id },
        CodexEvent::TurnStarted(id) => AiEvent::TurnStarted { provider, id },
        CodexEvent::Delta(text) => AiEvent::Delta { provider, text },
        CodexEvent::Message(text) => AiEvent::Message { provider, text },
        CodexEvent::ActionProposed { kind, target } => AiEvent::ActionProposed {
            provider,
            kind,
            target,
        },
        CodexEvent::Usage(usage) => AiEvent::Usage { provider, usage },
        CodexEvent::TurnCompleted(status) => AiEvent::TurnCompleted { provider, status },
        CodexEvent::ApprovalDenied(method) => AiEvent::Diagnostic {
            provider,
            message: format!("denied app-server request {method}"),
        },
        CodexEvent::Diagnostic(message) => AiEvent::Diagnostic { provider, message },
        CodexEvent::Error(message) => AiEvent::Error { provider, message },
    }
}

fn claude_account() -> Option<String> {
    let output = Command::new("claude")
        .args(["auth", "status", "--json"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    value
        .get("loggedIn")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        .then(|| {
            value
                .get("subscriptionType")
                .and_then(Value::as_str)
                .map_or_else(
                    || "authenticated".to_string(),
                    |plan| format!("{plan} subscription"),
                )
        })
}

fn gemini_credentials_available() -> bool {
    if std::env::var_os("GEMINI_API_KEY").is_some() || std::env::var_os("GOOGLE_API_KEY").is_some()
    {
        return true;
    }
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let credential = PathBuf::from(home).join(".gemini/oauth_creds.json");
    credential
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 2)
}

fn run_external_provider(
    provider: AiProvider,
    user_request: &str,
    environment_context: &str,
) -> Result<(String, Option<(String, String)>), String> {
    let prompt = format!(
        "{}\n\nReturn only a JSON object matching this schema: {}",
        planner_prompt(environment_context, user_request),
        bounded_action_schema()
    );
    let output = match provider {
        AiProvider::Claude => Command::new("claude")
            .args(["-p", &prompt, "--output-format", "json", "--max-turns", "1"])
            .arg("--json-schema")
            .arg(bounded_action_schema().to_string())
            .arg("--tools")
            .arg("")
            .arg("--disable-slash-commands")
            .output(),
        AiProvider::Gemini => Command::new("gemini")
            .args([
                "-p",
                &prompt,
                "--output-format",
                "json",
                "--approval-mode",
                "plan",
            ])
            .output(),
        AiProvider::Codex => unreachable!("Codex uses its app-server adapter"),
    }
    .map_err(|error| format!("could not start {}: {error}", provider.label()))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("{} request failed: {detail}", provider.label()));
    }
    parse_external_response(&output.stdout)
}

fn parse_external_response(raw: &[u8]) -> Result<(String, Option<(String, String)>), String> {
    let envelope: Value = serde_json::from_slice(raw)
        .map_err(|error| format!("provider returned invalid JSON: {error}"))?;
    let structured = envelope
        .get("structured_output")
        .cloned()
        .or_else(|| {
            envelope
                .get("response")
                .and_then(Value::as_str)
                .and_then(parse_json_text)
        })
        .or_else(|| {
            envelope
                .get("result")
                .and_then(Value::as_str)
                .and_then(parse_json_text)
        })
        .unwrap_or(envelope);
    let message = structured
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| structured.get("response").and_then(Value::as_str))
        .ok_or_else(|| "provider response did not contain a message".to_string())?
        .to_string();
    let action = structured
        .get("action")
        .filter(|value| !value.is_null())
        .and_then(|action| {
            Some((
                action.get("type")?.as_str()?.to_string(),
                action.get("target")?.as_str()?.to_string(),
            ))
        });
    Ok((message, action))
}

fn parse_json_text(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok().or_else(|| {
        text.strip_prefix("```json")?
            .strip_suffix("```")
            .and_then(|body| serde_json::from_str(body.trim()).ok())
    })
}

#[cfg(test)]
mod tests {
    use super::parse_external_response;

    #[test]
    fn parses_claude_structured_output() {
        let raw = br#"{"structured_output":{"message":"Found it","action":{"type":"catalog_search","target":"yazi"}}}"#;
        let (message, action) = parse_external_response(raw).unwrap();
        assert_eq!(message, "Found it");
        assert_eq!(
            action,
            Some(("catalog_search".to_string(), "yazi".to_string()))
        );
    }

    #[test]
    fn parses_gemini_json_response() {
        let raw = br#"{"response":"{\"message\":\"Ready\",\"action\":null}"}"#;
        let (message, action) = parse_external_response(raw).unwrap();
        assert_eq!(message, "Ready");
        assert_eq!(action, None);
    }
}
