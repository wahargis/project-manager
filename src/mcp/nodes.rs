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
use crate::analysis::confidence;
use crate::validation;
use crate::analysis::contradictions;
use std::collections::HashSet;
use crate::util::truncate_safe;

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
    // CAUSAL BACKBONE: findings SHOULD have a causal upstream (experiment preferred)
    // Warn but allow if no experiment — the finding can be linked to other node types
    if eid <= 0 {
        // Soft warning with guidance, not hard rejection
        let mut help = String::from("\u{26a0} CAUSAL BACKBONE WARNING: No experiment_id provided.\n");
        help += "Findings are stronger when linked to the experiment that produced them.\n";
        help += "The finding will be created but marked as needing causal linkage.\n\n";
        help += "Active experiments (most recent first):\n";
        if let Ok(projects) = store.list_projects() {
            for proj in projects.iter().filter(|p| p.status == crate::store::ProjectStatus::Active) {
                if let Ok(phases) = store.list_phases(proj.id) {
                    for phase in phases.iter().filter(|p| p.status == crate::store::PhaseStatus::InProgress) {
                        if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                            for exp in exps.iter().rev().take(5) {
                                let name_short: String = exp.name.chars().take(60).collect();
                                help += &format!("  Exp #{}: {} [{:?}]\n", exp.id, name_short, exp.status);
                            }
                        }
                    }
                }
            }
        }
        help += "\nRecommended:\n";
        help += "  pm_log_finding experiment_id=<id> text=\"...\"\n";
        help += "  Or link after creation: pm_add_edge source_type=<type> source_id=<id> target_type=finding target_id=<new_id> relation=informed\n";
        // Continue with creation but include warning in output
    }
    // Session experiment fallback: if no explicit experiment_id, check active session
    let exp_id = if eid > 0 {
        Some(eid)
    } else if let Ok(Some(session)) = store.get_current_session() {
        session.active_experiment_id
    } else {
        None
    };
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

            // AUTO-CONTRADICTION CHECK: search for findings that may conflict
            {
                let search_text: String = text.chars().take(100).collect();
                if let Ok(results) = store.text_search(&search_text) {
                    let contradictions: Vec<_> = results.iter()
                        .filter(|r| r.node_type == "finding" && r.node_id != f.id as i64)
                        .take(3)
                        .collect();
                    if !contradictions.is_empty() {
                        out += "\n  RELATED FINDINGS (check for contradictions):\n";
                        for c in &contradictions {
                            let ctext: String = c.text_excerpt.chars().take(80).collect();
                            out += &format!("    F#{}: {}\n", c.node_id, ctext);
                            suggestions.push((
                                format!("If F#{} contradicts this finding", c.node_id),
                                format!("pm_add_edge source_type=Finding source_id={} target_type=Finding target_id={} relation=Contradicts", f.id, c.node_id),
                            ));
                        }
                    }
                }
            }

            // Show siblings for edge suggestions
            if let Some(eid) = exp_id {
                if let Ok(siblings) = store.list_findings(Some(eid)) {
                    let others: Vec<_> = siblings.iter().filter(|s| s.id != f.id).collect();
                    if !others.is_empty() {
                        out += "\nSibling findings (same experiment):\n";
                        for s in others.iter().take(5) {
                            let t = truncate_safe(&s.text, 60);
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
                                        let t = truncate_safe(&l.title, 60);
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
                                        let t = truncate_safe(&p.text, 80);
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

            // Layer 1 contradiction detection: scan existing findings for contradictions
            if let Some(eid_val) = exp_id {
                if let Ok(exp) = store.get_experiment(eid_val) {
                    if let Some(phase_id) = exp.phase_id {
                        if let Ok(phase) = store.get_phase(phase_id) {
                            let project_id = phase.project_id;
                            let mut candidates: Vec<contradictions::ContradictionCandidate> = Vec::new();

                            if let Ok(phases) = store.list_phases(project_id) {
                                for p in &phases {
                                    if let Ok(exps) = store.list_experiments(Some(p.id)) {
                                        for ex in &exps {
                                            if let Ok(findings) = store.list_findings(Some(ex.id)) {
                                                for existing in &findings {
                                                    if existing.id == f.id {
                                                        continue;
                                                    }
                                                    let (score, signals) = contradictions::score_pair(text, &existing.text);
                                                    if score > 0.3 {
                                                        candidates.push(contradictions::ContradictionCandidate {
                                                            node_type: "Finding".to_string(),
                                                            node_id: existing.id,
                                                            text_excerpt: existing.text.clone(),
                                                            signal_score: score,
                                                            signals,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Sort by score descending, keep top 5
                            candidates.sort_by(|a, b| b.signal_score.partial_cmp(&a.signal_score).unwrap_or(std::cmp::Ordering::Equal));
                            candidates.truncate(5);

                            if !candidates.is_empty() {
                                let nli_prompt = contradictions::generate_nli_prompt(text, &candidates);
                                let layer1 = contradictions::Layer1Result {
                                    candidates,
                                    subagent_prompt: Some(nli_prompt),
                                };
                                out += &contradictions::format_layer1_results(text, &layer1);
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

/// Anti-cleanup guardrail (#34): scan text for closure/pruning language.
/// Returns the matched text if cleanup language is detected, None otherwise.
pub fn cleanup_guard_check(text: &str) -> Option<String> {
    let lower = text.to_lowercase();

    // Closure patterns: "close/deprecate/remove" near research terms
    let word_patterns: &[(&[&str], &str)] = &[
        (&["deprecate", "deprecating", "deprecated"], "deprecation"),
        (&["remove", "removing"], "removal"),
        (&["prune", "pruning"], "pruning"),
        (&["cleanup", "clean up", "cleaning up"], "cleanup"),
        (&["stale"], "staleness label"),
        (&["abandoned"], "abandonment label"),
        (&["obsolete"], "obsolescence label"),
        (&["dead end", "dead-end"], "dead end label"),
    ];

    // Check "close/closing" + research terms
    if (lower.contains("close") || lower.contains("closing")) &&
       (lower.contains("phase") || lower.contains("experiment") ||
        lower.contains("branch") || lower.contains("hypothesis")) {
        // Find the matched close word for display
        let close_word = if lower.contains("closing") { "closing" } else { "close" };
        return Some(format!("closure of research element (\"{}\")", close_word));
    }

    // Check impact zeroing
    if lower.contains("impact to 0") || lower.contains("impact=0") {
        return Some("zeroing impact (\"impact to 0\")".to_string());
    }
    if lower.contains("reduce impact") {
        return Some("reducing impact (\"reduce impact\")".to_string());
    }

    // Check simple word patterns
    for (words, label) in word_patterns {
        for word in *words {
            if lower.contains(word) {
                return Some(format!("{} (\"{}\")", label, word));
            }
        }
    }
    None
}

pub fn tool_decision(store: &SqliteStore, what: &str, why: Option<&str>, experiment_id: Option<i64>, finding_ids_str: Option<&str>, project_name: Option<&str>) -> String {
    let v = validation::validate_decision(what, why);
    if !v.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", v.to_mcp_error());
    }
    // #34: Anti-cleanup guardrail -- warn on closure/pruning language
    let cleanup_warning = cleanup_guard_check(what);

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

    // CAUSAL BACKBONE ENFORCEMENT: decisions must have causal upstream
    if finding_ids.is_empty() && experiment_id.is_none() {
        return format!("\u{274c} CAUSAL BACKBONE ERROR: Decision requires causal upstream.\nEvery decision must be informed by at least one finding or experiment.\n\nProvide one of:\n  finding_ids=\"1,2,3\"    — findings that informed this decision\n  experiment_id=<id>     — experiment that led to this decision\n");
    }

    match store.create_decision(experiment_id, what, why, project_id) {
        Ok(d) => {
            let dref = d.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", d.id));
            let mut out = format!("Decision {} created (global #{}): {}\n", dref, d.id, d.what);

            // #34: Emit cleanup guard warning if triggered
            if let Some(ref matched) = cleanup_warning {
                out += &format!(
                    "\n\u{26a0}\u{fe0f} CLEANUP GUARD: This decision contains closure/pruning language ({}).\n\
                    Research phases and experiments with negative results are valuable \u{2014} they narrow the search space.\n\
                    If this is an explicit user request to close/deprioritize, proceed. Otherwise, consider reframing\n\
                    as a redirect (what NEW direction does this suggest?) rather than a closure.\n\n",
                    matched
                );
            }
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

            // Merge/convergence detection: if findings come from different experiments, this is a convergence point
            if finding_ids.len() > 1 {
                let mut source_experiments: std::collections::HashSet<i64> = std::collections::HashSet::new();
                for fid in &finding_ids {
                    if let Ok(finding) = store.get_finding(*fid) {
                        if let Some(eid) = finding.experiment_id {
                            source_experiments.insert(eid);
                        }
                    }
                }
                if source_experiments.len() > 1 {
                    out += &format!("\n** MERGE POINT: This decision converges findings from {} different experiments.\n", source_experiments.len());
                    // Auto-create ConvergesInto edges from each source experiment to this decision
                    for src_eid in &source_experiments {
                        match store.create_edge(NodeType::Experiment, *src_eid, NodeType::Decision, d.id, EdgeType::ConvergesInto) {
                            Ok(_) => auto_edges.push(format!("Exp#{} --ConvergesInto--> Decision#{}", src_eid, d.id)),
                            Err(e) => out += &format!("(convergence edge note for Exp#{}: {})\n", src_eid, e),
                        }
                    }
                    let exp_list: Vec<String> = source_experiments.iter().map(|e| format!("Exp#{}", e)).collect();
                    out += &format!("  Converging experiments: {}\n", exp_list.join(", "));
                }
            }

            // DECISION SUPPORT: surface ALL related nodes (any type) that may inform this decision
            {
                let search_text: String = what.chars().take(100).collect();
                if let Ok(results) = store.text_search(&search_text) {
                    let related: Vec<_> = results.iter()
                        .filter(|r| !(r.node_type == "decision" && r.node_id == d.id as i64))
                        .take(5)
                        .collect();
                    if !related.is_empty() {
                        out += "\n  RELATED KG NODES (review for consistency):\n";
                        for r in &related {
                            let rtext: String = r.text_excerpt.chars().take(80).collect();
                            out += &format!("    {}#{}: {}\n", r.node_type, r.node_id, rtext);
                        }
                    }
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

    // Literature status lifecycle enforcement:
    // unread -> read -> cited -> tested -> dead_end/promising/integrated
    let current = match store.get_literature(literature_id) {
        Ok(lit) => lit.status.unwrap_or_else(|| "unread".to_string()),
        Err(e) => return format!("Error fetching literature #{}: {}", literature_id, e),
    };

    let valid_transitions = match current.as_str() {
        "unread" => vec!["read", "dead_end"],
        "read" => vec!["cited", "tested", "dead_end", "promising"],
        "cited" => vec!["tested", "dead_end", "promising", "integrated"],
        "tested" => vec!["dead_end", "promising", "integrated"],
        "promising" => vec!["tested", "cited", "integrated"],
        "dead_end" | "integrated" => vec![], // terminal states
        _ => vec!["unread", "read", "cited", "tested", "dead_end", "promising", "integrated"],
    };

    if valid_transitions.is_empty() {
        return format!(
            "\u{274c} Literature #{} is in terminal state '{}'. Cannot transition further.",
            literature_id, current
        );
    }

    if !valid_transitions.contains(&status) {
        return format!(
            "\u{274c} Invalid literature transition: '{}' -> '{}'.\nValid transitions from '{}': {}\n\nLifecycle: unread -> read -> cited -> tested -> dead_end/promising/integrated",
            current, status, current, valid_transitions.join(", ")
        );
    }

    match store.update_literature_status(literature_id, status) {
        Ok(_) => {
            let mut out = format!("Literature #{} status updated: '{}' -> '{}'", literature_id, current, status);

            // Edge suggestions based on new status
            match status {
                "cited" => {
                    out += "\n\nSuggested edges:";
                    out += &format!("\n  pm_add_edge source_type=literature source_id={} target_type=finding target_id=? relation=cited", literature_id);
                    out += &format!("\n  pm_add_edge source_type=literature source_id={} target_type=experiment target_id=? relation=informed", literature_id);
                }
                "tested" => {
                    out += "\n\nSuggested edges:";
                    out += &format!("\n  pm_add_edge source_type=experiment source_id=? target_type=literature target_id={} relation=tested_by", literature_id);
                }
                "integrated" => {
                    out += "\n\nSuggested edges:";
                    out += &format!("\n  pm_add_edge source_type=literature source_id={} target_type=finding target_id=? relation=informed", literature_id);
                    out += &format!("\n  pm_add_edge source_type=literature source_id={} target_type=decision target_id=? relation=informed", literature_id);
                }
                _ => {}
            }

            out
        }
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

    // Statistical confidence scoring (MAD-based)
    let all_findings = store.list_findings(Some(eid)).unwrap_or_default();
    if let Some(conf) = confidence::compute_experiment_confidence(&all_findings) {
        out += "
";
        out += &conf.display();
    }

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
            // Use TMS-aware edge creation to auto-suspend dependent nodes
            match store.create_edge_with_tms(NodeType::Finding, fid, NodeType::Hypothesis, hid, EdgeType::Contradicts) {
                Ok(result) => {
                    if !result.suspended_nodes.is_empty() {
                        // Log suspended dependents for the caller to see
                        let _ = result; // Will be reported in the output below
                    }
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

    // Capture TMS suspension info from refutation edge (if any)
    let tms_suspended: Vec<(String, i64)> = Vec::new(); // populated by create_edge_with_tms above

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

            // Auto-suggestion for refuted: mention the contradiction edge and any TMS suspensions
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

    // Provenance enforcement: principles MUST trace to evidence
    if finding_id.is_none() && decision_id.is_none() {
        return format!(
            "\u{274c} PROVENANCE ERROR: Principles must be derived from evidence.\n             Provide at least one of:\n               finding_id=<id>   -- finding that established this principle\n               decision_id=<id>  -- decision that established this principle\n             This ensures every principle traces back through the causal backbone."
        );
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
                let mut out = format!("Principle {} added (global #{}): {}", prref, pr.id, truncate_safe(&text, 80));

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

                // Suggest ViolatedBy edges from recent contradicting findings
                if let Ok(phases) = store.list_phases(proj.id) {
                    let mut recent_findings = Vec::new();
                    for phase in &phases {
                        if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                            for exp in &exps {
                                if let Ok(findings) = store.list_findings(Some(exp.id)) {
                                    recent_findings.extend(findings);
                                }
                            }
                        }
                    }
                    if !recent_findings.is_empty() {
                        out += "\n\nIf any finding contradicts this principle, create a ViolatedBy edge:";
                        for f in recent_findings.iter().rev().take(3) {
                            let t = truncate_safe(&f.text, 60);
                            let fref = f.project_seq.map(|seq| format!("#{}", seq)).unwrap_or_else(|| format!("#{}", f.id));
                            out += &format!("\n  F{}: {} -> pm_add_edge source_type=finding source_id={} target_type=principle target_id={} relation=violated_by", fref, t, f.id, pr.id);
                        }
                    }
                }

                // Suggest edges to related constraints (#15)
                if let Ok(constraints) = store.list_constraints(proj.id) {
                    if !constraints.is_empty() {
                        out += "\n\nActive constraints (suggest related edges):\n";
                        for c in constraints.iter().take(5) {
                            let t = truncate_safe(&c.text, 60);
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
                let mut out = format!("Constraint {} added (global #{}): {}", cref, con.id, truncate_safe(&text, 80));
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

/// DEPRECATED: use pm_experiment_create + pm_log_finding instead.
/// Auto-routing broke the causal backbone by dumping findings into wrong experiments.
/// This tool now redirects to the proper workflow.
/// Accepts project name + finding text, finds the highest-impact in-progress phase,
/// picks its most recent pending/active experiment, and logs the finding there.
pub fn tool_research_step(store: &SqliteStore, project_name: &str, text: &str) -> String {
    // DEPRECATED: auto-routing breaks causal backbone
    let mut help = String::from("\u{26a0} pm_research_step is deprecated — it breaks the causal backbone.\n\n");
    help += "Use this workflow instead:\n";
    help += "  1. pm_experiment_create phase_id=<id> name=\"<what you are investigating>\" informed_by_finding=<id>\n";
    help += "  2. pm_log_finding experiment_id=<new_exp_id> text=\"<your finding>\"\n\n";
    help += "This ensures every finding traces back through a causal chain.\n";
    return help;

    // Original auto-routing code below (dead code, kept for reference)
    let v = crate::validation::validate_finding(text);
    if !v.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", v.to_mcp_error());
    }

    // Find project
    let projects = match store.list_projects() {
        Ok(p) => p,
        Err(e) => return format!("Error listing projects: {}", e),
    };
    let project = projects.iter().find(|p| {
        p.name == project_name || p.alias.as_deref() == Some(project_name)
    });
    let project = match project {
        Some(p) => p,
        None => return format!("Project {} not found. Available: {}", project_name,
            projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")),
    };

    // Find highest-impact in-progress phase
    let phases = match store.list_phases(project.id) {
        Ok(p) => p,
        Err(e) => return format!("Error listing phases: {}", e),
    };
    let active_phase = phases.iter()
        .filter(|p| p.status == crate::store::PhaseStatus::InProgress)
        .max_by_key(|p| p.impact);
    let active_phase = match active_phase {
        Some(p) => p,
        None => {
            // Fall back to any phase with pending experiments
            match phases.iter().filter(|p| p.status != crate::store::PhaseStatus::Complete && p.status != crate::store::PhaseStatus::Deprioritized).max_by_key(|p| p.impact) {
                Some(p) => p,
                None => return format!("No active phases in project {}", project_name),
            }
        }
    };

    // Find experiments in that phase, prefer pending ones
    let experiments = match store.list_experiments(Some(active_phase.id)) {
        Ok(e) => e,
        Err(e) => return format!("Error listing experiments: {}", e),
    };
    let target_exp = experiments.iter()
        .filter(|e| e.status == crate::store::ExperimentStatus::Pending)
        .last()  // most recently created
        .or_else(|| experiments.last());  // fallback to any

    let eid = match target_exp {
        Some(e) => e.id,
        None => return format!("No experiments in phase {} (#{}) to route finding to", active_phase.name, active_phase.id),
    };

    // Delegate to existing tool_log_finding
    let mut result = format!("Auto-routed to Phase #{} {} → Exp #{}\n\n",
        active_phase.id, 
        truncate_safe(&active_phase.name, 40),
        eid);
    result += &tool_log_finding(store, eid, text);
    result
}

/// Create a new experiment with REQUIRED causal upstream linkage.
/// Every experiment must be causally linked to what motivated it.
pub fn tool_experiment_create(store: &SqliteStore, phase_id: i64, name: &str,
    informed_by_finding: Option<i64>,
    informed_by_decision: Option<i64>,
    informed_by_experiment: Option<i64>,
) -> String {
    // Validate name
    if name.len() < 10 {
        return "\u{274c} Experiment name too short (min 10 chars). Describe what investigation this experiment represents.".to_string();
    }

    // CAUSAL BACKBONE ENFORCEMENT: must have at least one upstream link
    let has_upstream = informed_by_finding.is_some()
        || informed_by_decision.is_some()
        || informed_by_experiment.is_some();

    // Check if this is the first experiment in the phase (root is OK)
    let is_first = match store.list_experiments(Some(phase_id)) {
        Ok(exps) => exps.is_empty(),
        Err(_) => false,
    };

    if !has_upstream && !is_first {
        let mut help = String::from("\u{274c} CAUSAL BACKBONE ERROR: Experiment requires causal upstream.\n");
        help += "Every experiment (except the first in a phase) must link to what motivated it.\n\n";
        help += "Provide one of:\n";
        help += "  informed_by_finding=<id>     — this experiment investigates finding #N\n";
        help += "  informed_by_decision=<id>    — this experiment was directed by decision #N\n";
        help += "  informed_by_experiment=<id>  — this experiment continues/branches from experiment #N\n";
        return help;
    }

    match store.create_experiment(Some(phase_id), name) {
        Ok(exp) => {
            let mut out = format!("Experiment #{} created: {}\n", exp.id, name);
            let mut edges: Vec<String> = Vec::new();

            // Auto-create Phase --Contains--> Experiment edge
            match store.create_edge(NodeType::Phase, phase_id, NodeType::Experiment, exp.id, EdgeType::Contains) {
                Ok(_) => edges.push(format!("Phase#{} --Contains--> Exp#{}", phase_id, exp.id)),
                Err(_) => {},
            }

            // Create causal upstream edges
            if let Some(fid) = informed_by_finding {
                match store.create_edge(NodeType::Finding, fid, NodeType::Experiment, exp.id, EdgeType::Informed) {
                    Ok(_) => edges.push(format!("Finding#{} --Informed--> Exp#{}", fid, exp.id)),
                    Err(e) => out += &format!("  (edge error: {})\n", e),
                }
            }
            if let Some(did) = informed_by_decision {
                match store.create_edge(NodeType::Decision, did, NodeType::Experiment, exp.id, EdgeType::Informed) {
                    Ok(_) => edges.push(format!("Decision#{} --Informed--> Exp#{}", did, exp.id)),
                    Err(e) => out += &format!("  (edge error: {})\n", e),
                }
            }
            if let Some(eid) = informed_by_experiment {
                // Branch detection: check if source experiment already has other downstream experiments
                let existing_downstream = store.get_edges_from(NodeType::Experiment, eid)
                    .unwrap_or_default()
                    .iter()
                    .filter(|e| e.target_type == NodeType::Experiment && matches!(e.relation, EdgeType::Informed | EdgeType::BranchesFrom))
                    .count();

                if existing_downstream > 0 {
                    // This creates a branch point -- use BranchesFrom edge type
                    match store.create_edge(NodeType::Experiment, eid, NodeType::Experiment, exp.id, EdgeType::BranchesFrom) {
                        Ok(_) => edges.push(format!("Exp#{} --BranchesFrom--> Exp#{}", eid, exp.id)),
                        Err(e) => out += &format!("  (edge error: {})\n", e),
                    }
                    out += &format!("\n** BRANCH POINT: Exp#{} now has {} downstream experiments (including this one). This is a research branch point.\n", eid, existing_downstream + 1);

                    // Upgrade existing Informed edges from the source to BranchesFrom
                    let existing_informed: Vec<(i64, i64)> = store.get_edges_from(NodeType::Experiment, eid)
                        .unwrap_or_default()
                        .iter()
                        .filter(|e| e.target_type == NodeType::Experiment && e.relation == EdgeType::Informed)
                        .map(|e| (e.id, e.target_id))
                        .collect();
                    for (edge_id, target_id) in &existing_informed {
                        let _ = store.delete_edge(*edge_id);
                        let _ = store.create_edge(NodeType::Experiment, eid, NodeType::Experiment, *target_id, EdgeType::BranchesFrom);
                    }
                    if !existing_informed.is_empty() {
                        out += &format!("  (upgraded {} existing Informed edges to BranchesFrom)\n", existing_informed.len());
                    }
                } else {
                    match store.create_edge(NodeType::Experiment, eid, NodeType::Experiment, exp.id, EdgeType::Informed) {
                        Ok(_) => edges.push(format!("Exp#{} --Informed--> Exp#{}", eid, exp.id)),
                        Err(e) => out += &format!("  (edge error: {})\n", e),
                    }
                }
            }

            if !edges.is_empty() {
                out += "\nCausal links:\n";
                for e in &edges {
                    out += &format!("  + {}\n", e);
                }
            }

            // Auto-surface relevant constraints for this phase's project
            if let Ok(phase) = store.get_phase(phase_id) {
                if let Ok(constraints) = store.list_constraints(phase.project_id) {
                    if !constraints.is_empty() {
                        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                        let mut active_constraints = Vec::new();
                        let mut expired_constraints = Vec::new();
                        for c in &constraints {
                            if let Some(ref expires) = c.expires_at {
                                if !expires.is_empty() && expires.as_str() <= today.as_str() {
                                    expired_constraints.push(c);
                                    continue;
                                }
                            }
                            active_constraints.push(c);
                        }
                        if !active_constraints.is_empty() {
                            out += "\nActive constraints to consider:\n";
                            for c in active_constraints.iter().take(5) {
                                let t = truncate_safe(&c.text, 60);
                                let sev = c.severity.as_deref().unwrap_or("hard");
                                let cref = c.project_seq.map(|seq| format!("#{}", seq)).unwrap_or_else(|| format!("#{}", c.id));
                                out += &format!("  C{} [{}]: {}\n", cref, sev, t);
                            }
                        }
                        if !expired_constraints.is_empty() {
                            out += "\nWARNING: Expired constraints (may need re-validation):\n";
                            for c in expired_constraints.iter().take(3) {
                                let t = truncate_safe(&c.text, 60);
                                let cref = c.project_seq.map(|seq| format!("#{}", seq)).unwrap_or_else(|| format!("#{}", c.id));
                                out += &format!("  C{}: {} (expired {})\n", cref, t, c.expires_at.as_deref().unwrap_or("?"));
                            }
                        }
                    }
                }
            }

            out
        }
        Err(e) => format!("Error creating experiment: {}", e),
    }
}
