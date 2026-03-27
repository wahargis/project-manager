//! MCP tool implementations for node CRUD operations.
//!
//! Contains: pm_log_finding, pm_decision, pm_lit_add, pm_lit_status,
//!           pm_exp_complete, pm_hyp_update, pm_research_complete,
//!           pm_principle_add, pm_constraint_add
//!
//! Issue #19: Causal backbone edge enforcement — auto-creates edges
//! based on node type's position in the causal chain:
//!   Phase → Experiment → Finding → Decision → Hypothesis/Research

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, NodeType, EdgeType, HypothesisStatus, ExperimentStatus};
use crate::validation;
use std::collections::HashSet;

/// Helper: build the causal guidance section for a response.
/// `auto_created` — list of auto-created edge descriptions.
/// `suggestions` — list of (description, pm_add_edge command) tuples.
fn causal_guidance(auto_created: &[String], suggestions: &[(String, String)]) -> String {
    if auto_created.is_empty() && suggestions.is_empty() {
        return String::new();
    }
    let mut out = "\n\n=== Causal Links ===\n".to_string();
    if !auto_created.is_empty() {
        out += "Auto-created:\n";
        for ac in auto_created {
            out += &format!("  + {}\n", ac);
        }
    }
    if !suggestions.is_empty() {
        out += "Suggested (specify if applicable):\n";
        for (desc, cmd) in suggestions {
            out += &format!("  → {}: {}\n", desc, cmd);
        }
    }
    out
}

/// Parse comma-separated IDs from a string.
pub fn parse_ids(s: &str) -> Vec<i64> {
    s.split(',')
        .filter_map(|part| part.trim().parse::<i64>().ok())
        .collect()
}

pub fn tool_log_finding(store: &SqliteStore, eid: i64, text: &str) -> String {
    let v = validation::validate_finding(text);
    if !v.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", v.to_mcp_error());
    }
    let exp_id = if eid > 0 { Some(eid) } else { None };
    match store.create_finding(exp_id, text) {
        Ok(f) => {
            let fref = f.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", f.id));
            let mut out = format!("Finding {} created (global #{})", fref, f.id);
            let mut auto_edges: Vec<String> = Vec::new();
            let mut suggestions: Vec<(String, String)> = Vec::new();

            // Auto-create Experiment --Produced--> Finding edge
            if let Some(eid_val) = exp_id {
                match store.create_edge(NodeType::Experiment, eid_val, NodeType::Finding, f.id, EdgeType::ProducedBy) {
                    Ok(_) => auto_edges.push(format!("Experiment#{} --ProducedBy--> Finding#{}", eid_val, f.id)),
                    Err(e) => out += &format!(" (edge auto-create note: {})", e),
                }

                // #19: Auto-create Phase --Contains--> Finding if experiment has phase_id
                if let Ok(exp) = store.get_experiment(eid_val) {
                    if let Some(phase_id) = exp.phase_id {
                        match store.create_edge(NodeType::Phase, phase_id, NodeType::Finding, f.id, EdgeType::Contains) {
                            Ok(_) => auto_edges.push(format!("Phase#{} --Contains--> Finding#{}", phase_id, f.id)),
                            Err(e) => out += &format!(" (phase edge note: {})", e),
                        }
                    }
                }
            }

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
                            let sref = s.project_seq.map(|seq| format!("#{}", seq)).unwrap_or_else(|| format!("#{}", s.id));
                            out += &format!("  F{}: {}\n", sref, t);
                        }
                    }
                }
            }

            // Suggest edges to recent literature
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
                                        let lref = l.project_seq.map(|seq| format!("#{}", seq)).unwrap_or_else(|| format!("#{}", l.id));
                                        out += &format!("  L{}: {}\n", lref, t);
                                    }
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
                                        let prref = p.project_seq.map(|seq| format!("#{}", seq)).unwrap_or_else(|| format!("#{}", p.id));
                                    out += &format!("  P{}: {}\n", prref, t);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // #19: Causal guidance — Finding is produced by experiments, informs decisions/hypotheses
            suggestions.push((
                "If this finding informs a decision".to_string(),
                format!("pm_add_edge source_type=Finding source_id={} target_type=Decision target_id=? relation=Informed", f.id),
            ));
            suggestions.push((
                "If this finding supports a hypothesis".to_string(),
                format!("pm_add_edge source_type=Finding source_id={} target_type=Hypothesis target_id=? relation=Supports", f.id),
            ));
            suggestions.push((
                "If this finding contradicts a hypothesis".to_string(),
                format!("pm_add_edge source_type=Finding source_id={} target_type=Hypothesis target_id=? relation=Contradicts", f.id),
            ));

            out += &causal_guidance(&auto_edges, &suggestions);
            out
        }
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_decision(store: &SqliteStore, what: &str, why: Option<&str>, experiment_id: Option<i64>, finding_ids_str: Option<&str>, project_name: Option<&str>) -> String {
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

    // Parse finding_ids from comma-separated string
    let finding_ids: Vec<i64> = finding_ids_str
        .map(|s| parse_ids(s))
        .unwrap_or_default();

    match store.create_decision(experiment_id, what, why, project_id) {
        Ok(d) => {
            let dref = d.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", d.id));
            let mut out = format!("Decision {} created (global #{}): {}\n", dref, d.id, d.what);
            let mut auto_edges: Vec<String> = Vec::new();
            let mut suggestions: Vec<(String, String)> = Vec::new();

            // #19: Warn if no causal upstream provided
            if experiment_id.is_none() && finding_ids.is_empty() {
                out += "\n\u{26a0} WARNING: No causal upstream (experiment_id or finding_ids) provided.\n";
                out += "Decisions should be traceable to evidence. Specify:\n";
                out += "  - experiment_id: the experiment that led to this decision\n";
                out += "  - finding_ids: comma-separated finding IDs that informed this decision\n";
            }

            // #19: Auto-create Experiment --Informed--> Decision edge
            if let Some(eid) = experiment_id {
                match store.create_edge(NodeType::Experiment, eid, NodeType::Decision, d.id, EdgeType::Informed) {
                    Ok(_) => auto_edges.push(format!("Experiment#{} --Informed--> Decision#{}", eid, d.id)),
                    Err(e) => out += &format!("(edge note: {})\n", e),
                }
            }

            // #19: Auto-create Finding --Informed--> Decision edges for each finding_id
            for fid in &finding_ids {
                match store.create_edge(NodeType::Finding, *fid, NodeType::Decision, d.id, EdgeType::Informed) {
                    Ok(_) => auto_edges.push(format!("Finding#{} --Informed--> Decision#{}", fid, d.id)),
                    Err(e) => out += &format!("(edge note for Finding#{}:  {})\n", fid, e),
                }
            }

            // Suggest downstream edges
            suggestions.push((
                "If this decision spawns a new experiment".to_string(),
                format!("pm_add_edge source_type=Decision source_id={} target_type=Experiment target_id=? relation=Informed", d.id),
            ));
            suggestions.push((
                "If this decision updates a hypothesis".to_string(),
                format!("pm_add_edge source_type=Decision source_id={} target_type=Hypothesis target_id=? relation=Informed", d.id),
            ));
            suggestions.push((
                "If this decision derives a principle".to_string(),
                format!("pm_principle_add project=<project> scope=project text=\"...\" decision_id={}", d.id),
            ));

            out += &causal_guidance(&auto_edges, &suggestions);
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
                let lref = l.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", l.id));
                let mut out = format!("Literature {} added (global #{}): {}", lref, l.id, l.title);

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
    let exp_pref = store.get_experiment(eid).ok().and_then(|e| e.project_seq).map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", eid));
    let mut out = format!("Experiment {} updated: status={}, result set.\n", exp_pref, status);
    let mut auto_edges: Vec<String> = Vec::new();
    let mut suggestions: Vec<(String, String)> = Vec::new();

    // #19: Auto-create Phase --Contains--> Experiment edge if experiment has phase_id
    if let Ok(exp) = store.get_experiment(eid) {
        if let Some(phase_id) = exp.phase_id {
            match store.create_edge(NodeType::Phase, phase_id, NodeType::Experiment, eid, EdgeType::Contains) {
                Ok(_) => auto_edges.push(format!("Phase#{} --Contains--> Experiment#{}", phase_id, eid)),
                Err(e) => out += &format!("(phase-experiment edge note: {})\n", e),
            }
        }
    }

    if let Some(text) = finding_text {
        match store.create_finding(Some(eid), text) {
            Ok(f) => {
                let fref = f.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", f.id));
                out += &format!("Finding {} created (global #{}).\n", fref, f.id);
                // Auto-create Experiment --ProducedBy--> Finding edge
                match store.create_edge(NodeType::Experiment, eid, NodeType::Finding, f.id, EdgeType::ProducedBy) {
                    Ok(_) => auto_edges.push(format!("Experiment#{} --ProducedBy--> Finding#{}", eid, f.id)),
                    Err(e) => out += &format!("(edge auto-create note: {})\n", e),
                }
                // #19: Auto-create Phase --Contains--> Finding edge
                if let Ok(exp) = store.get_experiment(eid) {
                    if let Some(phase_id) = exp.phase_id {
                        match store.create_edge(NodeType::Phase, phase_id, NodeType::Finding, f.id, EdgeType::Contains) {
                            Ok(_) => auto_edges.push(format!("Phase#{} --Contains--> Finding#{}", phase_id, f.id)),
                            Err(e) => out += &format!("(phase-finding edge note: {})\n", e),
                        }
                    }
                }

                // #19: Suggest downstream edges for the finding
                suggestions.push((
                    "If this finding informs a decision".to_string(),
                    format!("pm_add_edge source_type=Finding source_id={} target_type=Decision target_id=? relation=Informed", f.id),
                ));
                suggestions.push((
                    "If this finding supports/contradicts a hypothesis".to_string(),
                    format!("pm_add_edge source_type=Finding source_id={} target_type=Hypothesis target_id=? relation=Supports", f.id),
                ));
            }
            Err(e) => { out += &format!("Error creating finding: {}\n", e); }
        }
    } else {
        // No finding_text provided — check if experiment has ANY existing findings
        let has_findings = match store.list_findings(Some(eid)) {
            Ok(findings) => !findings.is_empty(),
            Err(_) => false,
        };
        if !has_findings {
            out += "\nWARNING: Completing experiment with no findings logged.\n                    Findings capture what was observed. Use pm_log_finding to record observations.\n";
        }
    }

    // #19: Suggest downstream edges for completed experiment
    suggestions.push((
        "If this experiment tested a hypothesis".to_string(),
        format!("pm_hyp_update hypothesis_id=? status=<confirmed|refuted> experiment_id={}", eid),
    ));
    suggestions.push((
        "If this result warrants a decision".to_string(),
        format!("pm_decision what=\"...\" why=\"...\" experiment_id={}", eid),
    ));

    out += &causal_guidance(&auto_edges, &suggestions);
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

    let mut auto_edges: Vec<String> = Vec::new();
    let mut suggestions: Vec<(String, String)> = Vec::new();

    // #19: Auto-create Experiment --TestedBy--> Hypothesis when transitioning to testing
    if hs == HypothesisStatus::Testing {
        if let Some(eid) = experiment_id {
            match store.create_edge(NodeType::Experiment, eid, NodeType::Hypothesis, hid, EdgeType::TestedBy) {
                Ok(_) => auto_edges.push(format!("Experiment#{} --TestedBy--> Hypothesis#{}", eid, hid)),
                Err(e) => { let _ = e; } // duplicate is fine
            }
        }
    }

    // #19: Auto-create Finding --Supports--> Hypothesis when confirming
    if hs == HypothesisStatus::Confirmed {
        if let Some(fid) = finding_id {
            match store.create_edge(NodeType::Finding, fid, NodeType::Hypothesis, hid, EdgeType::Supports) {
                Ok(_) => auto_edges.push(format!("Finding#{} --Supports--> Hypothesis#{}", fid, hid)),
                Err(e) => { let _ = e; }
            }
        }
    }

    match store.update_hypothesis(hid, hs.clone(), experiment_id, finding_id) {
        Ok(_) => {
            let h_pref = store.get_hypothesis(hid).ok().and_then(|h| h.project_seq).map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", hid));
            let mut out = format!("Hypothesis {} updated to {:?}", h_pref, hs);

            // Auto-suggestion for confirmed: suggest creating a principle
            if hs == HypothesisStatus::Confirmed {
                out += &format!(
                    "\n\nHypothesis confirmed. Consider creating a principle:\n  pm_principle_add project=<project> scope=project text=\"<principle derived from hypothesis #{}>\"\n",
                    hid
                );
                suggestions.push((
                    "If confirmation warrants a decision".to_string(),
                    format!("pm_decision what=\"...\" why=\"Based on confirmed hypothesis #{}\"", hid),
                ));
            }

            // Auto-suggestion for refuted: mention the contradiction edge
            if hs == HypothesisStatus::Refuted {
                if let Some(fid) = finding_id {
                    auto_edges.push(format!("Finding#{} --Contradicts--> Hypothesis#{}", fid, hid));
                }
                suggestions.push((
                    "If refutation warrants a new hypothesis".to_string(),
                    format!("pm_add_edge source_type=Hypothesis source_id={} target_type=Hypothesis target_id=? relation=Supersedes", hid),
                ));
            }

            // Testing: suggest experiment and finding edges
            if hs == HypothesisStatus::Testing {
                suggestions.push((
                    "Log findings from the testing experiment".to_string(),
                    "pm_log_finding experiment_id=? text=\"...\"".to_string(),
                ));
                suggestions.push((
                    "When evidence confirms or refutes".to_string(),
                    format!("pm_hyp_update hypothesis_id={} status=<confirmed|refuted> finding_id=?", hid),
                ));
            }

            out += &causal_guidance(&auto_edges, &suggestions);
            out
        }
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_research_complete(store: &SqliteStore, rid: i64, status_str: &str, report: Option<&str>, phase_id: Option<i64>, finding_ids_str: Option<&str>) -> String {
    let rs = match status_str {
        _ => crate::store::ResearchStatus::Complete,
    };
    match store.update_research(rid, rs.clone(), report) {
        Ok(_) => {
            let r_pref = store.get_research(rid).ok().and_then(|r| r.project_seq).map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", rid));
            let mut out = format!("Research {} updated to {:?}", r_pref, rs);
            let mut auto_edges: Vec<String> = Vec::new();
            let mut suggestions: Vec<(String, String)> = Vec::new();

            // #19: Auto-create Phase --Contains--> Research if phase_id provided
            if let Some(pid) = phase_id {
                match store.create_edge(NodeType::Phase, pid, NodeType::Research, rid, EdgeType::Contains) {
                    Ok(_) => auto_edges.push(format!("Phase#{} --Contains--> Research#{}", pid, rid)),
                    Err(e) => out += &format!(" (phase edge note: {})", e),
                }
            } else {
                // Try to derive phase_id from the research record itself
                if let Ok(research) = store.get_research(rid) {
                    if let Some(pid) = research.phase_id {
                        match store.create_edge(NodeType::Phase, pid, NodeType::Research, rid, EdgeType::Contains) {
                            Ok(_) => auto_edges.push(format!("Phase#{} --Contains--> Research#{}", pid, rid)),
                            Err(e) => { let _ = e; }
                        }
                    }
                }
            }

            // #19: Auto-create Finding --Informed--> Research for each finding_id
            let finding_ids: Vec<i64> = finding_ids_str
                .map(|s| parse_ids(s))
                .unwrap_or_default();
            for fid in &finding_ids {
                match store.create_edge(NodeType::Finding, *fid, NodeType::Research, rid, EdgeType::Informed) {
                    Ok(_) => auto_edges.push(format!("Finding#{} --Informed--> Research#{}", fid, rid)),
                    Err(e) => out += &format!(" (finding edge note: {})", e),
                }
            }

            // Suggest downstream edges
            suggestions.push((
                "If research produced findings".to_string(),
                format!("pm_log_finding experiment_id=? text=\"Research #{} found: ...\"", rid),
            ));
            suggestions.push((
                "If research informs a decision".to_string(),
                format!("pm_add_edge source_type=Research source_id={} target_type=Decision target_id=? relation=Informed", rid),
            ));
            suggestions.push((
                "If research supports/contradicts a hypothesis".to_string(),
                format!("pm_add_edge source_type=Research source_id={} target_type=Hypothesis target_id=? relation=Supports", rid),
            ));

            out += &causal_guidance(&auto_edges, &suggestions);
            out
        }
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
                let prref = pr.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", pr.id));
                let mut out = format!("Principle {} added (global #{}): {}", prref, pr.id, &text[..text.len().min(80)]);

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
                            let cref = c.project_seq.map(|seq| format!("#{}", seq)).unwrap_or_else(|| format!("#{}", c.id));
                            out += &format!("  C{} [{}]: {}\n", cref, sev, t);
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
    let experiment_id = args.get("experiment_id").and_then(|v| v.as_i64());

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
                let cref = con.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", con.id));
                let mut out = format!("Constraint {} added (global #{}): {}", cref, con.id, &text[..text.len().min(80)]);
                let mut auto_edges: Vec<String> = Vec::new();
                let mut suggestions: Vec<(String, String)> = Vec::new();

                // #19: Auto-create Experiment --TestedBy--> Constraint if experiment_id provided
                if let Some(eid) = experiment_id {
                    match store.create_edge(NodeType::Experiment, eid, NodeType::Constraint, con.id, EdgeType::TestedBy) {
                        Ok(_) => auto_edges.push(format!("Experiment#{} --TestedBy--> Constraint#{}", eid, con.id)),
                        Err(e) => out += &format!(" (experiment edge note: {})", e),
                    }
                }

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
                            suggestions.push((
                                format!("Constraint overlaps with Phase#{} ({})", phase.id, phase.name),
                                format!("pm_add_edge source_type=constraint source_id={} target_type=phase target_id={} relation=related", con.id, phase.id),
                            ));
                        }
                    }
                    // Also suggest edges to pending experiments in matching phases
                    for phase in &phases {
                        if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                            let pending: Vec<_> = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).collect();
                            for exp in pending.iter().take(3) {
                                suggestions.push((
                                    format!("Pending Experiment#{} may be affected", exp.id),
                                    format!("pm_add_edge source_type=constraint source_id={} target_type=experiment target_id={} relation=related", con.id, exp.id),
                                ));
                            }
                        }
                    }
                }

                out += &causal_guidance(&auto_edges, &suggestions);
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

        let pref = phase.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", phase_id));
        return format!("Phase {} ({}) updated to {:?}", pref, phase.name, new_status);
    }

    let pref2 = phase.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", phase_id));
    format!("Phase {} ({}) fields updated", pref2, phase.name)
}
