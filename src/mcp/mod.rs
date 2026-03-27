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

fn tool_review(store: &SqliteStore, project: &str) -> serde_json::Value {
    let proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(p) => p,
        None => return serde_json::json!({"content": [{"type": "text", "text": format!("Project not found: {}", project)}]}),
    };
    let kg = crate::kg::KgEngine::new(store);
    let mut text = format!("=== Research Review: {} ===\n\n", proj.name);
    let mut total = 0; let mut pass = 0; let mut fail = 0; let mut pending = 0;
    if let Ok(phases) = store.list_phases(proj.id) {
        for phase in &phases {
            if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                for exp in &exps {
                    total += 1;
                    match exp.status {
                        crate::store::ExperimentStatus::Pass => pass += 1,
                        crate::store::ExperimentStatus::Fail => fail += 1,
                        crate::store::ExperimentStatus::Pending => pending += 1,
                        _ => {}
                    }
                }
            }
        }
    }
    text += &format!("## Experiments: {} total, {} pass, {} fail, {} pending\n", total, pass, fail, pending);
    let dag = DagEngine::new(store, proj.id);
    if let Ok(Some(n)) = dag.stagnation_check(3) {
        text += &format!("\n## STAGNATION: {} consecutive fails\n", n);
    }
    if let Ok(next) = dag.next_phases() {
        text += "\n## Top phases by impact:\n";
        for p in next.iter().take(3) {
            text += &format!("  #{} [impact:{}] {:?} {}\n", p.id, p.impact, p.status, p.name);
        }
    }
    let findings = store.list_findings(None).unwrap_or_default();
    let contradictions = kg.find_contradictions(&findings).unwrap_or_default();
    if !contradictions.is_empty() {
        text += &format!("\n## Contradictions: {}\n", contradictions.len());
    }
    serde_json::json!({"content": [{"type": "text", "text": text}]})
}

fn tool_kg_traverse(store: &SqliteStore, nt_str: &str, nid: i64) -> serde_json::Value {
    use crate::store::NodeType;
    let nt = match nt_str {
        "finding" | "f" => NodeType::Finding,
        "experiment" | "e" => NodeType::Experiment,
        "decision" | "d" => NodeType::Decision,
        "phase" | "p" => NodeType::Phase,
        "research" | "r" => NodeType::Research,
        "principle" | "pr" => NodeType::Principle,
        _ => return serde_json::json!({"content": [{"type": "text", "text": format!("Unknown node type: {}", nt_str)}]}),
    };
    let kg = crate::kg::KgEngine::new(store);
    match kg.traverse(nt, nid) {
        Ok(result) => {
            let mut text = format!("ROOT: {:?} #{}: {}\n", result.root.node_type, result.root.id, &result.root.label[..result.root.label.len().min(100)]);
            for (edge, target, incoming) in &result.edges {
                if *incoming {
                    text += &format!("  <--{:?}-- {:?} #{}: {}\n", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(80)]);
                } else {
                    text += &format!("  --{:?}--> {:?} #{}: {}\n", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(80)]);
                }
            }
            serde_json::json!({"content": [{"type": "text", "text": text}]})
        }
        Err(e) => serde_json::json!({"content": [{"type": "text", "text": format!("Error: {}", e)}]}),
    }
}

fn tool_scaffold(store: &SqliteStore, project: &str, phase_id: i64) -> serde_json::Value {
    let _proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(p) => p,
        None => return serde_json::json!({"content": [{"type": "text", "text": format!("Project not found: {}", project)}]}),
    };
    let phase = match store.get_phase(phase_id) {
        Ok(p) => p,
        Err(e) => return serde_json::json!({"content": [{"type": "text", "text": format!("Phase not found: {}", e)}]}),
    };
    let exps = store.list_experiments(Some(phase_id)).unwrap_or_default();
    let pending: Vec<_> = exps.iter().filter(|e| e.status == crate::store::ExperimentStatus::Pending).collect();
    let mut text = format!("=== Phase #{} ({}) — {} pending experiments ===\n\n", phase.id, phase.name, pending.len());
    for e in &pending {
        text += &format!("TASK: Exp #{}: {}\n", e.id, e.name);
        if let Some(notes) = &e.notes {
            text += &format!("  {}\n", &notes[..notes.len().min(200)]);
        }
        text += "\n";
    }
    serde_json::json!({"content": [{"type": "text", "text": text}]})
}

