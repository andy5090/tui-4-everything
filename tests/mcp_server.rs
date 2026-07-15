use std::io::{BufReader, Cursor};
use std::path::Path;

use serde_json::{Value, json};
use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::mcp::{MCP_PROTOCOL_VERSION, run_server};

fn invoke(messages: &[Value]) -> Vec<Value> {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog");
    let workspaces = load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspaces");
    let input = messages
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    run_server(
        BufReader::new(Cursor::new(input)),
        &mut output,
        &catalog,
        &workspaces,
    )
    .expect("MCP server runs");
    String::from_utf8(output)
        .expect("utf8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect()
}

fn handshake() -> Vec<Value> {
    vec![
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" }
            }
        }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ]
}

#[test]
fn mcp_lifecycle_lists_structured_tools() {
    let mut messages = handshake();
    messages.push(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));

    let responses = invoke(&messages);

    assert_eq!(
        responses[0]["result"]["protocolVersion"],
        MCP_PROTOCOL_VERSION
    );
    let tools = responses[1]["result"]["tools"]
        .as_array()
        .expect("tool list");
    assert!(tools.iter().any(|tool| tool["name"] == "catalog_search"));
    assert!(tools.iter().any(|tool| tool["name"] == "workspace_launch"));
}

#[test]
fn mcp_read_tools_return_registry_data_and_side_effects_fail_closed() {
    let mut messages = handshake();
    messages.extend([
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "catalog_search", "arguments": { "query": "ripgrep" } }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "install_plan", "arguments": { "toolId": "ripgrep", "platform": "linux" } }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "workspace_launch", "arguments": { "workspaceId": "video-desk" } }
        }),
    ]);

    let responses = invoke(&messages);

    assert_eq!(
        responses[1]["result"]["structuredContent"]["tools"][0]["id"],
        "ripgrep"
    );
    assert_eq!(
        responses[2]["result"]["structuredContent"]["task"]["check_command"],
        "rg"
    );
    assert_eq!(responses[3]["result"]["isError"], true);
    assert!(
        responses[3]["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("explicit approval")
    );
}

#[test]
fn mcp_unknown_tools_are_protocol_errors() {
    let mut messages = handshake();
    messages.push(json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": { "name": "run_arbitrary_shell", "arguments": {} }
    }));

    let responses = invoke(&messages);

    assert_eq!(responses[1]["error"]["code"], -32602);
}
