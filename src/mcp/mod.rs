//! MCP (Model Context Protocol) server for project-manager.
//! Provides tools that claude-code can call:
//! - pm_dashboard: cross-project priority view
//! - pm_next: next action for a project
//! - pm_review: research health check
//! - pm_scaffold: decompose phase into tasks

use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

use crate::store::sqlite::SqliteStore;
use crate::store::Store;
use crate::dag::DagEngine;
use crate::store::PhaseStatus;

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Serialize)]
struct ToolDef {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: serde_json::Value,
}

fn default_db_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    format!("{}/.local/share/pm/pm.db", home)
}

pub fn run_mcp_server() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let db_path = std::env::var("PM_DB").unwrap_or_else(|_| default_db_path());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(JsonRpcError { code: -32700, message: format!("Parse error: {}", e) }),
                };
                writeln!(out, "{}", serde_json::to_string(&resp).unwrap()).ok();
                continue;
            }
        };

        let resp = handle_request(&req, &db_path);
        writeln!(out, "{}", serde_json::to_string(&resp).unwrap()).ok();
        out.flush().ok();
    }
}

fn handle_request(req: &JsonRpcRequest, db_path: &str) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id.clone(),
            result: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "pm", "version": "3.0.0" }
            })),
            error: None,
        },
        "tools/list" => {
            let tools = vec![
                ToolDef {
                    name: "pm_dashboard".into(),
                    description: "Cross-project priority dashboard. Shows highest-impact action across all active projects.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {}}),
                },
                ToolDef {
                    name: "pm_next".into(),
                    description: "Next actions for a project, impact-weighted with stagnation warning.".into(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "project": { "type": "string", "description": "Project name or alias" } },
                        "required": ["project"]
                    }),
                },
                ToolDef {
                    name: "pm_review".into(),
                    description: "Research health check: experiment velocity, stagnation, impact assessment, contradictions.".into(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "project": { "type": "string" } },
                        "required": ["project"]
                    }),
                },
            ];
            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: Some(serde_json::json!({ "tools": tools })),
                error: None,
            }
        },
        "tools/call" => {
            let tool_name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = req.params.get("arguments").cloned().unwrap_or_default();
            
            let store = match SqliteStore::new(db_path) {
                Ok(s) => s,
                Err(e) => return JsonRpcResponse {
                    jsonrpc: "2.0".into(), id: req.id.clone(),
                    result: None, error: Some(JsonRpcError { code: -1, message: format!("DB error: {}", e) }),
                },
            };

            let output = match tool_name {
                "pm_dashboard" => tool_dashboard(&store),
                "pm_next" => {
                    let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
                    tool_next(&store, project)
                },
                _ => format!("Unknown tool: {}", tool_name),
            };

            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: Some(serde_json::json!({
                    "content": [{ "type": "text", "text": output }]
                })),
                error: None,
            }
        },
        "notifications/initialized" => JsonRpcResponse {
            jsonrpc: "2.0".into(), id: req.id.clone(),
            result: Some(serde_json::json!({})), error: None,
        },
        _ => JsonRpcResponse {
            jsonrpc: "2.0".into(), id: req.id.clone(),
            result: None, error: Some(JsonRpcError { code: -32601, message: format!("Method not found: {}", req.method) }),
        },
    }
}

fn tool_dashboard(store: &SqliteStore) -> String {
    let mut out = String::from("=== Cross-Project Dashboard ===\n\n");
    if let Ok(projects) = store.list_projects() {
        for proj in &projects {
            if proj.status != crate::store::ProjectStatus::Active { continue; }
            let dag = DagEngine::new(store, proj.id);
            if let Ok(next) = dag.next_phases() {
                if let Some(top) = next.first() {
                    let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                    out += &format!("  [{}] {} #{} [impact:{}] {}\n", proj.name, s, top.id, top.impact, top.name);
                }
            }
        }
    }
    out += "\n## ACTION: Execute the highest-impact item above.";
    out
}

fn tool_next(store: &SqliteStore, project: &str) -> String {
    let mut out = String::new();
    if let Ok(projects) = store.list_projects() {
        if let Some(proj) = projects.iter().find(|p| p.name == project || p.alias.as_deref() == Some(project)) {
            let dag = DagEngine::new(store, proj.id);
            if let Ok(next) = dag.next_phases() {
                out += "=== Next Phases (by impact) ===\n\n";
                for phase in next.iter().take(3) {
                    let s = if phase.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                    out += &format!("  {} #{} [impact:{}] {}\n", s, phase.id, phase.impact, phase.name);
                }
            }
            if let Ok(Some(n)) = dag.stagnation_check(3) {
                out += &format!("\n  WARNING: STAGNATION — {} consecutive failed experiments\n", n);
            }
            out += "\n## ACTION: Execute the top phase.";
        } else {
            out += &format!("Project not found: {}", project);
        }
    }
    out
}
