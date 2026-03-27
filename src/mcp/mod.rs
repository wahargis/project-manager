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
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            crate::web::serve(&db_path_for_web, web_port).await;
        });
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
                "serverInfo": { "name": "pm", "version": "4.0.0" }
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

        // Review tools
        "pm_review" => {
            let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            review::tool_review(store, p)
        },
        "pm_stats" => {
            let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
            review::tool_stats(store, p)
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
        "pm_decision" => {
            let what = args.get("what").and_then(|v| v.as_str()).unwrap_or("");
            let why = args.get("why").and_then(|v| v.as_str());
            let eid = args.get("experiment_id").and_then(|v| v.as_i64());
            let finding_ids = args.get("finding_ids").and_then(|v| v.as_str());
            let project = args.get("project").and_then(|v| v.as_str());
            nodes::tool_decision(store, what, why, eid, finding_ids, project)
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
            name: "pm_exp_complete".into(),
            description: "Complete an experiment: set status + result + optionally create finding. Returns confirmation.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "status": {"type": "string", "description": "pass, fail, or inconclusive"}, "result": {"type": "string", "description": "Result summary"}, "finding": {"type": "string", "description": "Optional finding text to create"}}, "required": ["experiment_id", "status", "result"]}),
        },
        ToolDef {
            name: "pm_log_finding".into(),
            description: "Create a finding for an experiment. Min 100 chars required. Returns finding ID + sibling findings for edge suggestions. Warns if no experiment_id (orphan).".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "text": {"type": "string", "description": "Finding text (min 100 chars). Include: what was observed, conditions, implications."}}, "required": ["text"]}),
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
            name: "pm_phase_update".into(),
            description: "Update phase details and status. Completion gating: all experiments must be resolved before completing. Auto-sets started_at/completed_at timestamps.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"phase_id": {"type": "integer"}, "description": {"type": "string"}, "goals": {"type": "string"}, "success_criteria": {"type": "string"}, "status": {"type": "string", "description": "pending, in_progress, complete, or paused"}}, "required": ["phase_id"]}),
        },
    ]
}
