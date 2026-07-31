use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

use crate::codex::service::{
    CodexCommand, CodexEvent, CodexService, bounded_action_schema, planner_prompt,
};
use crate::storage::{ApiProviderProfile, ProviderAuthMode, default_api_provider_profiles};

const CURL_CONNECT_TIMEOUT_SEC: u64 = 10;
const CURL_MAX_TIMEOUT_SEC: u64 = 45;
const MAX_ERROR_DETAIL_BYTES: usize = 320;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AiProvider {
    Codex,
    Claude,
    Gemini,
    Zhipu,
    Kimi,
    Custom,
}

impl AiProvider {
    pub const ALL: [Self; 6] = [
        Self::Codex,
        Self::Claude,
        Self::Gemini,
        Self::Zhipu,
        Self::Kimi,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Zhipu => "Zhipu AI",
            Self::Kimi => "Kimi",
            Self::Custom => "Custom",
        }
    }

    pub fn profile_id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Zhipu => "zhipu",
            Self::Kimi => "kimi",
            Self::Custom => "custom",
        }
    }

    pub fn supports_subscription(self) -> bool {
        matches!(self, Self::Codex | Self::Claude | Self::Gemini)
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

#[derive(Clone)]
struct ApiProviderRuntimeState {
    profiles: BTreeMap<String, ApiProviderProfile>,
    session_api_keys: BTreeMap<AiProvider, String>,
    readiness: Vec<ProviderReadiness>,
}

impl ApiProviderRuntimeState {
    fn new(profiles: BTreeMap<String, ApiProviderProfile>) -> Self {
        let mut state = Self {
            profiles,
            session_api_keys: BTreeMap::new(),
            readiness: Vec::new(),
        };
        state.refresh_readiness();
        state
    }

    fn refresh_readiness(&mut self) {
        let mut readiness = Vec::new();
        for provider in AiProvider::ALL {
            let profile = profile_for(provider, &self.profiles);
            match profile.auth_mode {
                ProviderAuthMode::Subscription => {
                    if let Some(account) = subscription_account(provider) {
                        readiness.push(ProviderReadiness { provider, account });
                    }
                }
                ProviderAuthMode::ApiKey => {
                    if let Ok(config) =
                        resolve_api_provider(provider, &self.profiles, &self.session_api_keys)
                    {
                        readiness.push(ProviderReadiness {
                            provider,
                            account: readiness_label(&config),
                        });
                    }
                }
            }
        }
        self.readiness = readiness;
    }
}

pub struct AiService {
    codex: CodexService,
    external_events: Receiver<AiEvent>,
    external_sender: mpsc::Sender<AiEvent>,
    api_runtime: Arc<Mutex<ApiProviderRuntimeState>>,
}

impl AiService {
    pub fn spawn(cwd: String) -> Self {
        Self::spawn_with_profiles(cwd, default_api_provider_profiles())
    }

    pub fn spawn_with_profiles(
        cwd: String,
        profiles: BTreeMap<String, ApiProviderProfile>,
    ) -> Self {
        let codex = CodexService::spawn(cwd.clone());
        let (external_sender, external_events) = mpsc::channel();
        Self {
            codex,
            external_events,
            external_sender,
            api_runtime: Arc::new(Mutex::new(ApiProviderRuntimeState::new(profiles))),
        }
    }

    pub fn ready_providers(&self) -> Vec<ProviderReadiness> {
        self.api_runtime
            .lock()
            .expect("api runtime lock")
            .readiness
            .clone()
    }

    pub fn reload_api_provider_profiles(
        &self,
        profiles: BTreeMap<String, ApiProviderProfile>,
    ) -> Vec<ProviderReadiness> {
        let mut runtime = self.api_runtime.lock().expect("api runtime lock");
        runtime.profiles = profiles;
        runtime.refresh_readiness();
        runtime.readiness.clone()
    }

    pub fn configure_api_provider(
        &self,
        provider: AiProvider,
        profile: ApiProviderProfile,
        api_key: String,
    ) -> Result<ProviderReadiness, String> {
        let mut runtime = self.api_runtime.lock().expect("api runtime lock");
        let mut profiles = runtime.profiles.clone();
        profiles.insert(provider.profile_id().to_string(), profile.clone());
        let mut session_api_keys = runtime.session_api_keys.clone();
        let trimmed_key = api_key.trim().to_string();
        if trimmed_key.is_empty() {
            session_api_keys.remove(&provider);
        } else {
            session_api_keys.insert(provider, trimmed_key);
        }
        let account = match profile.auth_mode {
            ProviderAuthMode::Subscription => subscription_account(provider)
                .ok_or_else(|| format!("{} subscription is not detected", provider.label()))?,
            ProviderAuthMode::ApiKey => {
                let config = resolve_api_provider(provider, &profiles, &session_api_keys)?;
                readiness_label(&config)
            }
        };
        runtime.profiles = profiles;
        runtime.session_api_keys = session_api_keys;
        runtime.refresh_readiness();
        Ok(ProviderReadiness { provider, account })
    }

    pub fn prompt(
        &self,
        provider: AiProvider,
        text: String,
        environment_context: String,
    ) -> Result<(), String> {
        let auth_mode = {
            let runtime = self.api_runtime.lock().expect("api runtime lock");
            profile_for(provider, &runtime.profiles).auth_mode
        };
        if auth_mode == ProviderAuthMode::Subscription {
            return match provider {
                AiProvider::Codex => self
                    .codex
                    .send(CodexCommand::Prompt {
                        text,
                        environment_context,
                    })
                    .map_err(|_| "Codex service is not running".to_string()),
                AiProvider::Claude | AiProvider::Gemini => {
                    self.spawn_one_shot(provider, move || {
                        run_external_provider(provider, &text, &environment_context)
                    });
                    Ok(())
                }
                AiProvider::Zhipu | AiProvider::Kimi | AiProvider::Custom => Err(format!(
                    "{} does not support subscription authentication",
                    provider.label()
                )),
            };
        }

        let config = {
            let runtime = self.api_runtime.lock().expect("api runtime lock");
            resolve_api_provider(provider, &runtime.profiles, &runtime.session_api_keys)?
        };
        self.spawn_one_shot(provider, move || {
            run_api_provider(provider, &config, &text, &environment_context)
        });
        Ok(())
    }

    pub fn interrupt(&self, provider: AiProvider) -> Result<(), String> {
        let auth_mode = {
            let runtime = self.api_runtime.lock().expect("api runtime lock");
            profile_for(provider, &runtime.profiles).auth_mode
        };
        if provider == AiProvider::Codex && auth_mode == ProviderAuthMode::Subscription {
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
        loop {
            let event = self.codex.try_recv()?;
            let use_subscription = self
                .api_runtime
                .lock()
                .expect("api runtime lock")
                .profiles
                .get(AiProvider::Codex.profile_id())
                .is_none_or(|profile| profile.auth_mode == ProviderAuthMode::Subscription);
            if use_subscription {
                return Ok(map_codex_event(event));
            }
        }
    }

    fn spawn_one_shot<F>(&self, provider: AiProvider, request: F)
    where
        F: FnOnce() -> Result<(String, Option<(String, String)>), String> + Send + 'static,
    {
        let sender = self.external_sender.clone();
        thread::spawn(move || {
            let _ = sender.send(AiEvent::TurnStarted {
                provider,
                id: "one-shot".to_string(),
            });
            match request() {
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

#[derive(Clone)]
struct ResolvedApiProvider {
    provider: AiProvider,
    profile: ApiProviderProfile,
    api_key: String,
    key_source: String,
    endpoint_url: String,
}

fn resolve_api_provider(
    provider: AiProvider,
    profiles: &BTreeMap<String, ApiProviderProfile>,
    session_api_keys: &BTreeMap<AiProvider, String>,
) -> Result<ResolvedApiProvider, String> {
    let profile = profile_for(provider, profiles);
    if profile.auth_mode != ProviderAuthMode::ApiKey {
        return Err(format!("{} is using subscription mode", provider.label()));
    }
    validate_profile(provider, &profile)?;
    let session_key = session_api_keys
        .get(&provider)
        .cloned()
        .filter(|key| !key.is_empty());
    let env_key = profile
        .api_key_env
        .trim()
        .strip_prefix('$')
        .unwrap_or(profile.api_key_env.trim())
        .to_string();
    let (api_key, key_source) = if let Some(api_key) = session_key {
        (api_key, "session key".to_string())
    } else if !env_key.is_empty() {
        let value = std::env::var(&env_key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{} is missing {env_key}", provider.label()))?;
        (value, format!("env {env_key}"))
    } else {
        return Err(format!("{} needs an API key", provider.label()));
    };
    if api_key.chars().any(char::is_whitespace) || api_key.chars().any(char::is_control) {
        return Err(format!(
            "{} API key contains unsupported whitespace or control characters",
            provider.label()
        ));
    }
    Ok(ResolvedApiProvider {
        provider,
        endpoint_url: endpoint_for_profile(provider, &profile)?,
        profile,
        api_key,
        key_source,
    })
}

fn default_api_profile(provider: AiProvider) -> ApiProviderProfile {
    default_api_provider_profiles()
        .remove(provider.profile_id())
        .unwrap_or(ApiProviderProfile {
            auth_mode: ProviderAuthMode::ApiKey,
            label: provider.label().to_string(),
            base_url: String::new(),
            model: String::new(),
            api_key_env: String::new(),
        })
}

fn profile_for(
    provider: AiProvider,
    profiles: &BTreeMap<String, ApiProviderProfile>,
) -> ApiProviderProfile {
    profiles
        .get(provider.profile_id())
        .cloned()
        .unwrap_or_else(|| default_api_profile(provider))
}

fn readiness_label(config: &ResolvedApiProvider) -> String {
    format!("{} via {}", config.profile.label, config.key_source)
}

fn validate_profile(provider: AiProvider, profile: &ApiProviderProfile) -> Result<(), String> {
    if profile.label.trim().is_empty() {
        return Err(format!(
            "{} profile label cannot be empty",
            provider.label()
        ));
    }
    if profile.model.trim().is_empty() {
        return Err(format!("{} model cannot be empty", provider.label()));
    }
    if profile
        .label
        .chars()
        .chain(profile.model.chars())
        .any(char::is_control)
    {
        return Err(format!(
            "{} profile fields cannot contain control characters",
            provider.label()
        ));
    }
    let env_name = profile
        .api_key_env
        .trim()
        .strip_prefix('$')
        .unwrap_or(profile.api_key_env.trim());
    if !env_name.is_empty() && !valid_env_name(env_name) {
        return Err(format!(
            "{} key environment must be a valid variable name",
            provider.label()
        ));
    }
    endpoint_for_profile(provider, profile).map(|_| ())
}

fn valid_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn endpoint_for_profile(
    provider: AiProvider,
    profile: &ApiProviderProfile,
) -> Result<String, String> {
    let base_url = &profile.base_url;
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(format!("{} base URL cannot be empty", provider.label()));
    }
    let (scheme, rest) = trimmed.split_once("://").ok_or_else(|| {
        format!(
            "{} base URL must include http:// or https://",
            provider.label()
        )
    })?;
    if rest.is_empty() {
        return Err(format!(
            "{} base URL host cannot be empty",
            provider.label()
        ));
    }
    if trimmed.chars().any(char::is_whitespace) || trimmed.chars().any(char::is_control) {
        return Err(format!(
            "{} base URL cannot contain whitespace or control characters",
            provider.label()
        ));
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() || authority.starts_with(':') {
        return Err(format!(
            "{} base URL host cannot be empty",
            provider.label()
        ));
    }
    if authority.contains('@') {
        return Err(format!(
            "{} base URL must not contain embedded credentials",
            provider.label()
        ));
    }
    if trimmed.contains('?') || trimmed.contains('#') {
        return Err(format!(
            "{} base URL must not contain query or fragment",
            provider.label()
        ));
    }
    match scheme {
        "https" => {}
        "http" if provider == AiProvider::Custom && is_localhost_host(rest) => {}
        "http" => {
            return Err(format!(
                "{} requires HTTPS unless the custom endpoint is localhost",
                provider.label()
            ));
        }
        _ => {
            return Err(format!(
                "{} base URL must use http or https",
                provider.label()
            ));
        }
    }
    let endpoint = match provider {
        AiProvider::Codex => format!("{trimmed}/responses"),
        AiProvider::Claude => format!("{trimmed}/messages"),
        AiProvider::Gemini => {
            let model = profile
                .model
                .trim()
                .strip_prefix("models/")
                .unwrap_or(profile.model.trim());
            if model.is_empty()
                || !model
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
            {
                return Err("Gemini model must be a valid model ID".to_string());
            }
            format!("{trimmed}/models/{model}:generateContent")
        }
        AiProvider::Zhipu | AiProvider::Kimi | AiProvider::Custom => {
            format!("{trimmed}/chat/completions")
        }
    };
    Ok(endpoint)
}

fn is_localhost_host(rest: &str) -> bool {
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn subscription_account(provider: AiProvider) -> Option<String> {
    match provider {
        AiProvider::Codex => codex_account(),
        AiProvider::Claude => claude_account(),
        AiProvider::Gemini => gemini_subscription_account(),
        AiProvider::Zhipu | AiProvider::Kimi | AiProvider::Custom => None,
    }
}

fn codex_account() -> Option<String> {
    let mut command = Command::new("codex");
    command
        .args(["login", "status"])
        .env_remove("OPENAI_API_KEY");
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(if status.is_empty() {
        "Codex subscription".to_string()
    } else {
        status
    })
}

fn claude_account() -> Option<String> {
    let mut command = Command::new("claude");
    command
        .args(["auth", "status", "--json"])
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN");
    let output = command.output().ok()?;
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

fn gemini_subscription_account() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let credential = PathBuf::from(home).join(".gemini/oauth_creds.json");
    credential
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 2)
        .then(|| "Gemini subscription".to_string())
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
    let mut command = match provider {
        AiProvider::Claude => {
            let mut command = Command::new("claude");
            command
                .args(["-p", &prompt, "--output-format", "json", "--max-turns", "1"])
                .arg("--json-schema")
                .arg(bounded_action_schema().to_string())
                .arg("--tools")
                .arg("")
                .arg("--disable-slash-commands")
                .env_remove("ANTHROPIC_API_KEY")
                .env_remove("ANTHROPIC_AUTH_TOKEN");
            command
        }
        AiProvider::Gemini => {
            let mut command = Command::new("gemini");
            command.args([
                "-p",
                &prompt,
                "--output-format",
                "json",
                "--approval-mode",
                "plan",
            ]);
            command
                .env_remove("GEMINI_API_KEY")
                .env_remove("GOOGLE_API_KEY");
            command
        }
        AiProvider::Codex | AiProvider::Zhipu | AiProvider::Kimi | AiProvider::Custom => {
            unreachable!("handled by another adapter")
        }
    };
    let output = command
        .output()
        .map_err(|error| format!("could not start {}: {error}", provider.label()))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("{} request failed: {detail}", provider.label()));
    }
    parse_external_response(&output.stdout)
}

#[derive(Clone)]
struct CurlInvocation {
    args: Vec<String>,
    config: String,
    debug_summary: String,
}

fn run_api_provider(
    provider: AiProvider,
    config: &ResolvedApiProvider,
    user_request: &str,
    environment_context: &str,
) -> Result<(String, Option<(String, String)>), String> {
    let prompt = format!(
        "{}\n\nReturn only a JSON object matching this schema: {}",
        planner_prompt(environment_context, user_request),
        bounded_action_schema()
    );
    let request_body = request_body_for_provider(config, &prompt);
    let invocation = build_curl_invocation(config, &request_body)?;
    let mut command = Command::new("curl");
    command
        .args(&invocation.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start {} request: {error}", provider.label()))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| format!("{} request stdin is unavailable", provider.label()))?
        .write_all(invocation.config.as_bytes())
        .map_err(|error| format!("could not write {} request: {error}", provider.label()))?;
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .map_err(|error| format!("{} request failed: {error}", provider.label()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = bounded_error_detail(
            &format!("{stderr}\n{stdout}"),
            &config.api_key,
            &invocation.debug_summary,
        );
        return Err(format!("{} request failed: {detail}", provider.label()));
    }
    let message = parse_api_message(provider, &output.stdout)?;
    parse_external_response(message.as_bytes())
}

fn request_body_for_provider(config: &ResolvedApiProvider, prompt: &str) -> Value {
    match config.provider {
        AiProvider::Codex => json!({
            "model": config.profile.model,
            "input": prompt,
            "store": false,
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "t4e_action",
                    "strict": true,
                    "schema": bounded_action_schema(),
                }
            }
        }),
        AiProvider::Claude => json!({
            "model": config.profile.model,
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": prompt }],
        }),
        AiProvider::Gemini => json!({
            "contents": [{ "parts": [{ "text": prompt }] }],
            "generationConfig": { "responseMimeType": "application/json" },
        }),
        AiProvider::Zhipu | AiProvider::Kimi | AiProvider::Custom => json!({
            "model": config.profile.model,
            "messages": [{ "role": "user", "content": prompt }],
        }),
    }
}

fn build_curl_invocation(
    config: &ResolvedApiProvider,
    body: &Value,
) -> Result<CurlInvocation, String> {
    let serialized_body = serde_json::to_string(body)
        .map_err(|error| format!("could not serialize request body: {error}"))?;
    let escaped_url = escape_curl_config_string(&config.endpoint_url);
    let escaped_content_type = escape_curl_config_string("Content-Type: application/json");
    let escaped_accept = escape_curl_config_string("Accept: application/json");
    let escaped_body = escape_curl_config_string(&serialized_body);
    let mut config_text = format!(
        "url = \"{escaped_url}\"\nrequest = \"POST\"\nheader = \"{escaped_content_type}\"\nheader = \"{escaped_accept}\"\n"
    );
    for header in api_auth_headers(config) {
        config_text.push_str(&format!(
            "header = \"{}\"\n",
            escape_curl_config_string(&header)
        ));
    }
    config_text.push_str(&format!("data = \"{escaped_body}\"\n"));
    Ok(CurlInvocation {
        args: vec![
            "--config".to_string(),
            "-".to_string(),
            "--silent".to_string(),
            "--show-error".to_string(),
            "--fail-with-body".to_string(),
            "--connect-timeout".to_string(),
            CURL_CONNECT_TIMEOUT_SEC.to_string(),
            "--max-time".to_string(),
            CURL_MAX_TIMEOUT_SEC.to_string(),
        ],
        debug_summary: format!(
            "POST {} model {}",
            config.endpoint_url, config.profile.model
        ),
        config: config_text,
    })
}

fn api_auth_headers(config: &ResolvedApiProvider) -> Vec<String> {
    match config.provider {
        AiProvider::Claude => vec![
            format!("x-api-key: {}", config.api_key),
            "anthropic-version: 2023-06-01".to_string(),
        ],
        AiProvider::Gemini => vec![format!("x-goog-api-key: {}", config.api_key)],
        AiProvider::Codex | AiProvider::Zhipu | AiProvider::Kimi | AiProvider::Custom => {
            vec![format!("Authorization: Bearer {}", config.api_key)]
        }
    }
}

fn escape_curl_config_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn bounded_error_detail(detail: &str, secret: &str, summary: &str) -> String {
    let redacted = redact_secret(detail, secret);
    let trimmed = redacted.trim();
    if trimmed.is_empty() {
        summary.to_string()
    } else if trimmed.len() > MAX_ERROR_DETAIL_BYTES {
        let mut end = MAX_ERROR_DETAIL_BYTES;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &trimmed[..end])
    } else {
        trimmed.to_string()
    }
}

fn redact_secret(detail: &str, secret: &str) -> String {
    let mut redacted = detail.to_string();
    if !secret.is_empty() {
        redacted = redacted.replace(secret, "[redacted]");
        redacted = redacted.replace(
            &format!("Authorization: Bearer {secret}"),
            "Authorization: Bearer [redacted]",
        );
    }
    redacted
}

fn parse_openai_chat_message(raw: &[u8]) -> Result<String, String> {
    let envelope: Value = serde_json::from_slice(raw)
        .map_err(|error| format!("provider returned invalid JSON: {error}"))?;
    envelope
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(openai_content_to_text)
        .ok_or_else(|| "provider response did not contain choices[0].message.content".to_string())
}

fn parse_api_message(provider: AiProvider, raw: &[u8]) -> Result<String, String> {
    match provider {
        AiProvider::Codex => parse_openai_response_message(raw),
        AiProvider::Claude => parse_anthropic_message(raw),
        AiProvider::Gemini => parse_gemini_message(raw),
        AiProvider::Zhipu | AiProvider::Kimi | AiProvider::Custom => parse_openai_chat_message(raw),
    }
}

fn parse_openai_response_message(raw: &[u8]) -> Result<String, String> {
    let envelope: Value = serde_json::from_slice(raw)
        .map_err(|error| format!("provider returned invalid JSON: {error}"))?;
    let mut text = String::new();
    if let Some(items) = envelope.get("output").and_then(Value::as_array) {
        for content in items
            .iter()
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flatten()
        {
            if content.get("type").and_then(Value::as_str) == Some("output_text")
                && let Some(value) = content.get("text").and_then(Value::as_str)
            {
                text.push_str(value);
            }
        }
    }
    (!text.is_empty())
        .then_some(text)
        .ok_or_else(|| "OpenAI response did not contain output text".to_string())
}

fn parse_anthropic_message(raw: &[u8]) -> Result<String, String> {
    let envelope: Value = serde_json::from_slice(raw)
        .map_err(|error| format!("provider returned invalid JSON: {error}"))?;
    let text = envelope
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    (!text.is_empty())
        .then_some(text)
        .ok_or_else(|| "Anthropic response did not contain text content".to_string())
}

fn parse_gemini_message(raw: &[u8]) -> Result<String, String> {
    let envelope: Value = serde_json::from_slice(raw)
        .map_err(|error| format!("provider returned invalid JSON: {error}"))?;
    let text = envelope
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.pointer("/content/parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    (!text.is_empty())
        .then_some(text)
        .ok_or_else(|| "Gemini response did not contain candidate text".to_string())
}

fn openai_content_to_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if let Some(value) = part.get("text").and_then(Value::as_str) {
                    text.push_str(value);
                    continue;
                }
                if part.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(value) = part.get("text").and_then(Value::as_str)
                {
                    text.push_str(value);
                }
            }
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
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
    use super::{
        AiProvider, CurlInvocation, bounded_error_detail, build_curl_invocation,
        endpoint_for_profile, openai_content_to_text, parse_anthropic_message,
        parse_external_response, parse_gemini_message, parse_openai_chat_message,
        parse_openai_response_message, redact_secret, validate_profile,
    };
    use crate::storage::{ApiProviderProfile, ProviderAuthMode};
    use serde_json::json;

    fn resolved(provider: AiProvider) -> super::ResolvedApiProvider {
        super::ResolvedApiProvider {
            provider,
            profile: ApiProviderProfile {
                auth_mode: ProviderAuthMode::ApiKey,
                label: provider.label().to_string(),
                base_url: "https://example.com/v1".to_string(),
                model: "demo-model".to_string(),
                api_key_env: "DEMO_API_KEY".to_string(),
            },
            api_key: "super-secret-key".to_string(),
            key_source: "session key".to_string(),
            endpoint_url: "https://example.com/v1/chat/completions".to_string(),
        }
    }

    fn endpoint(provider: AiProvider, base_url: &str) -> Result<String, String> {
        let mut profile = resolved(provider).profile;
        profile.base_url = base_url.to_string();
        endpoint_for_profile(provider, &profile)
    }

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

    #[test]
    fn parses_openai_chat_completions_string_content() {
        let raw =
            br#"{"choices":[{"message":{"content":"{\"message\":\"Ready\",\"action\":null}"}}]}"#;
        let message = parse_openai_chat_message(raw).unwrap();
        assert_eq!(message, r#"{"message":"Ready","action":null}"#);
    }

    #[test]
    fn parses_openai_chat_completions_array_content() {
        let raw = br#"{"choices":[{"message":{"content":[{"type":"text","text":"{\"message\":\"Ready\""},{"type":"text","text":",\"action\":null}"}]}}]}"#;
        let message = parse_openai_chat_message(raw).unwrap();
        assert_eq!(message, r#"{"message":"Ready","action":null}"#);
    }

    #[test]
    fn parses_native_provider_response_shapes() {
        let openai = br#"{"output":[{"content":[{"type":"output_text","text":"{\"message\":\"OpenAI\",\"action\":null}"}]}]}"#;
        let anthropic =
            br#"{"content":[{"type":"text","text":"{\"message\":\"Claude\",\"action\":null}"}]}"#;
        let gemini = br#"{"candidates":[{"content":{"parts":[{"text":"{\"message\":\"Gemini\",\"action\":null}"}]}}]}"#;

        assert!(
            parse_openai_response_message(openai)
                .unwrap()
                .contains("OpenAI")
        );
        assert!(
            parse_anthropic_message(anthropic)
                .unwrap()
                .contains("Claude")
        );
        assert!(parse_gemini_message(gemini).unwrap().contains("Gemini"));
    }

    #[test]
    fn builds_provider_specific_api_endpoints_and_auth_headers() {
        let mut openai = resolved(AiProvider::Codex);
        openai.endpoint_url = endpoint_for_profile(AiProvider::Codex, &openai.profile).unwrap();
        assert!(openai.endpoint_url.ends_with("/responses"));

        let mut claude = resolved(AiProvider::Claude);
        claude.endpoint_url = endpoint_for_profile(AiProvider::Claude, &claude.profile).unwrap();
        let invocation = build_curl_invocation(&claude, &json!({"ok": true})).unwrap();
        assert!(claude.endpoint_url.ends_with("/messages"));
        assert!(invocation.config.contains("x-api-key: super-secret-key"));
        assert!(invocation.config.contains("anthropic-version: 2023-06-01"));

        let mut gemini = resolved(AiProvider::Gemini);
        gemini.profile.model = "gemini-3.5-flash".to_string();
        gemini.endpoint_url = endpoint_for_profile(AiProvider::Gemini, &gemini.profile).unwrap();
        let invocation = build_curl_invocation(&gemini, &json!({"ok": true})).unwrap();
        assert!(
            gemini
                .endpoint_url
                .ends_with("/models/gemini-3.5-flash:generateContent")
        );
        assert!(
            invocation
                .config
                .contains("x-goog-api-key: super-secret-key")
        );
    }

    #[test]
    fn rejects_http_for_builtin_provider() {
        let error = endpoint(AiProvider::Zhipu, "http://api.z.ai/v1").unwrap_err();
        assert!(error.contains("requires HTTPS"));
    }

    #[test]
    fn allows_localhost_http_for_custom_provider() {
        let endpoint = endpoint(AiProvider::Custom, "http://localhost:11434/v1").unwrap();
        assert_eq!(endpoint, "http://localhost:11434/v1/chat/completions");
    }

    #[test]
    fn rejects_non_localhost_http_for_custom_provider() {
        let error = endpoint(AiProvider::Custom, "http://192.168.1.3:11434/v1").unwrap_err();
        assert!(error.contains("localhost"));
    }

    #[test]
    fn allows_ipv6_loopback_http_for_custom_provider() {
        let endpoint = endpoint(AiProvider::Custom, "http://[::1]:11434/v1").unwrap();
        assert_eq!(endpoint, "http://[::1]:11434/v1/chat/completions");
    }

    #[test]
    fn rejects_embedded_url_credentials() {
        let error = endpoint(AiProvider::Custom, "https://user:secret@example.com/v1").unwrap_err();
        assert!(error.contains("embedded credentials"));
    }

    #[test]
    fn rejects_invalid_key_environment_name() {
        let mut profile = resolved(AiProvider::Custom).profile;
        profile.api_key_env = "BAD-NAME".to_string();
        let error = validate_profile(AiProvider::Custom, &profile).unwrap_err();
        assert!(error.contains("valid variable name"));
    }

    #[test]
    fn curl_invocation_keeps_secret_out_of_argv_and_summary() {
        let invocation: CurlInvocation =
            build_curl_invocation(&resolved(AiProvider::Custom), &json!({"ok": true})).unwrap();
        let joined_args = invocation.args.join(" ");
        assert!(!joined_args.contains("super-secret-key"));
        assert!(!invocation.debug_summary.contains("super-secret-key"));
        assert!(
            invocation
                .config
                .contains("Authorization: Bearer super-secret-key")
        );
    }

    #[test]
    fn redacts_secret_from_error_detail() {
        let detail = bounded_error_detail(
            "curl: Authorization: Bearer super-secret-key denied",
            "super-secret-key",
            "summary",
        );
        assert!(!detail.contains("super-secret-key"));
        assert!(detail.contains("[redacted]"));
    }

    #[test]
    fn bounds_unicode_error_detail_on_a_character_boundary() {
        let detail = bounded_error_detail(&"오".repeat(200), "secret", "summary");
        assert!(detail.ends_with("..."));
        assert!(detail.len() <= super::MAX_ERROR_DETAIL_BYTES + 3);
    }

    #[test]
    fn extracts_openai_text_parts() {
        let text = openai_content_to_text(&json!([
            {"type": "text", "text": "hello "},
            {"type": "text", "text": "world"}
        ]))
        .unwrap();
        assert_eq!(text, "hello world");
    }

    #[test]
    fn redact_secret_noops_without_match() {
        let detail = redact_secret("permission denied", "super-secret-key");
        assert_eq!(detail, "permission denied");
    }
}
