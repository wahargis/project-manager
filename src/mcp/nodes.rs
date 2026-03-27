//! MCP tool implementations for node CRUD operations.
//!
//! Contains: pm_log_finding, pm_decision, pm_lit_add, pm_lit_status,
//!           pm_exp_complete, pm_hyp_update, pm_research_complete,
//!           pm_principle_add, pm_constraint_add

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, NodeType, EdgeType, HypothesisStatus, ExperimentStatus};
use crate::validation;
use std::collections::HashSet;

pub fn tool_log_finding(store: &SqliteStore, eid: i64, text: &str) -> String {
    let v = validation::validate_finding(text);
    if !v.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", v.to_mcp_error());
    }
    let exp_id = if eid > 0 { Some(eid) } else { None };
    match store.create_finding(exp_id, text) {
        Ok(f) => {
            let mut out = format!("Finding #{} created", f.id);
            // Warn about orphaned findings (no experiment)
            if exp_id.is_none() {
                out += " (WARNING: no experiment_id — orphaned findings are less useful for traceability)";
            }
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
                // Suggest edges to the experiment
                out += &format!("\nSuggest: pm_add_edge source_type=finding source_id={} target_type=experiment target_id={} relation=produced\n", f.id, eid);
            }
            // Suggest edges to recent literature
            // We need a project_id to list literature — derive from experiment -> phase -> project
            if let Some(eid_val) = exp_id {
                if let Ok(exp) = store.get_experiment(eid_val) {
                    if let Some(phase_id) = exp.phase_id {
                        if let Ok(phase) = store.get_phase(phase_id) {
                            if let Ok(lit_entries) = store.list_literature(phase.project_id) {
                                let recent: Vec<_> = lit_entries.iter().rev().take(3).collect();
                                if !recent.is_empty() {
                                    out += "\nRecent literature (suggest cited edges):\n";
                                    for l in &recent {
                                        let t = if l.title.len() > 60 { &l.title[..60] } else { &l.title };
                                        out += &format!("  L#{}: {}\n", l.id, t);
                                    }
                                    out += &format!("\nSuggest: pm_add_edge source_type=finding source_id={} target_type=literature target_id={} relation=cited\n", f.id, recent[0].id);
                                }
                            }
                        }
                    }
                }
            }
            // Surface active principles for the project (#12)
            if let Some(eid_val) = exp_id {
                if let Ok(exp) = store.get_experiment(eid_val) {
                    if let Some(phase_id) = exp.phase_id {
                        if let Ok(phase) = store.get_phase(phase_id) {
                            if let Ok(principles) = store.list_principles(phase.project_id) {
                                let active: Vec<_> = principles.iter()
                                    .filter(|p| p.status == crate::store::PrincipleStatus::Active)
                                    .collect();
                                if !active.is_empty() {
                                    out += "\nActive principles for this project:\n";
                                    for p in active.iter().take(5) {
                                        let t = if p.text.len() > 80 { &p.text[..80] } else { &p.text };
                                        out += &format!("  P#{}: {}\n", p.id, t);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            out
        }
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_decision(store: &SqliteStore, what: &str, why: Option<&str>, experiment_id: Option<i64>, project_name: Option<&str>) -> String {
    let v = validation::validate_decision(what, why);
    if !v.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", v.to_mcp_error());
    }
    // Resolve project name to project_id
    let project_id = if let Some(pname) = project_name {
        match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == pname || p.alias.as_deref() == Some(pname))) {
            Some(proj) => Some(proj.id),
            None => return format!("Project not found: {}", pname),
        }
    } else {
        None
    };
    match store.create_decision(experiment_id, what, why, project_id) {
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

pub fn tool_lit_add(store: &SqliteStore, args: &serde_json::Value) -> String {
    let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let authors = args.get("authors").and_then(|v| v.as_str());
    let arxiv = args.get("arxiv_id").and_then(|v| v.as_str());
    let url = args.get("url").and_then(|v| v.as_str());
    let rel = args.get("relevance").and_then(|v| v.as_str());
    let kf = args.get("key_findings").and_then(|v| v.as_str());
    let venue = args.get("venue").and_then(|v| v.as_str());
    let year = args.get("year").and_then(|v| v.as_i64()).map(|y| y as i32);
    let code_url = args.get("code_url").and_then(|v| v.as_str());
    let summary = args.get("summary").and_then(|v| v.as_str());

    let lv = validation::validate_literature(title, authors, arxiv, url, kf, rel);
    if !lv.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", lv.to_mcp_error());
    }
    match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(proj) => match store.create_literature(proj.id, title, arxiv, rel, kf, authors, venue, year, url, code_url, summary) {
            Ok(l) => {
                let mut out = format!("Literature #{} added: {}", l.id, l.title);

                // Edge suggestions: find phases with overlapping keywords (#15)
                if let Ok(phases) = store.list_phases(proj.id) {
                    let title_words: HashSet<&str> = title.split_whitespace()
                        .filter(|w| w.len() > 3)
                        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                        .filter(|w| !w.is_empty())
                        .collect();
                    for phase in &phases {
                        let phase_words: HashSet<&str> = phase.name.split_whitespace()
                            .filter(|w| w.len() > 3)
                            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                            .filter(|w| !w.is_empty())
                            .collect();
                        let overlap: Vec<&&str> = title_words.intersection(&phase_words).collect();
                        if !overlap.is_empty() {
                            out += &format!("\nSuggest: pm_add_edge source_type=literature source_id={} target_type=phase target_id={} relation=informed", l.id, phase.id);
                        }
                    }
                }

                out
            }
            Err(e) => format!("Error: {}", e),
        },
        None => format!("Project not found: {}", project),
    }
}

pub fn tool_lit_status(store: &SqliteStore, literature_id: i64, status: &str) -> String {
    let sv = validation::validate_status("literature", status);
    if !sv.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", sv.to_mcp_error());
    }
    match store.update_literature_status(literature_id, status) {
        Ok(_) => format!("Literature #{} status updated to '{}'", literature_id, status),
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_exp_complete(store: &SqliteStore, eid: i64, status: &str, result: &str, finding_text: Option<&str>) -> String {
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

pub fn tool_hyp_update(store: &SqliteStore, hid: i64, status_str: &str, experiment_id: Option<i64>, finding_id: Option<i64>, prediction: Option<&str>, criteria: Option<&str>, confidence: Option<f64>) -> String {
    let hs = match status_str {
        "confirmed" => HypothesisStatus::Confirmed,
        "refuted" => HypothesisStatus::Refuted,
        "testing" => HypothesisStatus::Testing,
        _ => HypothesisStatus::Proposed,
    };

    // Get current hypothesis to check current status
    let current_hyp = match store.get_hypothesis(hid) {
        Ok(h) => h,
        Err(e) => return format!("Error fetching hypothesis #{}: {}", hid, e),
    };

    // Lifecycle enforcement: proposed -> testing requires supporting evidence
    if current_hyp.status == HypothesisStatus::Proposed && hs == HypothesisStatus::Testing {
        // Check for at least 1 incoming edge (supports, informed, produced)
        let has_evidence = match store.get_edges_to(NodeType::Hypothesis, hid) {
            Ok(edges) => edges.iter().any(|e| {
                matches!(e.relation, EdgeType::Supports | EdgeType::Informed | EdgeType::ProducedBy)
            }),
            Err(_) => false,
        };
        if !has_evidence {
            return format!(
                "\u{274c} Cannot transition to testing: hypothesis #{} has no supporting evidence.\n\
                 Add at least one edge (e.g., pm_add_edge source_type=finding source_id=X target_type=hypothesis target_id={} relation=supports)",
                hid, hid
            );
        }
    }

    // Lifecycle enforcement: testing -> refuted requires finding_id
    if current_hyp.status == HypothesisStatus::Testing && hs == HypothesisStatus::Refuted {
        if finding_id.is_none() {
            return format!(
                "\u{274c} Cannot refute hypothesis #{} without a disproving finding.\n\
                 Provide finding_id parameter with the finding that disproves this hypothesis.",
                hid
            );
        }
        // Auto-create Contradicts edge: Finding --Contradicts--> Hypothesis
        if let Some(fid) = finding_id {
            match store.create_edge(NodeType::Finding, fid, NodeType::Hypothesis, hid, EdgeType::Contradicts) {
                Ok(edge) => {
                    // Edge created successfully, will report below
                    let _ = edge;
                }
                Err(e) => return format!("Error creating contradiction edge: {}", e),
            }
        }
    }

    // Update hypothesis fields (prediction, criteria, confidence) if provided
    if prediction.is_some() || criteria.is_some() || confidence.is_some() {
        if let Err(e) = store.update_hypothesis_fields(hid, prediction, criteria, confidence) {
            return format!("Error updating hypothesis fields: {}", e);
        }
    }

    match store.update_hypothesis(hid, hs.clone(), experiment_id, finding_id) {
        Ok(_) => {
            let mut out = format!("Hypothesis #{} updated to {:?}", hid, hs);

            // Auto-suggestion for confirmed: suggest creating a principle
            if hs == HypothesisStatus::Confirmed {
                out += &format!(
                    "\n\nHypothesis confirmed. Consider creating a principle:\n  pm_principle_add project=<project> scope=project text=\"<principle derived from hypothesis #{}>\"\n",
                    hid
                );
            }

            // Auto-suggestion for refuted: mention the contradiction edge
            if hs == HypothesisStatus::Refuted {
                if let Some(fid) = finding_id {
                    out += &format!("\nAuto-created edge: Finding #{} --Contradicts--> Hypothesis #{}\n", fid, hid);
                }
            }

            out
        }
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_research_complete(store: &SqliteStore, rid: i64, status_str: &str, report: Option<&str>) -> String {
    let rs = match status_str {
        _ => crate::store::ResearchStatus::Complete,
    };
    match store.update_research(rid, rs.clone(), report) {
        Ok(_) => format!("Research #{} updated to {:?}", rid, rs),
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_principle_add(store: &SqliteStore, args: &serde_json::Value) -> String {
    let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
    let scope_str = args.get("scope").and_then(|v| v.as_str()).unwrap_or("methodology");
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let rationale = args.get("rationale").and_then(|v| v.as_str());
    let enforcement_level = args.get("enforcement_level").and_then(|v| v.as_str());
    let finding_id = args.get("finding_id").and_then(|v| v.as_i64());
    let decision_id = args.get("decision_id").and_then(|v| v.as_i64());

    let pv = validation::validate_principle(text, rationale);
    if !pv.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", pv.to_mcp_error());
    }
    let scope = match scope_str {
        "universal" | "architecture" => crate::store::PrincipleScope::Universal,
        "phase" | "process" => crate::store::PrincipleScope::Phase,
        _ => crate::store::PrincipleScope::Project,
    };
    match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(proj) => match store.create_principle(proj.id, scope, text, rationale, enforcement_level) {
            Ok(pr) => {
                let mut out = format!("Principle #{} added: {}", pr.id, &text[..text.len().min(80)]);

                // Auto-create DerivedFrom edge if finding_id provided (#12)
                if let Some(fid) = finding_id {
                    match store.create_edge(NodeType::Principle, pr.id, NodeType::Finding, fid, EdgeType::DerivedFrom) {
                        Ok(e) => out += &format!("\nAuto-created edge: Principle #{} --DerivedFrom--> Finding #{} (Edge #{})", pr.id, fid, e.id),
                        Err(e) => out += &format!("\nWarning: failed to create DerivedFrom edge to Finding #{}: {}", fid, e),
                    }
                }

                // Auto-create DerivedFrom edge if decision_id provided (#12)
                if let Some(did) = decision_id {
                    match store.create_edge(NodeType::Principle, pr.id, NodeType::Decision, did, EdgeType::DerivedFrom) {
                        Ok(e) => out += &format!("\nAuto-created edge: Principle #{} --DerivedFrom--> Decision #{} (Edge #{})", pr.id, did, e.id),
                        Err(e) => out += &format!("\nWarning: failed to create DerivedFrom edge to Decision #{}: {}", did, e),
                    }
                }

                // Suggest edges to related constraints (#15)
                if let Ok(constraints) = store.list_constraints(proj.id) {
                    if !constraints.is_empty() {
                        out += "\n\nActive constraints (suggest related edges):\n";
                        for c in constraints.iter().take(5) {
                            let t = if c.text.len() > 60 { &c.text[..60] } else { &c.text };
                            let sev = c.severity.as_deref().unwrap_or("hard");
                            out += &format!("  C#{} [{}]: {}\n", c.id, sev, t);
                        }
                        out += &format!("\nSuggest: pm_add_edge source_type=principle source_id={} target_type=constraint target_id={} relation=related\n", pr.id, constraints[0].id);
                    }
                }

                out
            }
            Err(e) => format!("Error: {}", e),
        },
        None => format!("Project not found: {}", project),
    }
}

pub fn tool_constraint_add(store: &SqliteStore, args: &serde_json::Value) -> String {
    let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("volta-renaissance");
    let scope_str = args.get("scope").and_then(|v| v.as_str()).unwrap_or("hardware");
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let source = args.get("source").and_then(|v| v.as_str());
    let severity = args.get("severity").and_then(|v| v.as_str());
    let resource = args.get("resource").and_then(|v| v.as_str());
    let measured_value = args.get("measured_value").and_then(|v| v.as_str());
    let expires_at = args.get("expires_at").and_then(|v| v.as_str());

    let cv = validation::validate_constraint(text, source);
    if !cv.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", cv.to_mcp_error());
    }
    let scope = match scope_str {
        "software" => crate::store::ConstraintScope::Software,
        "process" => crate::store::ConstraintScope::Process,
        _ => crate::store::ConstraintScope::Hardware,
    };
    match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(proj) => match store.create_constraint(proj.id, scope, text, source, severity, resource, measured_value, expires_at) {
            Ok(con) => {
                let mut out = format!("Constraint #{} added: {}", con.id, &text[..text.len().min(80)]);

                // Suggest edges to relevant phases and experiments (#15)
                if let Ok(phases) = store.list_phases(proj.id) {
                    let text_words: HashSet<&str> = text.split_whitespace()
                        .filter(|w| w.len() > 3)
                        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                        .filter(|w| !w.is_empty())
                        .collect();
                    for phase in &phases {
                        let phase_words: HashSet<&str> = phase.name.split_whitespace()
                            .filter(|w| w.len() > 3)
                            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                            .filter(|w| !w.is_empty())
                            .collect();
                        let overlap: Vec<&&str> = text_words.intersection(&phase_words).collect();
                        if !overlap.is_empty() {
                            out += &format!("\nSuggest: pm_add_edge source_type=constraint source_id={} target_type=phase target_id={} relation=related", con.id, phase.id);
                        }
                    }
                    // Also suggest edges to pending experiments in matching phases
                    for phase in &phases {
                        if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                            let pending: Vec<_> = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).collect();
                            for exp in pending.iter().take(3) {
                                out += &format!("\nSuggest: pm_add_edge source_type=constraint source_id={} target_type=experiment target_id={} relation=related", con.id, exp.id);
                            }
                        }
                    }
                }

                out
            }
            Err(e) => format!("Error: {}", e),
        },
        None => format!("Project not found: {}", project),
    }
}

/// Update a phase: description, goals, success_criteria, status with completion gating.
pub fn tool_phase_update(store: &SqliteStore, phase_id: i64, description: Option<&str>, goals: Option<&str>, success_criteria: Option<&str>, status: Option<&str>) -> String {
    // Verify phase exists
    let phase = match store.get_phase(phase_id) {
        Ok(p) => p,
        Err(e) => return format!("Phase not found: {}", e),
    };

    // Update description/goals/success_criteria if provided
    if description.is_some() || goals.is_some() || success_criteria.is_some() {
        if let Err(e) = store.update_phase_fields(phase_id, description, goals, success_criteria) {
            return format!("Error updating phase fields: {}", e);
        }
    }

    if let Some(status_str) = status {
        let sv = validation::validate_status("phase", status_str);
        if !sv.is_ok() {
            return format!("\u{274c} VALIDATION ERROR:\n{}", sv.to_mcp_error());
        }

        let new_status = match status_str {
            "in_progress" => crate::store::PhaseStatus::InProgress,
            "complete" => crate::store::PhaseStatus::Complete,
            "paused" => crate::store::PhaseStatus::Paused,
            _ => crate::store::PhaseStatus::Pending,
        };

        // Completion gating: if transitioning to "complete", check all experiments are resolved
        if new_status == crate::store::PhaseStatus::Complete {
            if let Ok(exps) = store.list_experiments(Some(phase_id)) {
                let pending: Vec<_> = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).collect();
                if !pending.is_empty() {
                    let mut out = format!("\u{274c} Cannot complete phase #{}: {} pending experiment(s):\n", phase_id, pending.len());
                    for e in &pending {
                        out += &format!("  Exp #{}: {}\n", e.id, e.name);
                    }
                    out += "\nResolve all experiments (pass/fail/inconclusive) before completing the phase.";
                    return out;
                }
            }
        }

        // Set started_at when transitioning to in_progress
        if new_status == crate::store::PhaseStatus::InProgress && phase.started_at.is_none() {
            if let Err(e) = store.set_phase_started(phase_id) {
                return format!("Error setting phase started_at: {}", e);
            }
        }

        // Set completed_at when transitioning to complete
        if new_status == crate::store::PhaseStatus::Complete {
            if let Err(e) = store.set_phase_completed(phase_id) {
                return format!("Error setting phase completed_at: {}", e);
            }
        }

        if let Err(e) = store.update_phase_status(phase_id, new_status.clone()) {
            return format!("Error updating phase status: {}", e);
        }

        return format!("Phase #{} ({}) updated to {:?}", phase_id, phase.name, new_status);
    }

    format!("Phase #{} ({}) fields updated", phase_id, phase.name)
}
