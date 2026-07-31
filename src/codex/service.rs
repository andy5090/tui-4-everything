use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::app_server::CodexAppServer;

#[derive(Debug)]
pub enum CodexCommand {
    Prompt {
        text: String,
        environment_context: String,
    },
    Interrupt,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexEvent {
    Ready { account: String },
    ThreadStarted(String),
    TurnStarted(String),
    Delta(String),
    Message(String),
    ActionProposed { kind: String, target: String },
    Usage(String),
    TurnCompleted(String),
    ApprovalDenied(String),
    Diagnostic(String),
    Error(String),
}

pub struct CodexService {
    commands: Sender<CodexCommand>,
    events: Receiver<CodexEvent>,
    handle: Option<thread::JoinHandle<()>>,
}

impl CodexService {
    pub fn spawn(cwd: String) -> Self {
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let handle = thread::spawn(move || run_service(cwd, command_receiver, event_sender));
        Self {
            commands: command_sender,
            events: event_receiver,
            handle: Some(handle),
        }
    }

    pub fn send(&self, command: CodexCommand) -> Result<(), mpsc::SendError<CodexCommand>> {
        self.commands.send(command)
    }

    pub fn try_recv(&self) -> Result<CodexEvent, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for CodexService {
    fn drop(&mut self) {
        let _ = self.commands.send(CodexCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_service(cwd: String, commands: Receiver<CodexCommand>, events: Sender<CodexEvent>) {
    let mut client = match connect_client(&events) {
        Ok(client) => client,
        Err(error) => {
            let _ = events.send(CodexEvent::Error(error.to_string()));
            return;
        }
    };

    let mut thread_id: Option<String> = None;
    let mut active_turn: Option<String> = None;
    let mut active_prompt: Option<(String, String)> = None;
    let mut retry_used = false;
    loop {
        match commands.recv_timeout(Duration::from_millis(25)) {
            Ok(CodexCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(CodexCommand::Prompt {
                text: prompt,
                environment_context,
            }) => {
                if active_turn.is_some() {
                    let _ = events.send(CodexEvent::Error(
                        "A Codex turn is already active; interrupt it first".to_string(),
                    ));
                } else {
                    match start_prompt(
                        &mut client,
                        &mut thread_id,
                        &cwd,
                        &environment_context,
                        &prompt,
                        None,
                        &events,
                    ) {
                        Ok(turn_id) => {
                            active_turn = Some(turn_id.clone());
                            active_prompt = Some((prompt, environment_context));
                            retry_used = false;
                            let _ = events.send(CodexEvent::TurnStarted(turn_id));
                        }
                        Err(error) => {
                            let _ = events.send(CodexEvent::Error(error.to_string()));
                        }
                    }
                }
            }
            Ok(CodexCommand::Interrupt) => {
                if let (Some(thread_id), Some(turn_id)) = (&thread_id, &active_turn)
                    && let Err(error) = client.interrupt_turn(thread_id, turn_id)
                {
                    let _ = events.send(CodexEvent::Error(error.to_string()));
                }
                active_prompt = None;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        loop {
            match client.recv_timeout(Duration::from_millis(1)) {
                Ok(Some(message)) => match handle_message(&mut client, message, &events) {
                    MessageOutcome::Continue => {}
                    MessageOutcome::TurnCompleted { status, error } => {
                        active_turn = None;
                        if status == "failed"
                            && !retry_used
                            && let Some((prompt, environment_context)) = active_prompt.as_ref()
                        {
                            let detail = error.as_deref().unwrap_or("unknown failure");
                            let fallback_model = fallback_model_for_error(detail);
                            let recovery = fallback_model.as_deref().map_or_else(
                                || "restarting app-server once".to_string(),
                                |model| format!("retrying once with compatible model {model}"),
                            );
                            let _ = events.send(CodexEvent::Error(format!(
                                "Codex turn failed ({detail}); {recovery}"
                            )));
                            match connect_client(&events).and_then(|new_client| {
                                client = new_client;
                                thread_id = None;
                                start_prompt(
                                    &mut client,
                                    &mut thread_id,
                                    &cwd,
                                    environment_context,
                                    prompt,
                                    fallback_model.as_deref(),
                                    &events,
                                )
                            }) {
                                Ok(turn_id) => {
                                    active_turn = Some(turn_id.clone());
                                    retry_used = true;
                                    let _ = events.send(CodexEvent::TurnStarted(turn_id));
                                }
                                Err(retry_error) => {
                                    active_prompt = None;
                                    let _ = events.send(CodexEvent::Error(format!(
                                        "Codex automatic retry failed: {retry_error}"
                                    )));
                                }
                            }
                        } else {
                            active_prompt = None;
                            let label = error
                                .map(|detail| format!("{status}: {detail}"))
                                .unwrap_or(status);
                            let _ = events.send(CodexEvent::TurnCompleted(label));
                        }
                    }
                },
                Ok(None) => break,
                Err(error) => {
                    let _ = events.send(CodexEvent::Error(error.to_string()));
                    return;
                }
            }
        }
    }
}

fn connect_client(events: &Sender<CodexEvent>) -> anyhow::Result<CodexAppServer> {
    let mut client = CodexAppServer::spawn()?;
    client.initialize()?;
    let account_value = client.account_read()?;
    if account_value.get("account").is_none_or(Value::is_null) {
        anyhow::bail!("Codex is not signed in; run `codex login`");
    }
    let account = account_label(&account_value);
    let _ = events.send(CodexEvent::Ready { account });
    Ok(client)
}

fn start_prompt(
    client: &mut CodexAppServer,
    thread_id: &mut Option<String>,
    cwd: &str,
    environment_context: &str,
    prompt: &str,
    model: Option<&str>,
    events: &Sender<CodexEvent>,
) -> anyhow::Result<String> {
    if thread_id.is_none() {
        let id = client.start_thread(cwd)?;
        let _ = events.send(CodexEvent::ThreadStarted(id.clone()));
        *thread_id = Some(id);
    }
    let bounded_prompt = planner_prompt(environment_context, prompt);
    let thread_id = thread_id.as_deref().expect("thread initialized");
    match model {
        Some(model) => client.start_turn_structured_with_model(
            thread_id,
            &bounded_prompt,
            bounded_action_schema(),
            model,
        ),
        None => client.start_turn_structured(thread_id, &bounded_prompt, bounded_action_schema()),
    }
}

pub fn planner_prompt(environment_context: &str, user_request: &str) -> String {
    format!(
        "You are the AI control plane embedded inside T4E (TUI for Everything), not a generic coding assistant. The user is talking to you from the assistant rail on HOME. T4E catalogs terminal apps, installs only through its approval flow, applies only T4E-verified app versions, and launches individual apps from HOME. Treat the supplied T4E runtime context as authoritative.\n\nHelp the user operate this T4E environment. Refer to apps by their exact local IDs. Never run shell commands, edit files, or claim an action already happened. Return a concise user-facing message and at most one bounded action. Every action is only a proposal; T4E asks the user to approve it and owns installation, verified updates, process lifecycle, hidden tmux sessions, permissions, and audit logs.\n\nAvailable bounded actions:\n- catalog_search: show an app in HOME\n- install_plan: prepare an app installation plan\n- verified_update: apply the exact T4E-verified version when the app supports it\n- launch_app: launch an installed catalog app through T4E\n\nCurrent T4E runtime context:\n{environment_context}\n\nUser request: {user_request}"
    )
}

enum MessageOutcome {
    Continue,
    TurnCompleted {
        status: String,
        error: Option<String>,
    },
}

fn handle_message(
    client: &mut CodexAppServer,
    message: Value,
    events: &Sender<CodexEvent>,
) -> MessageOutcome {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    if let Some(id) = message.get("id").cloned() {
        let _ = client.respond_error(id, -32000, "T4E denies app-server initiated requests");
        let _ = events.send(CodexEvent::ApprovalDenied(method.to_string()));
        return MessageOutcome::Continue;
    }
    match method {
        "item/agentMessage/delta" => {
            if let Some(delta) = message.pointer("/params/delta").and_then(Value::as_str) {
                let _ = events.send(CodexEvent::Delta(delta.to_string()));
            }
        }
        "item/completed" => {
            let item = message.pointer("/params/item");
            if item
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                == Some("agentMessage")
                && let Some(text) = item
                    .and_then(|item| item.get("text"))
                    .and_then(Value::as_str)
            {
                emit_structured_message(text, events);
            }
        }
        "turn/completed" => return parse_turn_completion(&message),
        "thread/tokenUsage/updated" | "account/rateLimits/updated" => {
            let usage = message
                .get("params")
                .cloned()
                .unwrap_or(Value::Null)
                .to_string();
            let _ = events.send(CodexEvent::Usage(usage));
        }
        "t4e/stderr" => {
            let detail = extract_error_detail(&message);
            let _ = events.send(CodexEvent::Diagnostic(detail));
        }
        "error" | "t4e/protocolError" => {
            let detail = extract_error_detail(&message);
            let _ = events.send(CodexEvent::Error(detail));
        }
        _ => {}
    }
    MessageOutcome::Continue
}

fn fallback_model_for_error(error: &str) -> Option<String> {
    error
        .contains("requires a newer version of Codex")
        .then(|| {
            std::env::var("T4E_CODEX_FALLBACK_MODEL").unwrap_or_else(|_| "gpt-5.4".to_string())
        })
}

fn parse_turn_completion(message: &Value) -> MessageOutcome {
    let status = message
        .pointer("/params/turn/status")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "completed".to_string());
    let error = message
        .pointer("/params/turn/error/message")
        .and_then(Value::as_str)
        .map(str::to_string);
    MessageOutcome::TurnCompleted { status, error }
}

fn extract_error_detail(message: &Value) -> String {
    message
        .pointer("/params/message")
        .or_else(|| message.pointer("/params/error/message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            message
                .get("params")
                .filter(|params| !params.is_null())
                .map(Value::to_string)
        })
        .unwrap_or_else(|| "unknown Codex error".to_string())
}

fn emit_structured_message(text: &str, events: &Sender<CodexEvent>) {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        let _ = events.send(CodexEvent::Message(text.to_string()));
        return;
    };
    let message = value.get("message").and_then(Value::as_str).unwrap_or(text);
    let _ = events.send(CodexEvent::Message(message.to_string()));
    if let Some(action) = value.get("action").filter(|action| !action.is_null())
        && let (Some(kind), Some(target)) = (
            action.get("type").and_then(Value::as_str),
            action.get("target").and_then(Value::as_str),
        )
    {
        let _ = events.send(CodexEvent::ActionProposed {
            kind: kind.to_string(),
            target: target.to_string(),
        });
    }
}

pub fn bounded_action_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "message": { "type": "string" },
            "action": {
                "anyOf": [
                    { "type": "null" },
                    {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["catalog_search", "install_plan", "verified_update", "launch_app"]
                            },
                            "target": { "type": "string" }
                        },
                        "required": ["type", "target"],
                        "additionalProperties": false
                    }
                ]
            }
        },
        "required": ["message", "action"],
        "additionalProperties": false
    })
}

fn account_label(value: &Value) -> String {
    let account = value.get("account").unwrap_or(value);
    let kind = account
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let email = account.get("email").and_then(Value::as_str);
    email.map_or_else(|| kind.to_string(), |email| format!("{kind} ({email})"))
}

#[cfg(test)]
mod tests {
    use super::{
        MessageOutcome, extract_error_detail, fallback_model_for_error, parse_turn_completion,
        planner_prompt,
    };
    use serde_json::json;

    #[test]
    fn failed_turn_preserves_status_and_error_message() {
        let message = json!({
            "params": {
                "turn": {
                    "status": "failed",
                    "error": { "message": "model cache is incompatible" }
                }
            }
        });
        match parse_turn_completion(&message) {
            MessageOutcome::TurnCompleted { status, error } => {
                assert_eq!(status, "failed");
                assert_eq!(error.as_deref(), Some("model cache is incompatible"));
            }
            MessageOutcome::Continue => panic!("expected a completed turn"),
        }
    }

    #[test]
    fn nested_protocol_error_is_visible() {
        let message = json!({
            "params": { "error": { "message": "request rejected" } }
        });
        assert_eq!(extract_error_detail(&message), "request rejected");
    }

    #[test]
    fn newer_model_error_selects_a_compatible_fallback() {
        let error = "The 'future' model requires a newer version of Codex. Please upgrade.";
        assert_eq!(fallback_model_for_error(error).as_deref(), Some("gpt-5.4"));
        assert!(fallback_model_for_error("temporary network failure").is_none());
    }

    #[test]
    fn planner_prompt_establishes_t4e_identity_and_execution_boundaries() {
        let prompt = planner_prompt(
            "platform: linux\ncatalog apps: yazi=Yazi (run: yazi)",
            "What can you do here?",
        );

        assert!(prompt.contains("AI control plane embedded inside T4E"));
        assert!(prompt.contains("not a generic coding assistant"));
        assert!(prompt.contains("installs only through its approval flow"));
        assert!(prompt.contains("launches individual apps"));
        assert!(prompt.contains("launches individual apps from HOME"));
        assert!(prompt.contains("Every action is only a proposal"));
        assert!(prompt.contains("verified_update"));
        assert!(prompt.contains("catalog apps: yazi=Yazi (run: yazi)"));
        assert!(prompt.ends_with("User request: What can you do here?"));
    }
}
