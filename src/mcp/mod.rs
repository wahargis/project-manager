//! MCP (Model Context Protocol) server for project-manager.
//!
//! JSON-RPC server loop, tool schema definitions, and dispatch.
//! Tool implementations are in submodules:
//! - nodes: node CRUD tools (findings, decisions, hypotheses, etc.)
//! - edges: KG edge tools (add_edge, kg_traverse)
//! - dashboard: dashboard, next, scaffold, session_init
//! - review: review, stats
//!
//! Sprint 4 (#16): Auto-starts web dashboard on port 9090 in background thread.

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
use crate::store::{Store, NodeType, EdgeType};

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
            let tools = tool_definitions();
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
            if text.len() < 20 {
                "Error: hypothesis text must be at least 20 characters".to_string()
            } else {
                match store.create_hypothesis(phase_id, text) {
                    Ok(h) => {
                        let mut out = format!("Hypothesis #{} added: {}", h.id, &text[..text.len().min(80)]);
                        // Auto-edge from informing finding
                        if let Some(fid) = finding_id {
                            match store.create_edge(NodeType::Finding, fid, NodeType::Hypothesis, h.id, EdgeType::Supports) {
                                Ok(_) => out += &format!("\nAuto-edge: Finding#{} --Supports--> Hypothesis#{}", fid, h.id),
                                Err(e) => out += &format!("\nEdge note: {}", e),
                            }
                        } else {
                            out += &format!("\nWARNING: Hypothesis has no informing finding. Consider: pm_add_edge source_type=Finding source_id=? target_type=Hypothesis target_id={} relation=Supports", h.id);
                        }
                        // Set prediction/criteria if provided
                        if prediction.is_some() || criteria.is_some() {
                            let _ = store.update_hypothesis_fields(h.id, prediction, criteria, None);
                        }
                        out
                    }
                    Err(e) => format!("Error: {}", e),
                }
            }
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
            match store.set_session_experiment(eid) {
                Ok(()) => format!("Session active experiment set to #{}", eid),
                Err(e) => format!("Error: {}", e),
            }
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

/// Define all MCP tool schemas.
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "pm_dashboard".into(),
            description: "Cross-project priority dashboard. Shows highest-impact action across all active projects.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "pm_next".into(),
            description: "Next actions for a project with experiment summary, stagnation warning, and TaskCreate-ready top action.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "project": { "type": "string", "description": "Project name or alias" } },
                "required": ["project"]
            }),
        },
        ToolDef {
            name: "pm_review".into(),
            description: "Research health check: experiment velocity, stagnation, impact assessment, contradictions, orphaned nodes (all types), expired constraints.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "project": { "type": "string" } },
                "required": ["project"]
            }),
        },
        ToolDef {
            name: "pm_kg_traverse".into(),
            description: "Traverse KG from a node. Shows connected edges and nodes with direction.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"node_type": {"type": "string"}, "node_id": {"type": "integer"}}, "required": ["node_type", "node_id"]}),
        },
        ToolDef {
            name: "pm_scaffold".into(),
            description: "Phase detail with experiment roll-up, TaskCreate-ready pending experiments, active constraints, and active principles.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "phase_id": {"type": "integer"}}, "required": ["project", "phase_id"]}),
        },
        ToolDef {
            name: "pm_session_init".into(),
            description: "Returns TaskCreate-ready actionable tasks from DAG for all active projects. Detects stale hypotheses and orphaned findings. Call at session start.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "pm_session_context".into(),
            description: "Get focused context for the current session u{2014} extracts the active phase's knowledge subgraph with recent findings, decisions, hypotheses, and blocking issues.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["project"],
                "properties": {
                    "project": {"type": "string", "description": "Project name or alias"}
                }
            }),
        },
        ToolDef {
            name: "pm_experiment_create".into(),
            description: "BEFORE calling: use pm_search to check if a similar experiment already exists or has results. Create experiment with REQUIRED causal upstream — every experiment must link to what motivated it (finding, decision, or prior experiment). First experiment in a phase is exempt.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {
                "phase_id": {"type": "integer", "description": "Phase this experiment belongs to"},
                "name": {"type": "string", "description": "What investigation this experiment represents (min 10 chars)"},
                "informed_by_finding": {"type": "integer", "description": "Finding that motivated this experiment"},
                "informed_by_decision": {"type": "integer", "description": "Decision that directed this experiment"},
                "informed_by_experiment": {"type": "integer", "description": "Prior experiment this continues/branches from"}
            }, "required": ["phase_id", "name"]}),
        },
        ToolDef {
            name: "pm_exp_complete".into(),
            description: "Complete an experiment: set status + result + optionally create finding. Returns confirmation.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "status": {"type": "string", "description": "pass, fail, or inconclusive"}, "result": {"type": "string", "description": "Result summary"}, "finding": {"type": "string", "description": "Optional finding text to create"}}, "required": ["experiment_id", "status", "result"]}),
        },
        ToolDef {
            name: "pm_log_finding".into(),
            description: "Log an empirical finding from an experiment. After creation, auto-checks for related/contradicting findings. IMPORTANT: findings should be detailed lab reports (200+ chars) with methodology, data, and conclusions — not brief summaries.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "text": {"type": "string"}}, "required": ["experiment_id", "text"]}),
        },
        ToolDef {
            name: "pm_research_step".into(),
            description: "Log a finding with auto-routing. Finds the best active experiment in the project and creates the finding there. No experiment_id needed.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name"}, "text": {"type": "string", "description": "Finding text"}}, "required": ["project", "text"]}),
        },
        ToolDef {
            name: "pm_decision".into(),
            description: "Record a decision with rationale. 'why' is REQUIRED. Returns decision ID + recent findings for informed-by edges.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"what": {"type": "string", "description": "The decision made (min 50 chars)"}, "why": {"type": "string", "description": "Rationale, alternatives considered, evidence (REQUIRED, min 50 chars)"}, "experiment_id": {"type": "integer", "description": "Experiment that led to this decision (causal upstream)"}, "finding_ids": {"type": "string", "description": "Comma-separated finding IDs that informed this decision (causal upstream)"}, "project": {"type": "string", "description": "Project name to associate this decision with"}}, "required": ["what", "why"]}),
        },
        ToolDef {
            name: "pm_add_edge".into(),
            description: "Add a KG edge between two nodes.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"source_type": {"type": "string"}, "source_id": {"type": "integer"}, "target_type": {"type": "string"}, "target_id": {"type": "integer"}, "relation": {"type": "string", "description": "supports, contradicts, depends, informed, supersedes, related, produced, cited, contains, derived_from, tested_by, violated_by"}}, "required": ["source_type", "source_id", "target_type", "target_id", "relation"]}),
        },
        ToolDef {
            name: "pm_hyp_add".into(),
            description: "Create a hypothesis with optional causal grounding. Hypotheses should be informed by findings and testable by experiments.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "text": {"type": "string", "description": "Hypothesis text (min 20 chars)"}, "phase_id": {"type": "integer", "description": "Phase this hypothesis belongs to"}, "finding_id": {"type": "integer", "description": "Finding that informs this hypothesis (creates Supports edge)"}, "prediction": {"type": "string", "description": "Measurable predicted outcome"}, "criteria": {"type": "string", "description": "How to evaluate: what would confirm/refute this?"}}, "required": ["project", "text"]}),
        },
        ToolDef {
            name: "pm_hyp_update".into(),
            description: "Update hypothesis status with lifecycle enforcement. proposed->testing requires supporting evidence edge. testing->refuted requires finding_id. testing->confirmed suggests creating a principle.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"hypothesis_id": {"type": "integer"}, "status": {"type": "string", "description": "proposed, testing, confirmed, or refuted"}, "experiment_id": {"type": "integer", "description": "Experiment that tested this"}, "finding_id": {"type": "integer", "description": "Finding with evidence (REQUIRED for refuted)"}, "prediction": {"type": "string", "description": "Measurable predicted outcome"}, "criteria": {"type": "string", "description": "Evaluation criteria"}, "confidence": {"type": "number", "description": "Confidence level 0.0-1.0"}}, "required": ["hypothesis_id", "status"]}),
        },
        ToolDef {
            name: "pm_lit_add".into(),
            description: "Add a literature entry (paper, blog, reference). Requires authors + arxiv_id or url. Returns ID + phase edge suggestions.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "title": {"type": "string"}, "authors": {"type": "string", "description": "Author names (REQUIRED)"}, "arxiv_id": {"type": "string"}, "url": {"type": "string"}, "venue": {"type": "string", "description": "Publication venue (e.g., NeurIPS, ICML)"}, "year": {"type": "integer", "description": "Publication year"}, "code_url": {"type": "string", "description": "URL to code repository"}, "summary": {"type": "string", "description": "Brief summary of the paper"}, "relevance": {"type": "string", "description": "Relevance to project (min 100 chars)"}, "key_findings": {"type": "string", "description": "Key findings (min 200 chars)"}}, "required": ["project", "title"]}),
        },
        ToolDef {
            name: "pm_lit_status".into(),
            description: "Update literature status lifecycle: unread -> read -> cited -> tested -> dead_end/promising/integrated.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"literature_id": {"type": "integer"}, "status": {"type": "string", "description": "unread, read, cited, tested, dead_end, promising, or integrated"}}, "required": ["literature_id", "status"]}),
        },
        ToolDef {
            name: "pm_constraint_add".into(),
            description: "Add a hard constraint (hardware, budget, correctness requirement). Returns ID + phase/experiment edge suggestions.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "scope": {"type": "string", "description": "hardware, software, or process"}, "text": {"type": "string"}, "source": {"type": "string", "description": "Where this constraint comes from (REQUIRED)"}, "severity": {"type": "string", "description": "hard (default) or soft"}, "resource": {"type": "string", "description": "Resource being constrained (e.g., GPU VRAM, context window)"}, "measured_value": {"type": "string", "description": "Current measured value"}, "expires_at": {"type": "string", "description": "Expiry date (YYYY-MM-DD) -- pm_review flags expired constraints"}, "experiment_id": {"type": "integer", "description": "Experiment that tested/discovered this constraint (auto-creates TestedBy edge)"}}, "required": ["project", "scope", "text"]}),
        },
        ToolDef {
            name: "pm_research_complete".into(),
            description: "Complete a research/reflection action with a report.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"research_id": {"type": "integer"}, "status": {"type": "string", "description": "complete or abandoned"}, "report": {"type": "string", "description": "Research findings report"}, "phase_id": {"type": "integer", "description": "Phase this research belongs to (auto-creates Contains edge)"}, "finding_ids": {"type": "string", "description": "Comma-separated finding IDs that informed this research"}}, "required": ["research_id", "status"]}),
        },
        ToolDef {
            name: "pm_principle_add".into(),
            description: "Add a project-level principle or design guideline. Auto-creates DerivedFrom edges if finding_id or decision_id provided.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "scope": {"type": "string", "description": "universal, project, or phase"}, "text": {"type": "string"}, "rationale": {"type": "string", "description": "Why this principle matters"}, "enforcement_level": {"type": "string", "description": "advisory (default), recommended, or mandatory"}, "finding_id": {"type": "integer", "description": "Auto-create DerivedFrom edge to this finding"}, "decision_id": {"type": "integer", "description": "Auto-create DerivedFrom edge to this decision"}}, "required": ["project", "scope", "text"]}),
        },
        ToolDef {
            name: "pm_stats".into(),
            description: "KG node and edge counts for a project.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}}, "required": ["project"]}),
        },
        ToolDef {
            name: "pm_search".into(),
            description: "Search across all KG node types by text content. Returns ranked results with graph connectivity and evidence scoring. Use to find nodes without knowing IDs.".into(),
            input_schema: serde_json::json!({"type": "object", "required": ["query"], "properties": {"query": {"type": "string", "description": "Text to search for across all KG node types"}}}),
        },
        ToolDef {
            name: "pm_query".into(),
            description: "Natural language KG query. Searches, ranks, shows top 3 results with graph neighbors.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}),
        },
        ToolDef {
            name: "pm_orphan_repair".into(),
            description: "Deep structural KG analysis. Finds orphaned nodes, decisions without causal upstream, cross-project bleed, missing phase assignments. Returns specific repair actions.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name or alias"}}, "required": ["project"]}),
        },
        ToolDef {
            name: "pm_project_create".into(),
            description: "Create a new project or subproject. If parent is provided, creates as a subproject under the named parent.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"name": {"type": "string", "description": "Project name (required)"}, "alias": {"type": "string", "description": "Short alias for the project"}, "parent": {"type": "string", "description": "Parent project name or alias to create as subproject under"}}, "required": ["name"]}),
        },
        ToolDef {
            name: "pm_project_list".into(),
            description: "List all projects in a tree hierarchy showing parent/child relationships and node counts per project.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "pm_project_activate".into(),
            description: "Mark a project or subproject as active. Active projects appear in dashboard by default.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": {"type": "string", "description": "Project name or alias to activate"}
                }
            }),
        },
        ToolDef {
            name: "pm_project_deactivate".into(),
            description: "Mark a project or subproject as inactive. Inactive projects hidden from dashboard unless explicitly requested. Use for future planning projects.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": {"type": "string", "description": "Project name or alias to deactivate"}
                }
            }),
        },
        ToolDef {
            name: "pm_phase_update".into(),
            description: "Update phase details and status. Completion gating: all experiments must be resolved before completing. Auto-sets started_at/completed_at timestamps.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"phase_id": {"type": "integer"}, "description": {"type": "string"}, "goals": {"type": "string"}, "success_criteria": {"type": "string"}, "status": {"type": "string", "description": "pending, in_progress, complete, or paused"}}, "required": ["phase_id"]}),
        },

        ToolDef {
            name: "pm_session_set_experiment".into(),
            description: "Set the active experiment for the current session. Findings without explicit experiment_id will auto-route here.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}}, "required": ["experiment_id"]}),
        },
        ToolDef {
            name: "pm_session_start".into(),
            description: "Start a research session. Creates a timestamped session record. Call at the beginning of a work session.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name or alias (optional, scopes session to a project)"}}}),
        },
        ToolDef {
            name: "pm_session_end".into(),
            description: "End the current research session with an optional summary. Records end timestamp.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"summary": {"type": "string", "description": "Brief summary of what was accomplished this session"}}}),
        },
        ToolDef {
            name: "pm_set_confidence".into(),
            description: "Set confidence level on any TMS-enabled node (finding, decision, hypothesis, principle, constraint). Value 0.0-1.0.".into(),
            input_schema: serde_json::json!({"type": "object", "required": ["node_type", "node_id", "confidence"], "properties": {"node_type": {"type": "string", "description": "finding, decision, hypothesis, principle, or constraint"}, "node_id": {"type": "integer"}, "confidence": {"type": "number", "description": "Confidence level 0.0-1.0"}}}),
        },
        ToolDef {
            name: "pm_set_belief".into(),
            description: "Set belief status on any TMS-enabled node. When a node is contradicted, TMS auto-suspends dependents. Use this to manually believed/suspended/retracted.".into(),
            input_schema: serde_json::json!({"type": "object", "required": ["node_type", "node_id", "status"], "properties": {"node_type": {"type": "string", "description": "finding, decision, hypothesis, principle, or constraint"}, "node_id": {"type": "integer"}, "status": {"type": "string", "description": "believed, suspended, or retracted"}}}),
        },
        ToolDef {
            name: "pm_kg_audit".into(),
            description: "Comprehensive KG structural audit. Validates causal backbone compliance, hypothesis coverage, literature utilization, edge density, temporal coherence, cross-project references. Returns health score 0-100.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name or alias"}}, "required": ["project"]}),
        },
        ToolDef {
            name: "pm_since".into(),
            description: "Show all nodes created or modified since a date or session. Delta query for catching up on changes.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"since": {"type": "string", "description": "ISO date or datetime (e.g., '2026-03-20' or '2026-03-20 14:00:00')"}, "session_id": {"type": "integer", "description": "Show changes since this session started (alternative to 'since')"}}}),
        },
    ]
}
