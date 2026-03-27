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
                ToolDef {
                    name: "pm_kg_traverse".into(),
                    description: "Traverse KG from a node. Shows connected edges and nodes with direction.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {"node_type": {"type": "string"}, "node_id": {"type": "integer"}}, "required": ["node_type", "node_id"]}),
                },
                ToolDef {
                    name: "pm_scaffold".into(),
                    description: "List pending experiments in a phase as task items.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "phase_id": {"type": "integer"}}, "required": ["project", "phase_id"]}),
                },
                ToolDef {
                    name: "pm_session_init".into(),
                    description: "Returns actionable tasks from the DAG for all active projects. Call at session start to populate task tracker.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {}}),
                },
                ToolDef {
                    name: "pm_exp_complete".into(),
                    description: "Complete an experiment: set status + result + optionally create finding. Returns confirmation.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "status": {"type": "string", "description": "pass, fail, or inconclusive"}, "result": {"type": "string", "description": "Result summary"}, "finding": {"type": "string", "description": "Optional finding text to create"}}, "required": ["experiment_id", "status", "result"]}),
                },
                ToolDef {
                    name: "pm_log_finding".into(),
                    description: "Create a finding for an experiment. Returns finding ID + sibling findings for edge suggestions.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "text": {"type": "string"}}, "required": ["experiment_id", "text"]}),
                },
                ToolDef {
                    name: "pm_decision".into(),
                    description: "Record a decision with rationale. Returns decision ID + recent findings for informed-by edges.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {"what": {"type": "string"}, "why": {"type": "string"}, "experiment_id": {"type": "integer"}}, "required": ["what"]}),
                },
                ToolDef {
                    name: "pm_add_edge".into(),
                    description: "Add a KG edge between two nodes.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {"source_type": {"type": "string"}, "source_id": {"type": "integer"}, "target_type": {"type": "string"}, "target_id": {"type": "integer"}, "relation": {"type": "string", "description": "supports, contradicts, depends, informed, supersedes, related, produced, cited"}}, "required": ["source_type", "source_id", "target_type", "target_id", "relation"]}),
                },
                ToolDef {
                    name: "pm_stats".into(),
                    description: "KG node and edge counts for a project.".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}}, "required": ["project"]}),
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
                "pm_review" => { let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance"); tool_review(&store, p) }
                "pm_kg_traverse" => { let nt = args.get("node_type").and_then(|v| v.as_str()).unwrap_or("finding"); let nid = args.get("node_id").and_then(|v| v.as_i64()).unwrap_or(1); tool_kg_traverse(&store, nt, nid) }
                "pm_scaffold" => { let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance"); let pid = args.get("phase_id").and_then(|v| v.as_i64()).unwrap_or(0); tool_scaffold(&store, p, pid) }
                "pm_session_init" => { tool_session_init(&store) }
                "pm_exp_complete" => {
                    let eid = args.get("experiment_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let status = args.get("status").and_then(|v| v.as_str()).unwrap_or("pass");
                    let result = args.get("result").and_then(|v| v.as_str()).unwrap_or("");
                    let finding_text = args.get("finding").and_then(|v| v.as_str());
                    tool_exp_complete(&store, eid, status, result, finding_text)
                }
                "pm_log_finding" => {
                    let eid = args.get("experiment_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    tool_log_finding(&store, eid, text)
                }
                "pm_decision" => {
                    let what = args.get("what").and_then(|v| v.as_str()).unwrap_or("");
                    let why = args.get("why").and_then(|v| v.as_str());
                    let eid = args.get("experiment_id").and_then(|v| v.as_i64());
                    tool_decision(&store, what, why, eid)
                }
                "pm_add_edge" => {
                    let st = args.get("source_type").and_then(|v| v.as_str()).unwrap_or("finding");
                    let si = args.get("source_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let tt = args.get("target_type").and_then(|v| v.as_str()).unwrap_or("finding");
                    let ti = args.get("target_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let rel = args.get("relation").and_then(|v| v.as_str()).unwrap_or("related");
                    tool_add_edge(&store, st, si, tt, ti, rel)
                }
                "pm_stats" => { let p = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance"); tool_stats(&store, p) }
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

fn tool_review(store: &SqliteStore, project: &str) -> String {
    let proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(p) => p,
        None => return format!("Project not found: {}", project),
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
    // Enhanced checks
    let lit_count = store.list_literature(proj.id).map(|l| l.len()).unwrap_or(0);
    text += &format!("
Literature: {} entries. Check for new papers.
", lit_count);
    if let Ok(hyps) = store.list_hypotheses(None) {
        let proposed: Vec<_> = hyps.iter().filter(|h| h.status == crate::store::HypothesisStatus::Proposed).collect();
        if !proposed.is_empty() {
            text += &format!("
Hypotheses: {} untested
", proposed.len());
            for h in proposed.iter().take(3) {
                let t = if h.text.len() > 60 { &h.text[..60] } else { &h.text };
                text += &format!("  H#{}: {}
", h.id, t);
            }
        }
    }
    text
}

fn tool_kg_traverse(store: &SqliteStore, nt_str: &str, nid: i64) -> String {
    use crate::store::NodeType;
    let nt = match nt_str {
        "finding" | "f" => NodeType::Finding,
        "experiment" | "e" => NodeType::Experiment,
        "decision" | "d" => NodeType::Decision,
        "phase" | "p" => NodeType::Phase,
        "research" | "r" => NodeType::Research,
        "principle" | "pr" => NodeType::Principle,
        _ => return format!("Unknown node type: {}", nt_str),
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
            text
        }
        Err(e) => format!("Error: {}", e),
    }
}

fn tool_scaffold(store: &SqliteStore, project: &str, phase_id: i64) -> String {
    let _proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(p) => p,
        None => return format!("Project not found: {}", project),
    };
    let phase = match store.get_phase(phase_id) {
        Ok(p) => p,
        Err(e) => return format!("Phase not found: {}", e),
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
    text
}


fn tool_stats(store: &SqliteStore, project: &str) -> String { let proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) { Some(p) => p, None => return "Not found".to_string() }; let phases = store.list_phases(proj.id).unwrap_or_default(); let mut ec = 0; let mut fc = 0; for p in &phases { ec += store.list_experiments(Some(p.id)).map(|e| e.len()).unwrap_or(0); for e in store.list_experiments(Some(p.id)).unwrap_or_default() { fc += store.list_findings(Some(e.id)).map(|f| f.len()).unwrap_or(0); } } let t = format!("Phases:{} Exp:{} Find:{} Dec:{} Princ:{} Hyp:{} Con:{} Lit:{} Edges:{}", phases.len(), ec, fc, store.list_decisions(proj.id).map(|d| d.len()).unwrap_or(0), store.list_principles(proj.id).map(|p| p.len()).unwrap_or(0), store.list_hypotheses(None).map(|h| h.len()).unwrap_or(0), store.list_constraints(proj.id).map(|c| c.len()).unwrap_or(0), store.list_literature(proj.id).map(|l| l.len()).unwrap_or(0), store.list_all_edges().map(|e| e.len()).unwrap_or(0)); t }

fn tool_session_init(store: &SqliteStore) -> String {
    let mut out = String::new();
    if let Ok(projects) = store.list_projects() {
        for proj in &projects {
            if proj.status != crate::store::ProjectStatus::Active { continue; }
            let dag = DagEngine::new(store, proj.id);
            if let Ok(phases) = dag.next_phases() {
                // Get InProgress and Pending phases (skip Paused)
                let actionable: Vec<_> = phases.iter()
                    .filter(|p| p.status == crate::store::PhaseStatus::InProgress || p.status == crate::store::PhaseStatus::Pending)
                    .take(3)
                    .collect();
                
                for phase in &actionable {
                    if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                        let pending: Vec<_> = exps.iter()
                            .filter(|e| e.status == crate::store::ExperimentStatus::Pending)
                            .collect();
                        if !pending.is_empty() {
                            out += &format!("## [{}] Phase #{} [impact:{}]: {}\n", proj.name, phase.id, phase.impact, phase.name);
                            for exp in pending.iter().take(5) {
                                out += &format!("TASK: [{}] Exp #{}: {}\n", proj.name, exp.id, exp.name);
                                if let Some(notes) = &exp.notes {
                                    out += &format!("  {}\n", &notes[..notes.len().min(150)]);
                                }
                            }
                            out += "\n";
                        }
                    }
                }
            }
        }
    }
    if out.is_empty() {
        "No pending tasks in actionable phases.".to_string()
    } else {
        format!("=== Session Init: Actionable DAG Tasks ===\n\n{}\nCreate these as task tracker items and work through them.", out)
    }
}

fn tool_exp_complete(store: &SqliteStore, eid: i64, status: &str, result: &str, finding_text: Option<&str>) -> String {
    let es = match status {
        "pass" => crate::store::ExperimentStatus::Pass,
        "fail" => crate::store::ExperimentStatus::Fail,
        "inconclusive" => crate::store::ExperimentStatus::Inconclusive,
        _ => crate::store::ExperimentStatus::Pending,
    };
    if let Err(e) = store.update_experiment_status(eid, es, Some(result)) {
        return format!("Error updating experiment: {}", e);
    }
    let mut out = format!("Experiment #{} updated: status={}, result set.\n", eid, status);
    if let Some(text) = finding_text {
        match store.create_finding(Some(eid), text) {
            Ok(f) => { out += &format!("Finding #{} created.\n", f.id); }
            Err(e) => { out += &format!("Error creating finding: {}\n", e); }
        }
    }
    out
}

fn tool_log_finding(store: &SqliteStore, eid: i64, text: &str) -> String {
    let exp_id = if eid > 0 { Some(eid) } else { None };
    match store.create_finding(exp_id, text) {
        Ok(f) => {
            let mut out = format!("Finding #{} created", f.id);
            if text.len() < 200 {
                out += &format!(" (WARNING: {} chars < 200 minimum for lab report format)", text.len());
            }
            out += ".\n";
            // Show siblings for edge suggestions
            if let Some(eid) = exp_id {
                if let Ok(siblings) = store.list_findings(Some(eid)) {
                    let others: Vec<_> = siblings.iter().filter(|s| s.id != f.id).collect();
                    if !others.is_empty() {
                        out += "\nSibling findings (same experiment):\n";
                        for s in others.iter().take(5) {
                            let t = if s.text.len() > 60 { &s.text[..60] } else { &s.text };
                            out += &format!("  F#{}: {}\n", s.id, t);
                        }
                        out += &format!("\nSuggest: pm_add_edge source_type=finding source_id={} target_type=finding target_id={} relation=supports\n", f.id, others[0].id);
                    }
                }
            }
            out
        }
        Err(e) => format!("Error: {}", e),
    }
}

fn tool_decision(store: &SqliteStore, what: &str, why: Option<&str>, experiment_id: Option<i64>) -> String {
    match store.create_decision(experiment_id, what, why) {
        Ok(d) => {
            let mut out = format!("Decision #{} created: {}\n", d.id, d.what);
            // Suggest informed-by edges from recent findings
            if let Ok(findings) = store.list_findings(None) {
                let recent: Vec<_> = findings.iter().rev().take(5).collect();
                if !recent.is_empty() {
                    out += "\nRecent findings (suggest informed-by edges):\n";
                    for f in &recent {
                        let t = if f.text.len() > 60 { &f.text[..60] } else { &f.text };
                        out += &format!("  F#{}: {}\n", f.id, t);
                    }
                    out += &format!("\nSuggest: pm_add_edge source_type=finding source_id={} target_type=decision target_id={} relation=informed\n", recent[0].id, d.id);
                }
            }
            out
        }
        Err(e) => format!("Error: {}", e),
    }
}

fn tool_add_edge(store: &SqliteStore, st: &str, si: i64, tt: &str, ti: i64, rel: &str) -> String {
    use crate::store::{NodeType, EdgeType};
    let source_type = match st {
        "finding" | "f" => NodeType::Finding,
        "experiment" | "e" => NodeType::Experiment,
        "decision" | "d" => NodeType::Decision,
        "phase" | "p" => NodeType::Phase,
        "research" | "r" => NodeType::Research,
        "principle" | "pr" => NodeType::Principle,
        "hypothesis" | "h" => NodeType::Hypothesis,
        "constraint" | "co" => NodeType::Constraint,
        "literature" | "l" => NodeType::Literature,
        "feedback" | "fb" => NodeType::Feedback,
        _ => return format!("Unknown source type: {}", st),
    };
    let target_type = match tt {
        "finding" | "f" => NodeType::Finding,
        "experiment" | "e" => NodeType::Experiment,
        "decision" | "d" => NodeType::Decision,
        "phase" | "p" => NodeType::Phase,
        "research" | "r" => NodeType::Research,
        "principle" | "pr" => NodeType::Principle,
        "hypothesis" | "h" => NodeType::Hypothesis,
        "constraint" | "co" => NodeType::Constraint,
        "literature" | "l" => NodeType::Literature,
        "feedback" | "fb" => NodeType::Feedback,
        _ => return format!("Unknown target type: {}", tt),
    };
    let relation = match rel {
        "supports" => EdgeType::Supports,
        "contradicts" => EdgeType::Contradicts,
        "depends" => EdgeType::DependsOn,
        "informed" => EdgeType::Informed,
        "supersedes" => EdgeType::Supersedes,
        "related" => EdgeType::RelatedTo,
        "produced" => EdgeType::ProducedBy,
        "cited" => EdgeType::CitedIn,
        _ => return format!("Unknown relation: {}", rel),
    };
    match store.create_edge(source_type, si, target_type, ti, relation) {
        Ok(e) => format!("Edge #{} added: {:?} #{} --{:?}--> {:?} #{}", e.id, e.source_type, si, e.relation, e.target_type, ti),
        Err(e) => format!("Error: {}", e),
    }
}
