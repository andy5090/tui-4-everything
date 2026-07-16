use std::io::{BufRead, Write};

use anyhow::Result;
use serde_json::{Value, json};

use crate::catalog::models::{CatalogRegistry, Platform};
use crate::installer::engine::{InstallPolicy, build_install_task};
use crate::mux::runtime::TmuxRuntime;
use crate::mux::workspace::WorkspaceRegistry;

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

pub fn run_server(
    reader: impl BufRead,
    mut writer: impl Write,
    catalog: &CatalogRegistry,
    workspaces: &WorkspaceRegistry,
) -> Result<()> {
    let mut initialized = false;
    for line in reader.lines() {
        let line = line?;
        let request = match serde_json::from_str::<Value>(&line) {
            Ok(request) => request,
            Err(error) => {
                write_json(
                    &mut writer,
                    &protocol_error(Value::Null, -32700, &error.to_string()),
                )?;
                continue;
            }
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let id = request.get("id").cloned();
        if method == "notifications/initialized" {
            initialized = true;
            continue;
        }
        let Some(id) = id else { continue };
        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "t4e",
                        "title": "t4e Terminal Environment",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": "Read-only discovery is available. Side effects require approval in the t4e TUI."
                }
            }),
            _ if !initialized => protocol_error(id, -32002, "server is not initialized"),
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": tool_definitions() }
            }),
            "tools/call" => {
                let name = request.pointer("/params/name").and_then(Value::as_str);
                let arguments = request
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match name {
                    Some(name) if is_known_tool(name) => {
                        match call_tool(name, &arguments, catalog, workspaces) {
                            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                            Err(error) => json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": tool_result(json!({ "error": error.to_string() }), true)
                            }),
                        }
                    }
                    Some(name) => protocol_error(id, -32602, &format!("unknown tool: {name}")),
                    None => protocol_error(id, -32602, "tools/call requires params.name"),
                }
            }
            _ => protocol_error(id, -32601, &format!("method not found: {method}")),
        };
        write_json(&mut writer, &response)?;
    }
    Ok(())
}

fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        "catalog_search"
            | "install_plan"
            | "workspace_list"
            | "workspace_launch"
            | "workspace_stop"
            | "app_start"
            | "app_stop"
    )
}

fn call_tool(
    name: &str,
    arguments: &Value,
    catalog: &CatalogRegistry,
    workspaces: &WorkspaceRegistry,
) -> Result<Value> {
    let structured = match name {
        "catalog_search" => {
            let query = required_string(arguments, "query")?.to_ascii_lowercase();
            let tools = catalog
                .tools
                .iter()
                .filter(|tool| {
                    tool.id.to_ascii_lowercase().contains(&query)
                        || tool.name.to_ascii_lowercase().contains(&query)
                        || tool
                            .tags
                            .iter()
                            .any(|tag| tag.to_ascii_lowercase().contains(&query))
                })
                .take(20)
                .map(|tool| {
                    json!({
                        "id": tool.id,
                        "name": tool.name,
                        "risk": tool.risk,
                        "runCommand": tool.run_command_for_current_platform()
                    })
                })
                .collect::<Vec<_>>();
            json!({ "tools": tools })
        }
        "install_plan" => {
            let tool_id = required_string(arguments, "toolId")?;
            let platform = match arguments.get("platform").and_then(Value::as_str).unwrap_or(
                if cfg!(target_os = "macos") {
                    "macos"
                } else {
                    "linux"
                },
            ) {
                "macos" => Platform::Macos,
                "linux" => Platform::Linux,
                value => anyhow::bail!("unsupported platform: {value}"),
            };
            let tool = catalog
                .tools
                .iter()
                .find(|tool| tool.id == tool_id)
                .ok_or_else(|| anyhow::anyhow!("unknown tool: {tool_id}"))?;
            let installer = tool
                .installers
                .iter()
                .find(|installer| installer.platform == platform)
                .ok_or_else(|| anyhow::anyhow!("no installer for {tool_id}"))?;
            let task = build_install_task(tool, installer, &InstallPolicy::default())?;
            json!({ "toolId": tool_id, "risk": tool.risk, "task": task })
        }
        "workspace_list" => {
            let managed = TmuxRuntime::default().list_managed().unwrap_or_default();
            let items = workspaces
                .workspaces
                .iter()
                .map(|workspace| {
                    let session = managed
                        .iter()
                        .find(|session| session.workspace_id == workspace.id);
                    json!({
                        "id": workspace.id,
                        "title": workspace.title,
                        "mux": workspace.mux,
                        "recommendedTools": workspace.recommended_tools,
                        "running": session.is_some(),
                        "session": session.map(|session| session.name.as_str())
                    })
                })
                .collect::<Vec<_>>();
            json!({ "workspaces": items })
        }
        "workspace_launch" | "workspace_stop" | "app_start" | "app_stop" => {
            return Ok(tool_result(
                json!({
                    "error": "This action requires explicit approval in the t4e TUI",
                    "action": name
                }),
                true,
            ));
        }
        _ => anyhow::bail!("unknown tool: {name}"),
    };
    Ok(tool_result(structured, false))
}

fn tool_result(structured: Value, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": structured.to_string() }],
        "structuredContent": structured,
        "isError": is_error
    })
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "catalog_search",
            "title": "Search t4e catalog",
            "description": "Search curated terminal applications without side effects.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"],
                "additionalProperties": false
            }
        },
        {
            "name": "install_plan",
            "title": "Plan a tool install",
            "description": "Build a validated install plan without executing it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "toolId": { "type": "string" },
                    "platform": { "type": "string", "enum": ["macos", "linux"] }
                },
                "required": ["toolId"],
                "additionalProperties": false
            }
        },
        {
            "name": "workspace_list",
            "title": "List t4e workspaces",
            "description": "List templates and live managed tmux session status.",
            "inputSchema": { "type": "object", "additionalProperties": false }
        },
        {
            "name": "workspace_launch",
            "title": "Launch a workspace",
            "description": "Request workspace launch; requires approval in the t4e TUI.",
            "inputSchema": {
                "type": "object",
                "properties": { "workspaceId": { "type": "string" } },
                "required": ["workspaceId"],
                "additionalProperties": false
            }
        },
        {
            "name": "workspace_stop",
            "title": "Stop a workspace",
            "description": "Request workspace stop; requires approval in the t4e TUI.",
            "inputSchema": {
                "type": "object",
                "properties": { "workspaceId": { "type": "string" } },
                "required": ["workspaceId"],
                "additionalProperties": false
            }
        },
        {
            "name": "app_start",
            "title": "Start a terminal app",
            "description": "Request app start; requires approval in the t4e TUI.",
            "inputSchema": {
                "type": "object",
                "properties": { "toolId": { "type": "string" } },
                "required": ["toolId"],
                "additionalProperties": false
            }
        },
        {
            "name": "app_stop",
            "title": "Stop a terminal app",
            "description": "Request app stop; requires approval in the t4e TUI.",
            "inputSchema": {
                "type": "object",
                "properties": { "toolId": { "type": "string" } },
                "required": ["toolId"],
                "additionalProperties": false
            }
        }
    ])
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing string argument: {field}"))
}

fn protocol_error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn write_json(writer: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
