//! MCP (Model Context Protocol) server for project-manager.
//!
//! JSON-RPC server loop and dispatch. Tool schemas and implementations
//! are in submodules:
//! - tools: Tool schema definitions (ToolDef, tool_definitions)
//! - nodes: node CRUD tools (findings, decisions, hypotheses, etc.)
//! - edges: KG edge tools (add_edge, kg_traverse)
//! - dashboard: dashboard, next, scaffold, session_init
//! - review: review, stats
//!
//! Sprint 4 (#16): Auto-starts web dashboard on port 9090 in background thread.

pub mod tools;
pub mod nodes;
pub mod edges;
pub mod dashboard;
pub mod review;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use crate::validation;
use std::io::{BufRead, Write};

use crate::store::sqlite::SqliteStore;


#[derive(Deserialize)]
#[allow(dead_code)]
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

fn default_db_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    format!("{}/.local/share/pm/pm.db", home)
}

pub fn run_mcp_server() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let db_path = std::env::var("PM_DB").unwrap_or_else(|_| default_db_path());

    // Sprint 4 (#16): Auto-start web dashboard in background thread
    let web_port: u16 = std::env::var("PM_WEB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9090);
    let db_path_for_web = db_path.clone();
    std::thread::spawn(move || {
        // Pre-check: if port is already in use, warn and skip
        match std::net::TcpListener::bind(("0.0.0.0", web_port)) {
            Ok(listener) => {
                // Port is free -- drop the test listener and start the web server
                drop(listener);
                eprintln!("[pm-mcp] Starting web dashboard on port {}...", web_port);
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("[pm-mcp] WARNING: Failed to create tokio runtime for web dashboard: {}", e);
                        return;
                    }
                };
                // Catch panics from warp::serve in case of race condition on port binding
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rt.block_on(async {
                        crate::web::serve(&db_path_for_web, web_port).await;
                    });
                }));
                if let Err(_) = result {
                    eprintln!("[pm-mcp] WARNING: Web dashboard failed to start (port {} may have become unavailable). MCP continues without dashboard.", web_port);
                }
            }
            Err(e) => {
                eprintln!("[pm-mcp] WARNING: Web dashboard port {} already in use ({}). Dashboard not started -- use existing instance or set PM_WEB_PORT.", web_port, e);
            }
        }
    });

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
                "serverInfo": { "name": "pm", "version": "5.0.0" }
            })),
            error: None,
        },
        "tools/list" => {
            let defs = tools::tool_definitions();
            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: Some(serde_json::json!({ "tools": defs })),
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

            let output = dispatch_tool(&store, tool_name, &args);

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

/// Dispatch a tool call to the appropriate submodule function.
fn dispatch_tool(store: &SqliteStore, tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        // Dashboard tools
        "pm_dashboard" => dashboard::tool_dashboard(store),
        "pm_next" => {
            let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            dashboard::tool_next(store, project)
        },
        "pm_scaffold" => {
            let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            let pid = args.get("phase_id").and_then(|v| v.as_i64()).unwrap_or(0);
            dashboard::tool_scaffold(store, p, pid)
        },
        "pm_session_init" => dashboard::tool_session_init(store),
        "pm_session_context" => {
            let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("");
            dashboard::tool_session_context(store, project)
        },

        // Review tools
        "pm_review" => {
            let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            review::tool_review(store, p)
        },
        "pm_stats" => {
            let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            review::tool_stats(store, p)
        },
        "pm_search" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            review::tool_search(store, q)
        },
        "pm_query" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            review::tool_query(store, q)
        },
        "pm_orphan_repair" => {
            let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            review::tool_orphan_repair(store, p)
        },

        // Edge tools
        "pm_kg_traverse" => {
            let nt = args.get("node_type").and_then(|v| v.as_str()).unwrap_or("finding");
            let nid = args.get("node_id").and_then(|v| v.as_i64()).unwrap_or(1);
            edges::tool_kg_traverse(store, nt, nid)
        },
        "pm_add_edge" => {
            let st = args.get("source_type").and_then(|v| v.as_str()).unwrap_or("finding");
            let si = args.get("source_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let tt = args.get("target_type").and_then(|v| v.as_str()).unwrap_or("finding");
            let ti = args.get("target_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let rel = args.get("relation").and_then(|v| v.as_str()).unwrap_or("related");
            edges::tool_add_edge(store, st, si, tt, ti, rel)
        },

        // Node tools
        "pm_log_finding" => {
            let eid = args.get("experiment_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            nodes::tool_log_finding(store, eid, text)
        },
        "pm_research_step" => {
            let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("");
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            nodes::tool_research_step(store, project, text)
        },
        "pm_decision" => {
            let what = args.get("what").and_then(|v| v.as_str()).unwrap_or("");
            let why = args.get("why").and_then(|v| v.as_str());
            let eid = args.get("experiment_id").and_then(|v| v.as_i64());
            let finding_ids = args.get("finding_ids").and_then(|v| v.as_str());
            let project = args.get("project").and_then(|v| v.as_str());
            nodes::tool_decision(store, what, why, eid, finding_ids, project)
        },
        "pm_experiment_create" => {
            let phase_id = args.get("phase_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let ibf = args.get("informed_by_finding").and_then(|v| v.as_i64());
            let ibd = args.get("informed_by_decision").and_then(|v| v.as_i64());
            let ibe = args.get("informed_by_experiment").and_then(|v| v.as_i64());
            nodes::tool_experiment_create(store, phase_id, name, ibf, ibd, ibe)
        },
        "pm_exp_complete" => {
            let eid = args.get("experiment_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let status = args.get("status").and_then(|v| v.as_str()).unwrap_or("pass");
            let result = args.get("result").and_then(|v| v.as_str()).unwrap_or("");
            let finding_text = args.get("finding").and_then(|v| v.as_str());
            let sv = validation::validate_status("experiment", status);
            if !sv.is_ok() {
                format!("\u{274c} VALIDATION ERROR:\n{}", sv.to_mcp_error())
            } else {
                nodes::tool_exp_complete(store, eid, status, result, finding_text)
            }
        },
        "pm_hyp_add" => {
            let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("");
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let phase_id = args.get("phase_id").and_then(|v| v.as_i64());
            let finding_id = args.get("finding_id").and_then(|v| v.as_i64());
            let prediction = args.get("prediction").and_then(|v| v.as_str());
            let criteria = args.get("criteria").and_then(|v| v.as_str());
            nodes::tool_hyp_add(store, project, text, phase_id, finding_id, prediction, criteria)
        },
        "pm_hyp_update" => {
            let hid = args.get("hypothesis_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let status_str = args.get("status").and_then(|v| v.as_str()).unwrap_or("proposed");
            let sv = validation::validate_status("hypothesis", status_str);
            if !sv.is_ok() {
                format!("\u{274c} VALIDATION ERROR:\n{}", sv.to_mcp_error())
            } else {
                let eid = args.get("experiment_id").and_then(|v| v.as_i64());
                let fid = args.get("finding_id").and_then(|v| v.as_i64());
                let prediction = args.get("prediction").and_then(|v| v.as_str());
                let criteria = args.get("criteria").and_then(|v| v.as_str());
                let confidence = args.get("confidence").and_then(|v| v.as_f64());
                nodes::tool_hyp_update(store, hid, status_str, eid, fid, prediction, criteria, confidence)
            }
        },
        "pm_lit_add" => nodes::tool_lit_add(store, args),
        "pm_lit_status" => {
            let lid = args.get("literature_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let status = args.get("status").and_then(|v| v.as_str()).unwrap_or("read");
            nodes::tool_lit_status(store, lid, status)
        },
        "pm_constraint_add" => nodes::tool_constraint_add(store, args),
        "pm_research_complete" => {
            let rid = args.get("research_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let status_str = args.get("status").and_then(|v| v.as_str()).unwrap_or("complete");
            let report = args.get("report").and_then(|v| v.as_str());
            let rv = validation::validate_status("research", status_str);
            if !rv.is_ok() {
                format!("\u{274c} VALIDATION ERROR:\n{}", rv.to_mcp_error())
            } else {
                let phase_id = args.get("phase_id").and_then(|v| v.as_i64());
                let finding_ids = args.get("finding_ids").and_then(|v| v.as_str());
                nodes::tool_research_complete(store, rid, status_str, report, phase_id, finding_ids)
            }
        },
        "pm_principle_add" => nodes::tool_principle_add(store, args),
        "pm_phase_update" => {
            let phase_id = args.get("phase_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let description = args.get("description").and_then(|v| v.as_str());
            let goals = args.get("goals").and_then(|v| v.as_str());
            let success_criteria = args.get("success_criteria").and_then(|v| v.as_str());
            let status = args.get("status").and_then(|v| v.as_str());
            nodes::tool_phase_update(store, phase_id, description, goals, success_criteria, status)
        },

        // Project CRUD tools
        "pm_project_create" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let alias = args.get("alias").and_then(|v| v.as_str());
            let parent = args.get("parent").and_then(|v| v.as_str());
            dashboard::tool_project_create(store, name, alias, parent)
        },
        "pm_project_list" => dashboard::tool_project_list(store),
        "pm_project_activate" => {
            let n = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            dashboard::tool_project_set_status(store, n, true)
        }
        "pm_project_deactivate" => {
            let n = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            dashboard::tool_project_set_status(store, n, false)
        }

        // Temporal Awareness tools (Feature 5)
        "pm_session_start" => {
            let project = args.get("project").and_then(|v| v.as_str());
            dashboard::tool_session_start(store, project)
        },
        "pm_session_set_experiment" => {
            let eid = args.get("experiment_id").and_then(|v| v.as_i64()).unwrap_or(0);
            dashboard::tool_session_set_experiment(store, eid)
        },
        "pm_session_end" => {
            let summary = args.get("summary").and_then(|v| v.as_str());
            dashboard::tool_session_end(store, summary)
        },
        "pm_since" => {
            let since = args.get("since").and_then(|v| v.as_str());
            let session_id = args.get("session_id").and_then(|v| v.as_i64());
            dashboard::tool_since(store, since, session_id)
        },

        // TMS (Truth-Maintenance System) tools
        "pm_set_confidence" => {
            let nt = args.get("node_type").and_then(|v| v.as_str()).unwrap_or("finding");
            let nid = args.get("node_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let conf = args.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5);
            edges::tool_set_confidence(store, nt, nid, conf)
        },
        "pm_set_belief" => {
            let nt = args.get("node_type").and_then(|v| v.as_str()).unwrap_or("finding");
            let nid = args.get("node_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let status = args.get("status").and_then(|v| v.as_str()).unwrap_or("believed");
            edges::tool_set_belief(store, nt, nid, status)
        },

        "pm_kg_audit" => {
            let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            review::tool_kg_audit(store, p)
        },

        _ => format!("Unknown tool: {}", tool_name),
    }
}
