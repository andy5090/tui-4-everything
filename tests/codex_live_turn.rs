use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::codex::app_server::CodexAppServer;
use t4e::codex::service::{bounded_action_schema, planner_prompt};

static LIVE_CODEX_LOCK: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "uses the signed-in Codex plan; run explicitly for release verification"]
fn signed_in_codex_completes_a_streamed_turn() {
    let _guard = LIVE_CODEX_LOCK.lock().expect("live Codex lock");
    if Command::new("codex").arg("--version").output().is_err() {
        return;
    }
    let structured = run_structured_turn("Set message to exactly T4E_CODEX_OK and action to null.");
    assert_eq!(structured["message"], "T4E_CODEX_OK");
    assert!(structured["action"].is_null());
}

#[test]
#[ignore = "uses the signed-in Codex plan; run explicitly for release verification"]
fn signed_in_codex_completes_tui_catalog_intent() {
    let _guard = LIVE_CODEX_LOCK.lock().expect("live Codex lock");
    if Command::new("codex").arg("--version").output().is_err() {
        return;
    }
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog");
    let workspaces = load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces");
    let environment_context = format!(
        "Catalog tool IDs: {}\nWorkspace IDs: {}",
        catalog
            .tools
            .iter()
            .map(|tool| tool.id.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        workspaces
            .workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let prompt = planner_prompt(&environment_context, "Find ripgrep in the catalog");

    let structured = run_structured_turn(&prompt);
    assert_eq!(
        structured["action"]["type"], "catalog_search",
        "unexpected planner response: {structured}"
    );
    assert_eq!(
        structured["action"]["target"], "ripgrep",
        "unexpected planner response: {structured}"
    );
}

#[test]
#[ignore = "uses the signed-in Codex plan; run explicitly for release verification"]
fn signed_in_codex_identifies_itself_as_t4e_control_plane() {
    let _guard = LIVE_CODEX_LOCK.lock().expect("live Codex lock");
    if Command::new("codex").arg("--version").output().is_err() {
        return;
    }
    let prompt = planner_prompt(
        "platform: linux\ninstall queue: empty\ncatalog apps: yazi=Yazi (run: yazi)\nworkspaces: video-desk=Video Desk (mux: Tmux, apps: yazi, state: stopped)",
        "What environment are you operating inside, and what manages app execution? Do not propose an action.",
    );
    let structured = run_structured_turn(&prompt);
    let message = structured["message"]
        .as_str()
        .expect("planner message")
        .to_ascii_lowercase();
    assert!(
        message.contains("t4e"),
        "unexpected planner response: {structured}"
    );
    assert!(
        message.contains("control") || message.contains("manage"),
        "unexpected planner response: {structured}"
    );
    assert!(structured["action"].is_null());
}

fn run_structured_turn(prompt: &str) -> serde_json::Value {
    let mut client = CodexAppServer::spawn().expect("Codex app-server starts");
    client.initialize().expect("initializes");
    let cwd = std::env::current_dir().expect("cwd");
    let thread = client
        .start_thread(cwd.to_str().expect("utf8 cwd"))
        .expect("thread starts");
    client
        .start_turn_structured_with_model(
            &thread,
            prompt,
            bounded_action_schema(),
            &compatible_model(),
        )
        .expect("turn starts");

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut text = String::new();
    let mut trace = Vec::new();
    while Instant::now() < deadline {
        let Some(message) = client
            .recv_timeout(Duration::from_secs(1))
            .expect("event reads")
        else {
            continue;
        };
        trace.push(message.to_string());
        match message.get("method").and_then(serde_json::Value::as_str) {
            Some("item/completed") => {
                if let Some(final_text) = message
                    .pointer("/params/item/text")
                    .and_then(serde_json::Value::as_str)
                {
                    text = final_text.to_string();
                }
            }
            Some("turn/completed") => {
                assert_eq!(
                    message.pointer("/params/turn/status"),
                    Some(&serde_json::Value::String("completed".to_string())),
                    "Codex turn failed: {message}; trace={trace:#?}"
                );
                return serde_json::from_str(&text).unwrap_or_else(|error| {
                    panic!("Codex returned structured JSON: {error}; trace={trace:#?}")
                });
            }
            _ => {}
        }
    }
    panic!("Codex turn did not complete before timeout; trace={trace:#?}");
}

fn compatible_model() -> String {
    std::env::var("T4E_CODEX_FALLBACK_MODEL").unwrap_or_else(|_| "gpt-5.4".to_string())
}
