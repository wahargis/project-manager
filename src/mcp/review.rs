//! MCP tool implementations for review/stats operations.
//!
//! Contains: pm_review, pm_stats

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, NodeType, EdgeType, HypothesisStatus};
use crate::dag::DagEngine;

pub fn tool_review(store: &SqliteStore, project: &str) -> String {
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
            let pref = p.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", p.id));
            text += &format!("  {} [impact:{}] {:?} {}\n", pref, p.impact, p.status, p.name);
        }
    }
    // Collect project-scoped findings through phase->experiment chain
    let mut project_findings = Vec::new();
    if let Ok(phases) = store.list_phases(proj.id) {
        for phase in &phases {
            if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                for exp in &exps {
                    if let Ok(fs) = store.list_findings(Some(exp.id)) {
                        project_findings.extend(fs);
                    }
                }
            }
        }
    }
    let contradictions = kg.find_contradictions(&project_findings).unwrap_or_default();
    if !contradictions.is_empty() {
        text += &format!("\n## Contradictions: {}\n", contradictions.len());
        for (f1, f2) in contradictions.iter().take(5) {
            let t1: String = f1.text.chars().take(60).collect();
            let t2: String = f2.text.chars().take(60).collect();
            text += &format!("  F#{} vs F#{}: \"{}\" <-> \"{}\"\n", f1.id, f2.id, t1, t2);
        }
        if contradictions.len() > 5 {
            text += &format!("  ... and {} more\n", contradictions.len() - 5);
        }
    }
    // Enhanced checks
    let lit_count = store.list_literature(proj.id).map(|l| l.len()).unwrap_or(0);
    text += &format!("\nLiterature: {} entries. Check for new papers.\n", lit_count);
    // Collect hypotheses scoped to this project's phases
    let mut project_hyps = Vec::new();
    if let Ok(phases) = store.list_phases(proj.id) {
        for phase in &phases {
            if let Ok(hs) = store.list_hypotheses(Some(phase.id)) {
                project_hyps.extend(hs);
            }
        }
    }
    {
        let hyps = &project_hyps;
        let proposed: Vec<_> = hyps.iter().filter(|h| h.status == HypothesisStatus::Proposed).collect();
        if !proposed.is_empty() {
            text += &format!("\nHypotheses: {} untested\n", proposed.len());
            for h in proposed.iter().take(3) {
                let t = if h.text.len() > 60 { &h.text[..60] } else { &h.text };
                let href = h.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", h.id));
                text += &format!("  H{}: {}\n", href, t);
            }
        }
    }

    // Hypothesis orphan detection (#9): count hypotheses with zero edges
    let mut orphan_count = 0;
    for h in &project_hyps {
        let from_edges = store.get_edges_from(NodeType::Hypothesis, h.id).unwrap_or_default();
        let to_edges = store.get_edges_to(NodeType::Hypothesis, h.id).unwrap_or_default();
        if from_edges.is_empty() && to_edges.is_empty() {
            orphan_count += 1;
        }
    }
    if orphan_count > 0 {
        text += &format!("\n## WARNING: {} orphaned hypothesis/hypotheses (no edges). Link them with pm_add_edge.\n", orphan_count);
    }

    // === Orphan detection across ALL node types (#14) ===
    let node_types = ["finding", "decision", "hypothesis", "literature", "principle", "constraint"];
    let mut orphan_sections = Vec::new();
    let mut total_orphans = 0;
    for nt in &node_types {
        if let Ok(orphaned_ids) = store.get_orphaned_nodes(nt, proj.id) {
            if !orphaned_ids.is_empty() {
                total_orphans += orphaned_ids.len();
                let prefix = match *nt {
                    "finding" => "F",
                    "decision" => "D",
                    "hypothesis" => "H",
                    "literature" => "L",
                    "principle" => "P",
                    "constraint" => "C",
                    _ => "?",
                };
                let ids_str: Vec<String> = orphaned_ids.iter().map(|id| format!("{}#{}", prefix, id)).collect();
                // Note: orphan IDs here are global IDs for identification
                let cap = capitalize(nt);
                orphan_sections.push(format!("  {}: {} orphaned ({})", cap, orphaned_ids.len(), ids_str.join(", ")));
            }
        }
    }
    if total_orphans > 0 {
        text += &format!("\n## Orphaned nodes: {}\n", total_orphans);
        for section in &orphan_sections {
            text += &format!("{}\n", section);
        }
    }

    // === Branch/Merge Analysis ===
    {
        let mut branch_points: Vec<(i64, Option<i64>, String, Vec<i64>)> = Vec::new(); // (exp_id, project_seq, name, downstream_ids)
        let mut merge_points: Vec<(i64, Option<i64>, String, Vec<i64>)> = Vec::new(); // (dec_id, project_seq, what, source_exp_ids)
        let mut dangling_branches: Vec<(i64, Option<i64>, String, usize, usize)> = Vec::new(); // (exp_id, seq, name, total, pending)

        // Find branch points: experiments with >1 downstream experiment
        if let Ok(phases) = store.list_phases(proj.id) {
            let mut all_exps = Vec::new();
            for phase in &phases {
                if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                    all_exps.extend(exps);
                }
            }
            for exp in &all_exps {
                let outgoing = store.get_edges_from(NodeType::Experiment, exp.id).unwrap_or_default();
                let downstream_exp_ids: Vec<i64> = outgoing.iter()
                    .filter(|e| e.target_type == NodeType::Experiment
                        && matches!(e.relation, EdgeType::Informed | EdgeType::BranchesFrom))
                    .map(|e| e.target_id)
                    .collect();
                if downstream_exp_ids.len() > 1 {
                    branch_points.push((exp.id, exp.project_seq, exp.name.clone(), downstream_exp_ids.clone()));

                    // Check for dangling branches (some downstream experiments still pending)
                    let mut pending_count = 0;
                    for did in &downstream_exp_ids {
                        if let Ok(downstream_exp) = store.get_experiment(*did) {
                            if downstream_exp.status == crate::store::ExperimentStatus::Pending {
                                pending_count += 1;
                            }
                        }
                    }
                    if pending_count > 0 {
                        dangling_branches.push((exp.id, exp.project_seq, exp.name.clone(), downstream_exp_ids.len(), pending_count));
                    }
                }
            }
        }

        // Find merge points: decisions with findings from multiple experiments
        if let Ok(decisions) = store.list_decisions(proj.id) {
            for d in &decisions {
                let incoming = store.get_edges_to(NodeType::Decision, d.id).unwrap_or_default();
                let mut source_exp_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();

                // Check direct experiment edges
                if let Some(eid) = d.experiment_id {
                    source_exp_ids.insert(eid);
                }

                // Check finding-based edges
                for edge in &incoming {
                    if edge.source_type == NodeType::Finding {
                        if let Ok(f) = store.get_finding(edge.source_id) {
                            if let Some(eid) = f.experiment_id {
                                source_exp_ids.insert(eid);
                            }
                        }
                    }
                    // Also count direct experiment edges (Informed, ConvergesInto)
                    if edge.source_type == NodeType::Experiment {
                        source_exp_ids.insert(edge.source_id);
                    }
                }

                if source_exp_ids.len() > 1 {
                    let exp_ids: Vec<i64> = source_exp_ids.into_iter().collect();
                    merge_points.push((d.id, d.project_seq, d.what.clone(), exp_ids));
                }
            }
        }

        if !branch_points.is_empty() {
            text += &format!("\n## Branch Points: {} experiments with fan-out\n", branch_points.len());
            for (eid, seq, name, downstream) in &branch_points {
                let eref = seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E(id:{})", eid));
                let name_trunc: String = name.chars().take(50).collect();
                let downstream_refs: Vec<String> = downstream.iter().map(|d| format!("E#{}", d)).collect();
                text += &format!("  {} \"{}\" -> {} downstream: {}\n", eref, name_trunc, downstream.len(), downstream_refs.join(", "));
            }
        }

        if !merge_points.is_empty() {
            text += &format!("\n## Merge Points: {} decisions converging multiple experiments\n", merge_points.len());
            for (did, seq, what, exps) in &merge_points {
                let dref = seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D(id:{})", did));
                let what_trunc: String = what.chars().take(50).collect();
                let exp_refs: Vec<String> = exps.iter().map(|e| format!("E#{}", e)).collect();
                text += &format!("  {} \"{}\" <- converges: {}\n", dref, what_trunc, exp_refs.join(", "));
            }
        }

        if !dangling_branches.is_empty() {
            text += &format!("\n## Dangling Branches: {} branch points with pending experiments\n", dangling_branches.len());
            for (eid, seq, name, total, pending) in &dangling_branches {
                let eref = seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E(id:{})", eid));
                let name_trunc: String = name.chars().take(50).collect();
                text += &format!("  {} \"{}\": {}/{} downstream experiments still pending\n", eref, name_trunc, pending, total);
            }
        }
    }

    // === Constraint expiry checking (#13) ===
    if let Ok(constraints) = store.list_constraints(proj.id) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut expired = Vec::new();
        for c in &constraints {
            if let Some(ref expires) = c.expires_at {
                if !expires.is_empty() && expires.as_str() <= today.as_str() {
                    let t = if c.text.len() > 60 { &c.text[..60] } else { &c.text };
                    let cref = c.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", c.id));
                    expired.push(format!("  C{}: {} (expired {})", cref, t, expires));
                }
            }
        }
        if !expired.is_empty() {
            text += &format!("\n## Expired constraints: {}\n", expired.len());
            for e in &expired {
                text += &format!("{}\n", e);
            }
        }
    }


    // === Staleness Report (Feature 5) ===
    if let Ok(report) = store.staleness_report(proj.id) {
        if !report.stale_hypotheses.is_empty() {
            text += &format!("\n## Stale Hypotheses (proposed >7 days, untested): {}\n", report.stale_hypotheses.len());
            for (h, days) in report.stale_hypotheses.iter().take(5) {
                let t = if h.text.len() > 60 { &h.text[..60] } else { &h.text };
                let href = h.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", h.id));
                text += &format!("  H{}: {} ({} days stale)\n", href, t, days);
            }
        }
        if !report.stale_experiments.is_empty() {
            text += &format!("\n## Stale Experiments (pending >14 days): {}\n", report.stale_experiments.len());
            for (e, days) in report.stale_experiments.iter().take(5) {
                let eref = e.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", e.id));
                text += &format!("  E{}: {} ({} days stale)\n", eref, e.name, days);
            }
        }
        if !report.unconnected_findings.is_empty() {
            text += &format!("\n## Unconnected Findings (>30 days, no edges): {}\n", report.unconnected_findings.len());
            for f in report.unconnected_findings.iter().take(5) {
                let t = if f.text.len() > 60 { &f.text[..60] } else { &f.text };
                text += &format!("  F#{}: {}\n", f.id, t);
            }
        }
    }



    // === TMS: Suspended/Retracted Nodes ===
    {
        let mut suspended_items = Vec::new();
        let mut retracted_items = Vec::new();

        // Check findings
        if let Ok(phases) = store.list_phases(proj.id) {
            for phase in &phases {
                if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                    for exp in &exps {
                        if let Ok(findings) = store.list_findings(Some(exp.id)) {
                            for f in &findings {
                                match f.belief_status.as_deref() {
                                    Some("suspended") => {
                                        let t = if f.text.len() > 60 { &f.text[..60] } else { &f.text };
                                        suspended_items.push(format!("  F#{}: {} (conf={:.2})", f.id, t, f.confidence.unwrap_or(0.0)));
                                    }
                                    Some("retracted") => {
                                        let t = if f.text.len() > 60 { &f.text[..60] } else { &f.text };
                                        retracted_items.push(format!("  F#{}: {} (conf={:.2})", f.id, t, f.confidence.unwrap_or(0.0)));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check hypotheses
        for h in &project_hyps {
            match h.belief_status.as_deref() {
                Some("suspended") => {
                    let t = if h.text.len() > 60 { &h.text[..60] } else { &h.text };
                    let href = h.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", h.id));
                    suspended_items.push(format!("  H{}: {} (conf={:.2})", href, t, h.confidence.unwrap_or(0.0)));
                }
                Some("retracted") => {
                    let t = if h.text.len() > 60 { &h.text[..60] } else { &h.text };
                    let href = h.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", h.id));
                    retracted_items.push(format!("  H{}: {} (conf={:.2})", href, t, h.confidence.unwrap_or(0.0)));
                }
                _ => {}
            }
        }

        // Check decisions
        if let Ok(decisions) = store.list_decisions(proj.id) {
            for d in &decisions {
                match d.belief_status.as_deref() {
                    Some("suspended") => {
                        let t = if d.what.len() > 60 { &d.what[..60] } else { &d.what };
                        let dref = d.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", d.id));
                        suspended_items.push(format!("  D{}: {} (conf={:.2})", dref, t, d.confidence.unwrap_or(0.0)));
                    }
                    Some("retracted") => {
                        let t = if d.what.len() > 60 { &d.what[..60] } else { &d.what };
                        let dref = d.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", d.id));
                        retracted_items.push(format!("  D{}: {} (conf={:.2})", dref, t, d.confidence.unwrap_or(0.0)));
                    }
                    _ => {}
                }
            }
        }

        // Check principles
        if let Ok(principles) = store.list_principles(proj.id) {
            for p in &principles {
                match p.belief_status.as_deref() {
                    Some("suspended") => {
                        let t = if p.text.len() > 60 { &p.text[..60] } else { &p.text };
                        suspended_items.push(format!("  P#{}: {} (conf={:.2})", p.id, t, p.confidence.unwrap_or(0.0)));
                    }
                    Some("retracted") => {
                        let t = if p.text.len() > 60 { &p.text[..60] } else { &p.text };
                        retracted_items.push(format!("  P#{}: {} (conf={:.2})", p.id, t, p.confidence.unwrap_or(0.0)));
                    }
                    _ => {}
                }
            }
        }

        if !suspended_items.is_empty() {
            text += &format!("\n## Suspended Nodes (TMS): {}\n", suspended_items.len());
            for item in &suspended_items {
                text += &format!("{}\n", item);
            }
        }
        if !retracted_items.is_empty() {
            text += &format!("\n## Retracted Nodes (TMS): {}\n", retracted_items.len());
            for item in &retracted_items {
                text += &format!("{}\n", item);
            }
        }
    }

    text
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

pub fn tool_stats(store: &SqliteStore, project: &str) -> String {
    let proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(p) => p,
        None => return "Not found".to_string(),
    };
    let phases = store.list_phases(proj.id).unwrap_or_default();
    let mut ec = 0;
    let mut fc = 0;
    let mut hc = 0;
    for p in &phases {
        ec += store.list_experiments(Some(p.id)).map(|e| e.len()).unwrap_or(0);
        hc += store.list_hypotheses(Some(p.id)).map(|h| h.len()).unwrap_or(0);
        for e in store.list_experiments(Some(p.id)).unwrap_or_default() {
            fc += store.list_findings(Some(e.id)).map(|f| f.len()).unwrap_or(0);
        }
    }
    // Count project-scoped edges: build set of all node IDs belonging to this project
    let mut project_node_ids: std::collections::HashSet<(NodeType, i64)> = std::collections::HashSet::new();
    for p in &phases {
        project_node_ids.insert((NodeType::Phase, p.id));
        for e in store.list_experiments(Some(p.id)).unwrap_or_default() {
            project_node_ids.insert((NodeType::Experiment, e.id));
            for f in store.list_findings(Some(e.id)).unwrap_or_default() {
                project_node_ids.insert((NodeType::Finding, f.id));
            }
        }
        for r in store.list_research(Some(p.id)).unwrap_or_default() {
            project_node_ids.insert((NodeType::Research, r.id));
        }
        for h in store.list_hypotheses(Some(p.id)).unwrap_or_default() {
            project_node_ids.insert((NodeType::Hypothesis, h.id));
        }
    }
    for d in store.list_decisions(proj.id).unwrap_or_default() {
        project_node_ids.insert((NodeType::Decision, d.id));
    }
    for p in store.list_principles(proj.id).unwrap_or_default() {
        project_node_ids.insert((NodeType::Principle, p.id));
    }
    for c in store.list_constraints(proj.id).unwrap_or_default() {
        project_node_ids.insert((NodeType::Constraint, c.id));
    }
    for l in store.list_literature(proj.id).unwrap_or_default() {
        project_node_ids.insert((NodeType::Literature, l.id));
    }
    let edge_count = store.list_all_edges().map(|edges| {
        edges.iter().filter(|e| {
            project_node_ids.contains(&(e.source_type.clone(), e.source_id))
                || project_node_ids.contains(&(e.target_type.clone(), e.target_id))
        }).count()
    }).unwrap_or(0);
    let t = format!("Phases:{} Exp:{} Find:{} Dec:{} Princ:{} Hyp:{} Con:{} Lit:{} Edges:{}",
        phases.len(), ec, fc,
        store.list_decisions(proj.id).map(|d| d.len()).unwrap_or(0),
        store.list_principles(proj.id).map(|p| p.len()).unwrap_or(0),
        hc,
        store.list_constraints(proj.id).map(|c| c.len()).unwrap_or(0),
        store.list_literature(proj.id).map(|l| l.len()).unwrap_or(0),
        edge_count);
    t
}

pub fn tool_search(store: &SqliteStore, query: &str) -> String {
    match store.text_search(query) {
        Ok(results) => {
            if results.is_empty() {
                return format!("=== Search Results for \"{}\" ===\n\nNo results found.\n", query);
            }

            // Post-query composite scoring
            let now = chrono::Local::now().naive_local();
            let mut scored: Vec<(f64, f64, f64, f64, &crate::store::SearchResult)> = Vec::new();

            for r in &results {
                // Capitalize node_type for edge table lookups (edges store "Finding", not "finding")
                let edge_node_type = capitalize(&r.node_type);

                // Edge count (graph connectivity)
                let edge_count: i64 = store.count_edges_for_node(&edge_node_type, r.node_id).unwrap_or(0);

                // Evidence weight: supports - contradicts edges pointing TO this node
                let evidence_weight: i64 = store.evidence_weight_for_node(&edge_node_type, r.node_id).unwrap_or(0);

                // Recency bonus: 1.0 for today, decays to 0.0 over 30 days
                let recency_bonus = match &r.modified_at {
                    Some(ts) => {
                        if let Ok(modified) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
                            let days_ago = (now - modified).num_days().max(0) as f64;
                            (1.0 - days_ago / 30.0).max(0.0)
                        } else {
                            0.0
                        }
                    },
                    None => 0.0,
                };

                // Text relevance: fraction of query words found in the result text
                let query_lower = query.to_lowercase();
                let query_words: Vec<&str> = query_lower.split_whitespace().filter(|w| w.len() >= 2).collect();
                let result_lower = r.text_excerpt.to_lowercase();
                let matches = query_words.iter().filter(|w| result_lower.contains(*w)).count();
                let text_match = if query_words.is_empty() { 1.0 } else { matches as f64 / query_words.len() as f64 };
                let score = text_match * 1.0
                    + edge_count as f64 * 0.1
                    + evidence_weight as f64 * 0.2
                    + recency_bonus * 0.3;

                scored.push((score, edge_count as f64, evidence_weight as f64, recency_bonus, r));
            }

            // Sort by composite score descending
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

            let mut text = format!("=== Search Results for \"{}\" ===\n\n", query);
            for (score, edges, evidence, recency, r) in &scored {
                let type_label = capitalize(&r.node_type);
                let seq_label = match r.project_seq {
                    Some(s) => {
                        let prefix = match r.node_type.as_str() {
                            "finding" => "F",
                            "decision" => "D",
                            "hypothesis" => "H",
                            "literature" => "L",
                            "phase" => "Ph",
                            "research" => "R",
                            "experiment" => "E",
                            "principle" => "P",
                            "constraint" => "C",
                            _ => "?",
                        };
                        format!(" ({}#{})", prefix, s)
                    },
                    None => String::new(),
                };
                let excerpt = if r.text_excerpt.len() >= 147 {
                    format!("{}...", &r.text_excerpt)
                } else {
                    r.text_excerpt.clone()
                };
                text += &format!("  {} #{}{}: {}\n", type_label, r.node_id, seq_label, excerpt);
                                let conf_str = match r.confidence {
                    Some(c) => format!(", conf={:.2}", c),
                    None => String::new(),
                };
                let belief_str = match &r.belief_status {
                    Some(s) if s != "believed" => format!(", {}", s),
                    _ => String::new(),
                };
                text += &format!("    score={:.2} [edges={:.0}, evidence={:.0}, recency={:.2}{}{}]\n", score, edges, evidence, recency, conf_str, belief_str);
                text += &format!("    -> pm_kg_traverse node_type={} node_id={}\n\n", r.node_type, r.node_id);
            }
            text += &format!("{} results found.\n", scored.len());
            text
        },
        Err(e) => format!("Search error: {}", e),
    }
}

/// Natural language KG query — search + auto-traverse top results + synthesize.
pub fn tool_query(store: &SqliteStore, query: &str) -> String {
    // Step 1: Search
    let results = match store.text_search(query) {
        Ok(r) => r,
        Err(e) => return format!("Search error: {}", e),
    };
    if results.is_empty() {
        return format!("No results found for: {}", query);
    }

    // Step 2: Score and rank (same as tool_search but simplified)
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().filter(|w| w.len() >= 2).collect();

    let mut scored: Vec<(f64, &crate::store::SearchResult)> = results.iter().map(|r| {
        let result_lower = r.text_excerpt.to_lowercase();
        let matches = query_words.iter().filter(|w| result_lower.contains(*w)).count();
        let text_match = if query_words.is_empty() { 1.0 } else { matches as f64 / query_words.len() as f64 };
        let edge_count = store.count_edges_for_node(&capitalize(&r.node_type), r.node_id).unwrap_or(0) as f64;
        let score = text_match * 2.0 + edge_count * 0.1;
        (score, r)
    }).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Step 3: Take top 3 results and expand their neighborhoods
    let mut text = format!("=== Query: \"{}\" ===\n\n", query);
    let top = scored.iter().take(3);

    for (score, r) in top {
        let type_cap = capitalize(&r.node_type);
        text += &format!("--- {} #{} (score={:.2}) ---\n", type_cap, r.node_id, score);
        text += &format!("{}\n\n", r.text_excerpt);

        // Get neighbors
        let nt = match r.node_type.as_str() {
            "finding" => crate::store::NodeType::Finding,
            "decision" => crate::store::NodeType::Decision,
            "hypothesis" => crate::store::NodeType::Hypothesis,
            "principle" => crate::store::NodeType::Principle,
            "experiment" => crate::store::NodeType::Experiment,
            "phase" => crate::store::NodeType::Phase,
            "literature" => crate::store::NodeType::Literature,
            "constraint" => crate::store::NodeType::Constraint,
            _ => continue,
        };

        // Show outgoing edges
        if let Ok(edges) = store.get_edges_from(nt.clone(), r.node_id) {
            for e in edges.iter().take(3) {
                text += &format!("  -> {:?} {:?} #{}\n", e.relation, e.target_type, e.target_id);
            }
        }
        if let Ok(edges) = store.get_edges_to(nt, r.node_id) {
            for e in edges.iter().take(3) {
                text += &format!("  <- {:?} {:?} #{}\n", e.relation, e.source_type, e.source_id);
            }
        }
        text += "\n";
    }

    text += &format!("{} total results. Showing top 3 with neighbors.\n", scored.len());
    text
}

/// Orphan repair tool -- deep structural analysis of a project's KG.
///
/// Detects 8 categories of structural issues:
/// 1. Decisions without causal upstream (no experiment_id AND no incoming edges)
/// 2. Decisions without project_id
/// 3. Orphaned hypotheses (0 edges)
/// 4. Orphaned principles (0 edges)
/// 5. Orphaned constraints (0 edges)
/// 6. Research nodes with no phase
/// 7. True orphans -- nodes with zero edges of any kind
/// 8. Causal chain breaks -- decisions referencing experiment_ids from other projects
pub fn tool_orphan_repair(store: &SqliteStore, project: &str) -> String {
    let proj = match store.list_projects().ok().and_then(|ps| {
        ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))
    }) {
        Some(p) => p,
        None => return format!("Project not found: {}", project),
    };

    let mut out = format!("=== Orphan Repair Report: {} ===\n", proj.name);
    let mut issues: Vec<RepairIssue> = Vec::new();

    // Collect all project-scoped nodes
    let phases = store.list_phases(proj.id).unwrap_or_default();
    let decisions = store.list_decisions(proj.id).unwrap_or_default();
    let principles = store.list_principles(proj.id).unwrap_or_default();
    let constraints = store.list_constraints(proj.id).unwrap_or_default();
    let literature = store.list_literature(proj.id).unwrap_or_default();

    // Phase-scoped collections
    let mut all_experiments = Vec::new();
    let mut all_findings = Vec::new();
    let mut all_hypotheses = Vec::new();
    let mut all_research = Vec::new();
    let mut project_experiment_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();

    for phase in &phases {
        if let Ok(exps) = store.list_experiments(Some(phase.id)) {
            for exp in &exps {
                project_experiment_ids.insert(exp.id);
                if let Ok(fs) = store.list_findings(Some(exp.id)) {
                    all_findings.extend(fs);
                }
            }
            all_experiments.extend(exps);
        }
        if let Ok(hs) = store.list_hypotheses(Some(phase.id)) {
            all_hypotheses.extend(hs);
        }
        if let Ok(rs) = store.list_research(Some(phase.id)) {
            all_research.extend(rs);
        }
    }

    // === Check 1: Decisions without causal upstream ===
    for d in &decisions {
        let has_experiment = d.experiment_id.is_some();
        let incoming = store.get_edges_to(NodeType::Decision, d.id).unwrap_or_default();
        let has_causal_incoming = incoming.iter().any(|e| {
            matches!(e.relation, crate::store::EdgeType::Informed
                | crate::store::EdgeType::Supports
                | crate::store::EdgeType::ProducedBy
                | crate::store::EdgeType::DerivedFrom)
        });
        if !has_experiment && !has_causal_incoming {
            let dref = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D(id:{})", d.id));
            let what_trunc: String = d.what.chars().take(60).collect();
            // Find a nearby experiment to suggest
            let suggestion = if let Some(last_exp) = all_experiments.last() {
                let eref = last_exp.project_seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E(id:{})", last_exp.id));
                format!("Link to {} via pm_add_edge or create a finding that leads to it", eref)
            } else {
                "Create an experiment or finding that motivates this decision".to_string()
            };
            issues.push(RepairIssue {
                severity: Severity::High,
                category: "Decision without causal upstream".to_string(),
                node_ref: dref,
                description: what_trunc,
                repair: suggestion,
            });
        }
    }

    // === Check 2: Decisions without project_id ===
    for d in &decisions {
        if d.project_id.is_none() {
            let dref = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D(id:{})", d.id));
            let what_trunc: String = d.what.chars().take(60).collect();
            issues.push(RepairIssue {
                severity: Severity::Medium,
                category: "Decision without project_id".to_string(),
                node_ref: dref,
                description: what_trunc,
                repair: format!("Backfill project_id to {} (id:{})", proj.name, proj.id),
            });
        }
    }

    // === Check 3: Orphaned hypotheses ===
    for h in &all_hypotheses {
        let from = store.get_edges_from(NodeType::Hypothesis, h.id).unwrap_or_default();
        let to = store.get_edges_to(NodeType::Hypothesis, h.id).unwrap_or_default();
        if from.is_empty() && to.is_empty() {
            let href = h.project_seq.map(|s| format!("H#{}", s)).unwrap_or_else(|| format!("H(id:{})", h.id));
            let text_trunc: String = h.text.chars().take(60).collect();
            // Suggest linking to a phase experiment
            let suggestion = if let Some(phase) = phases.iter().find(|p| Some(p.id) == h.phase_id) {
                let exps = store.list_experiments(Some(phase.id)).unwrap_or_default();
                if let Some(exp) = exps.first() {
                    let eref = exp.project_seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E(id:{})", exp.id));
                    format!("Link via: pm_add_edge source_type=hypothesis source_id={} target_type=experiment target_id={} relation=tested_by ({})", h.id, exp.id, eref)
                } else {
                    format!("Create an experiment in phase {} to test this hypothesis", phase.id)
                }
            } else {
                "Link to an experiment via pm_add_edge relation=tested_by".to_string()
            };
            issues.push(RepairIssue {
                severity: Severity::Medium,
                category: "Orphaned hypothesis".to_string(),
                node_ref: href,
                description: text_trunc,
                repair: suggestion,
            });
        }
    }

    // === Check 4: Orphaned principles ===
    for p in &principles {
        let from = store.get_edges_from(NodeType::Principle, p.id).unwrap_or_default();
        let to = store.get_edges_to(NodeType::Principle, p.id).unwrap_or_default();
        if from.is_empty() && to.is_empty() {
            let pref = p.project_seq.map(|s| format!("P#{}", s)).unwrap_or_else(|| format!("P(id:{})", p.id));
            let text_trunc: String = p.text.chars().take(60).collect();
            // Suggest linking to a finding or decision
            let suggestion = if let Some(last_finding) = all_findings.last() {
                format!("Link via: pm_add_edge source_type=principle source_id={} target_type=finding target_id={} relation=derived_from", p.id, last_finding.id)
            } else if let Some(last_dec) = decisions.last() {
                format!("Link via: pm_add_edge source_type=principle source_id={} target_type=decision target_id={} relation=derived_from", p.id, last_dec.id)
            } else {
                "Link to a finding or decision via pm_add_edge relation=derived_from".to_string()
            };
            issues.push(RepairIssue {
                severity: Severity::Medium,
                category: "Orphaned principle".to_string(),
                node_ref: pref,
                description: text_trunc,
                repair: suggestion,
            });
        }
    }

    // === Check 5: Orphaned constraints ===
    for c in &constraints {
        let from = store.get_edges_from(NodeType::Constraint, c.id).unwrap_or_default();
        let to = store.get_edges_to(NodeType::Constraint, c.id).unwrap_or_default();
        if from.is_empty() && to.is_empty() {
            let cref = c.project_seq.map(|s| format!("C#{}", s)).unwrap_or_else(|| format!("C(id:{})", c.id));
            let text_trunc: String = c.text.chars().take(60).collect();
            let suggestion = if let Some(exp) = all_experiments.iter().rev().find(|e| e.status == crate::store::ExperimentStatus::Pass) {
                let eref = exp.project_seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E(id:{})", exp.id));
                format!("Link via: pm_add_edge source_type=constraint source_id={} target_type=experiment target_id={} relation=tested_by ({})", c.id, exp.id, eref)
            } else {
                "Link to an experiment via pm_add_edge relation=tested_by".to_string()
            };
            issues.push(RepairIssue {
                severity: Severity::Low,
                category: "Orphaned constraint".to_string(),
                node_ref: cref,
                description: text_trunc,
                repair: suggestion,
            });
        }
    }

    // === Check 6: Research nodes with no phase ===
    // Scan research globally for phase_id NULL, then filter to project-relevant
    if let Ok(all_r) = store.list_research(None) {
        for r in &all_r {
            if r.phase_id.is_none() {
                let rref = r.project_seq.map(|s| format!("R#{}", s)).unwrap_or_else(|| format!("R(id:{})", r.id));
                let name_trunc: String = r.name.chars().take(60).collect();
                let suggestion = if let Some(active_phase) = phases.iter().find(|p| p.status == crate::store::PhaseStatus::InProgress) {
                    format!("Assign to active phase '{}' (id:{})", active_phase.name, active_phase.id)
                } else if let Some(first_phase) = phases.first() {
                    format!("Assign to phase '{}' (id:{})", first_phase.name, first_phase.id)
                } else {
                    "Create a phase and assign this research to it".to_string()
                };
                issues.push(RepairIssue {
                    severity: Severity::Medium,
                    category: "Research with no phase".to_string(),
                    node_ref: rref,
                    description: name_trunc,
                    repair: suggestion,
                });
            }
        }
    }

    // === Check 7: True orphans across all types ===
    // Findings, experiments, literature with zero edges either direction
    let node_checks: Vec<(&str, NodeType, Vec<(i64, Option<i64>, String)>)> = vec![
        ("Finding", NodeType::Finding, all_findings.iter().map(|f| (f.id, f.project_seq, f.text.chars().take(60).collect())).collect()),
        ("Experiment", NodeType::Experiment, all_experiments.iter().map(|e| (e.id, e.project_seq, e.name.chars().take(60).collect())).collect()),
        ("Literature", NodeType::Literature, literature.iter().map(|l| (l.id, l.project_seq, l.title.chars().take(60).collect())).collect()),
    ];
    for (type_name, node_type, items) in &node_checks {
        for (id, seq, text) in items {
            let from = store.get_edges_from(node_type.clone(), *id).unwrap_or_default();
            let to = store.get_edges_to(node_type.clone(), *id).unwrap_or_default();
            if from.is_empty() && to.is_empty() {
                let prefix = match *type_name {
                    "Finding" => "F",
                    "Experiment" => "E",
                    "Literature" => "L",
                    _ => "?",
                };
                let nref = seq.map(|s| format!("{}#{}", prefix, s)).unwrap_or_else(|| format!("{}(id:{})", prefix, id));
                // Skip if already reported
                let already_reported = issues.iter().any(|i| i.node_ref == nref);
                if !already_reported {
                    issues.push(RepairIssue {
                        severity: Severity::Low,
                        category: format!("True orphan ({})", type_name),
                        node_ref: nref,
                        description: text.clone(),
                        repair: format!("Link via pm_add_edge to connect this {} into the KG", type_name.to_lowercase()),
                    });
                }
            }
        }
    }

    // === Check 8: Causal chain breaks -- decisions referencing experiments from other projects ===
    for d in &decisions {
        if let Some(eid) = d.experiment_id {
            if !project_experiment_ids.contains(&eid) {
                let dref = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D(id:{})", d.id));
                let what_trunc: String = d.what.chars().take(60).collect();
                // Try to find which project owns this experiment
                let owner_info = if let Ok(exp) = store.get_experiment(eid) {
                    if let Some(pid) = exp.phase_id {
                        if let Ok(phase) = store.get_phase(pid) {
                            if let Ok(other_proj) = store.get_project(phase.project_id) {
                                format!(" (belongs to project '{}')", other_proj.name)
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        " (experiment has no phase)".to_string()
                    }
                } else {
                    " (experiment not found -- dangling reference)".to_string()
                };
                issues.push(RepairIssue {
                    severity: Severity::High,
                    category: "Cross-project causal bleed".to_string(),
                    node_ref: dref,
                    description: what_trunc,
                    repair: format!("Decision references experiment_id={}{} -- reassign to a local experiment or clear the reference", eid, owner_info),
                });
            }
        }
    }

    // === Check: Branch points and merge points ===
    {
        let mut branch_info = Vec::new();
        let mut merge_info = Vec::new();
        let mut dangling_info = Vec::new();

        // Branch points: experiments with >1 downstream experiment
        for exp in &all_experiments {
            let outgoing = store.get_edges_from(NodeType::Experiment, exp.id).unwrap_or_default();
            let downstream_exp_ids: Vec<i64> = outgoing.iter()
                .filter(|e| e.target_type == NodeType::Experiment
                    && matches!(e.relation, EdgeType::Informed | EdgeType::BranchesFrom))
                .map(|e| e.target_id)
                .collect();
            if downstream_exp_ids.len() > 1 {
                let eref = exp.project_seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E(id:{})", exp.id));
                let downstream_refs: Vec<String> = downstream_exp_ids.iter().map(|d| format!("E#{}", d)).collect();
                branch_info.push(format!("  {} -> fan-out to {}: {}", eref, downstream_exp_ids.len(), downstream_refs.join(", ")));

                // Dangling branch check
                let mut pending = 0;
                for did in &downstream_exp_ids {
                    if let Ok(d_exp) = store.get_experiment(*did) {
                        if d_exp.status == crate::store::ExperimentStatus::Pending {
                            pending += 1;
                        }
                    }
                }
                if pending > 0 {
                    let eref2 = exp.project_seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E(id:{})", exp.id));
                    dangling_info.push(format!("  {} has {}/{} downstream experiments still pending", eref2, pending, downstream_exp_ids.len()));
                    issues.push(RepairIssue {
                        severity: Severity::Medium,
                        category: "Dangling branch".to_string(),
                        node_ref: eref2,
                        description: format!("{}/{} downstream experiments still pending", pending, downstream_exp_ids.len()),
                        repair: "Complete or close pending branched experiments to resolve the branch".to_string(),
                    });
                }
            }
        }

        // Merge points: decisions informed by findings from multiple experiments
        for d in &decisions {
            let incoming = store.get_edges_to(NodeType::Decision, d.id).unwrap_or_default();
            let mut source_exp_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
            if let Some(eid) = d.experiment_id {
                source_exp_ids.insert(eid);
            }
            for edge in &incoming {
                if edge.source_type == NodeType::Finding {
                    if let Ok(f) = store.get_finding(edge.source_id) {
                        if let Some(eid) = f.experiment_id {
                            source_exp_ids.insert(eid);
                        }
                    }
                }
                if edge.source_type == NodeType::Experiment {
                    source_exp_ids.insert(edge.source_id);
                }
            }
            if source_exp_ids.len() > 1 {
                let dref = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D(id:{})", d.id));
                let exp_refs: Vec<String> = source_exp_ids.iter().map(|e| format!("E#{}", e)).collect();
                merge_info.push(format!("  {} <- converges: {}", dref, exp_refs.join(", ")));
            }
        }

        if !branch_info.is_empty() || !merge_info.is_empty() {
            out += &format!("\n--- Causal Backbone Topology ---\n");
        }
        if !branch_info.is_empty() {
            out += &format!("Branch points ({}):\n", branch_info.len());
            for b in &branch_info {
                out += &format!("{}\n", b);
            }
        }
        if !merge_info.is_empty() {
            out += &format!("Merge points ({}):\n", merge_info.len());
            for m in &merge_info {
                out += &format!("{}\n", m);
            }
        }
        if !dangling_info.is_empty() {
            out += &format!("Dangling branches ({}):\n", dangling_info.len());
            for d in &dangling_info {
                out += &format!("{}\n", d);
            }
        }
    }

    // Format output
    if issues.is_empty() {
        out += "\nNo structural issues found. KG is clean.\n";
    } else {
        // Count by severity
        let high = issues.iter().filter(|i| i.severity == Severity::High).count();
        let medium = issues.iter().filter(|i| i.severity == Severity::Medium).count();
        let low = issues.iter().filter(|i| i.severity == Severity::Low).count();
        out += &format!("\nFound {} issues: {} HIGH, {} MEDIUM, {} LOW\n", issues.len(), high, medium, low);

        // Group by category
        let mut by_category: std::collections::BTreeMap<String, Vec<&RepairIssue>> = std::collections::BTreeMap::new();
        for issue in &issues {
            by_category.entry(issue.category.clone()).or_default().push(issue);
        }

        for (cat, cat_issues) in &by_category {
            let sev = &cat_issues[0].severity;
            let sev_tag = match sev {
                Severity::High => "HIGH",
                Severity::Medium => "MED",
                Severity::Low => "LOW",
            };
            out += &format!("\n## [{}] {} ({})\n", sev_tag, cat, cat_issues.len());
            for issue in cat_issues {
                out += &format!("  {} -- {}\n", issue.node_ref, issue.description);
                out += &format!("    Repair: {}\n", issue.repair);
            }
        }
    }

    // Summary stats
    out += "\n--- KG Stats ---\n";
    out += &format!("Phases: {}, Experiments: {}, Findings: {}\n", phases.len(), all_experiments.len(), all_findings.len());
    out += &format!("Decisions: {}, Hypotheses: {}, Research: {}\n", decisions.len(), all_hypotheses.len(), all_research.len());
    out += &format!("Principles: {}, Constraints: {}, Literature: {}\n", principles.len(), constraints.len(), literature.len());

    // Count total edges for this project
    let edge_count = store.list_all_edges().map(|edges| {
        let mut project_node_ids: std::collections::HashSet<(NodeType, i64)> = std::collections::HashSet::new();
        for p in &phases { project_node_ids.insert((NodeType::Phase, p.id)); }
        for e in &all_experiments { project_node_ids.insert((NodeType::Experiment, e.id)); }
        for f in &all_findings { project_node_ids.insert((NodeType::Finding, f.id)); }
        for d in &decisions { project_node_ids.insert((NodeType::Decision, d.id)); }
        for h in &all_hypotheses { project_node_ids.insert((NodeType::Hypothesis, h.id)); }
        for r in &all_research { project_node_ids.insert((NodeType::Research, r.id)); }
        for pp in &principles { project_node_ids.insert((NodeType::Principle, pp.id)); }
        for c in &constraints { project_node_ids.insert((NodeType::Constraint, c.id)); }
        for l in &literature { project_node_ids.insert((NodeType::Literature, l.id)); }
        edges.iter().filter(|e| {
            project_node_ids.contains(&(e.source_type.clone(), e.source_id))
                || project_node_ids.contains(&(e.target_type.clone(), e.target_id))
        }).count()
    }).unwrap_or(0);
    out += &format!("Edges: {}\n", edge_count);

    out
}

#[derive(Debug, PartialEq)]
enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Debug)]
struct RepairIssue {
    severity: Severity,
    category: String,
    node_ref: String,
    description: String,
    repair: String,
}

/// KG structural audit: validates causal backbone compliance and reports health score.
pub fn tool_kg_audit(store: &SqliteStore, project: &str) -> String {
    let proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(p) => p,
        None => return format!("Project not found: {}", project),
    };

    let mut text = format!("=== KG Audit: {} ===\n\n", proj.name);
    let mut issues: Vec<String> = Vec::new();
    let mut scores: Vec<f64> = Vec::new(); // individual metric scores 0-100

    // Collect all project nodes
    let phases = store.list_phases(proj.id).unwrap_or_default();
    let mut all_experiments = Vec::new();
    let mut all_findings = Vec::new();
    let mut all_hypotheses = Vec::new();
    let mut all_research = Vec::new();
    for phase in &phases {
        if let Ok(exps) = store.list_experiments(Some(phase.id)) {
            for exp in &exps {
                if let Ok(findings) = store.list_findings(Some(exp.id)) {
                    all_findings.extend(findings);
                }
            }
            all_experiments.extend(exps);
        }
        if let Ok(hyps) = store.list_hypotheses(Some(phase.id)) {
            all_hypotheses.extend(hyps);
        }
        if let Ok(research) = store.list_research(Some(phase.id)) {
            all_research.extend(research);
        }
    }
    let decisions = store.list_decisions(proj.id).unwrap_or_default();
    let principles = store.list_principles(proj.id).unwrap_or_default();
    let constraints = store.list_constraints(proj.id).unwrap_or_default();
    let literature = store.list_literature(proj.id).unwrap_or_default();

    let total_nodes = phases.len() + all_experiments.len() + all_findings.len()
        + all_hypotheses.len() + all_research.len() + decisions.len()
        + principles.len() + constraints.len() + literature.len();

    text += &format!("Total nodes: {} ({}P {}E {}F {}D {}H {}R {}Pr {}C {}L)\n\n",
        total_nodes, phases.len(), all_experiments.len(), all_findings.len(),
        decisions.len(), all_hypotheses.len(), all_research.len(),
        principles.len(), constraints.len(), literature.len());

    // ---- 1. Causal Chain Completeness ----
    text += "## 1. Causal Chain Completeness\n";
    let mut broken_chains = Vec::new();
    let mut decisions_with_upstream = 0;

    for d in &decisions {
        // Check if decision has any incoming edges (Finding/Experiment --Informed--> Decision)
        let to_edges = store.get_edges_to(NodeType::Decision, d.id).unwrap_or_default();
        let has_upstream = to_edges.iter().any(|e| {
            matches!(e.relation, EdgeType::Informed | EdgeType::ProducedBy | EdgeType::Supports)
        });
        if has_upstream {
            decisions_with_upstream += 1;
        } else {
            let dref = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D#{}", d.id));
            let what_short: String = d.what.chars().take(50).collect();
            broken_chains.push(format!("  {} (no causal upstream): {}", dref, what_short));
        }
    }

    // Also check findings that have no experiment link
    let mut orphan_findings = 0;
    for f in &all_findings {
        if f.experiment_id.is_none() {
            let to_edges = store.get_edges_to(NodeType::Finding, f.id).unwrap_or_default();
            if to_edges.is_empty() {
                orphan_findings += 1;
            }
        }
    }

    if broken_chains.is_empty() && orphan_findings == 0 {
        text += "  All decisions trace to upstream evidence. No orphan findings.\n";
        scores.push(100.0);
    } else {
        if !broken_chains.is_empty() {
            text += &format!("  BROKEN: {} decision(s) without causal upstream:\n", broken_chains.len());
            for bc in &broken_chains {
                text += &format!("{}\n", bc);
                issues.push(bc.clone());
            }
        }
        if orphan_findings > 0 {
            text += &format!("  ORPHAN: {} finding(s) with no experiment or incoming edge\n", orphan_findings);
            issues.push(format!("{} orphan finding(s)", orphan_findings));
        }
        let total_evidence_nodes = decisions.len() + all_findings.len();
        if total_evidence_nodes > 0 {
            let connected = decisions_with_upstream + (all_findings.len() - orphan_findings);
            scores.push((connected as f64 / total_evidence_nodes as f64) * 100.0);
        } else {
            scores.push(100.0);
        }
    }

    // ---- 2. Hypothesis Coverage ----
    text += "\n## 2. Hypothesis Coverage\n";
    if all_hypotheses.is_empty() {
        text += "  No hypotheses recorded.\n";
        scores.push(50.0); // neutral -- no hypotheses is not great but not broken
    } else {
        let tested = all_hypotheses.iter().filter(|h| {
            matches!(h.status, HypothesisStatus::Testing | HypothesisStatus::Confirmed | HypothesisStatus::Refuted)
        }).count();
        let pct = (tested as f64 / all_hypotheses.len() as f64) * 100.0;
        text += &format!("  {}/{} hypotheses tested ({:.0}%)\n", tested, all_hypotheses.len(), pct);

        let untested: Vec<_> = all_hypotheses.iter()
            .filter(|h| h.status == HypothesisStatus::Proposed)
            .collect();
        if !untested.is_empty() {
            text += &format!("  Untested ({}):\n", untested.len());
            for h in untested.iter().take(5) {
                let t: String = h.text.chars().take(60).collect();
                let href = h.project_seq.map(|s| format!("H#{}", s)).unwrap_or_else(|| format!("H#{}", h.id));
                text += &format!("    {}: {}\n", href, t);
            }
        }
        scores.push(pct);
    }

    // ---- 3. Literature Utilization ----
    text += "\n## 3. Literature Utilization\n";
    if literature.is_empty() {
        text += "  No literature entries.\n";
        scores.push(50.0);
    } else {
        let utilized = literature.iter().filter(|l| {
            match l.status.as_deref() {
                Some("cited") | Some("tested") | Some("promising") | Some("integrated") => true,
                _ => false,
            }
        }).count();
        let pct = (utilized as f64 / literature.len() as f64) * 100.0;
        text += &format!("  {}/{} literature entries utilized ({:.0}%)\n", utilized, literature.len(), pct);

        let unread: Vec<_> = literature.iter()
            .filter(|l| l.status.as_deref() == Some("unread") || l.status.is_none())
            .collect();
        if !unread.is_empty() {
            text += &format!("  Unread ({}):\n", unread.len());
            for l in unread.iter().take(5) {
                let lref = l.project_seq.map(|s| format!("L#{}", s)).unwrap_or_else(|| format!("L#{}", l.id));
                let t: String = l.title.chars().take(60).collect();
                text += &format!("    {}: {}\n", lref, t);
            }
        }
        scores.push(pct);
    }

    // ---- 4. Edge Density ----
    text += "\n## 4. Edge Density\n";
    let all_edges = store.list_all_edges().unwrap_or_default();

    // Build set of project node IDs for edge filtering
    let mut project_node_ids: std::collections::HashSet<(NodeType, i64)> = std::collections::HashSet::new();
    for p in &phases {
        project_node_ids.insert((NodeType::Phase, p.id));
    }
    for e in &all_experiments {
        project_node_ids.insert((NodeType::Experiment, e.id));
    }
    for f in &all_findings {
        project_node_ids.insert((NodeType::Finding, f.id));
    }
    for h in &all_hypotheses {
        project_node_ids.insert((NodeType::Hypothesis, h.id));
    }
    for r in &all_research {
        project_node_ids.insert((NodeType::Research, r.id));
    }
    for d in &decisions {
        project_node_ids.insert((NodeType::Decision, d.id));
    }
    for p in &principles {
        project_node_ids.insert((NodeType::Principle, p.id));
    }
    for c in &constraints {
        project_node_ids.insert((NodeType::Constraint, c.id));
    }
    for l in &literature {
        project_node_ids.insert((NodeType::Literature, l.id));
    }

    let project_edges: Vec<_> = all_edges.iter().filter(|e| {
        project_node_ids.contains(&(e.source_type.clone(), e.source_id))
            || project_node_ids.contains(&(e.target_type.clone(), e.target_id))
    }).collect();

    let edge_count = project_edges.len();
    let density = if total_nodes > 0 { edge_count as f64 / total_nodes as f64 } else { 0.0 };
    text += &format!("  {} edges across {} nodes (density: {:.2} edges/node)\n", edge_count, total_nodes, density);

    if density < 2.0 && total_nodes > 5 {
        text += "  WARNING: Sparse graph (< 2.0 edges/node). Add more cross-references.\n";
        issues.push(format!("Sparse graph: {:.2} edges/node", density));
    }
    // Score: 100 at density >= 3.0, 0 at density == 0
    scores.push((density / 3.0 * 100.0).min(100.0));

    // ---- 5. Temporal Coherence ----
    text += "\n## 5. Temporal Coherence\n";
    let mut temporal_violations = Vec::new();

    // Check: decisions should not be created before their supporting findings
    for d in &decisions {
        let to_edges = store.get_edges_to(NodeType::Decision, d.id).unwrap_or_default();
        for edge in &to_edges {
            if edge.source_type == NodeType::Finding {
                if let Ok(finding) = store.get_finding(edge.source_id) {
                    if finding.created_at > d.created_at {
                        let dref = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D#{}", d.id));
                        let fref = finding.project_seq.map(|s| format!("F#{}", s)).unwrap_or_else(|| format!("F#{}", finding.id));
                        temporal_violations.push(format!(
                            "  {} (created {}) informed by {} (created {} -- AFTER decision)",
                            dref, d.created_at.format("%Y-%m-%d %H:%M"),
                            fref, finding.created_at.format("%Y-%m-%d %H:%M")
                        ));
                    }
                }
            }
        }
    }

    // Check: hypotheses confirmed/refuted before their testing experiments
    for h in &all_hypotheses {
        if matches!(h.status, HypothesisStatus::Confirmed | HypothesisStatus::Refuted) {
            if let Some(eid) = h.experiment_id {
                if let Ok(exp) = store.get_experiment(eid) {
                    if exp.created_at > h.created_at {
                        let href = h.project_seq.map(|s| format!("H#{}", s)).unwrap_or_else(|| format!("H#{}", h.id));
                        temporal_violations.push(format!(
                            "  {} resolved before testing experiment #{} was created",
                            href, eid
                        ));
                    }
                }
            }
        }
    }

    if temporal_violations.is_empty() {
        text += "  No temporal coherence violations.\n";
        scores.push(100.0);
    } else {
        text += &format!("  {} temporal violation(s):\n", temporal_violations.len());
        for tv in &temporal_violations {
            text += &format!("{}\n", tv);
            issues.push(tv.clone());
        }
        // Deduct based on violation count relative to total evidence relationships
        let evidence_count = (decisions.len() + all_hypotheses.len()).max(1);
        let violation_ratio = temporal_violations.len() as f64 / evidence_count as f64;
        scores.push(((1.0 - violation_ratio) * 100.0).max(0.0));
    }

    // ---- 6. Cross-Project References ----
    text += "\n## 6. Cross-Project References\n";
    let mut cross_refs = Vec::new();

    for edge in &project_edges {
        let source_in = project_node_ids.contains(&(edge.source_type.clone(), edge.source_id));
        let target_in = project_node_ids.contains(&(edge.target_type.clone(), edge.target_id));
        if source_in != target_in {
            // One end is in the project, the other is not
            let (inside_type, inside_id, outside_type, outside_id) = if source_in {
                (&edge.source_type, edge.source_id, &edge.target_type, edge.target_id)
            } else {
                (&edge.target_type, edge.target_id, &edge.source_type, edge.source_id)
            };
            cross_refs.push(format!(
                "  Edge #{}: {:?}#{} ({}) <-> {:?}#{} (external) [{:?}]",
                edge.id, inside_type, inside_id, proj.name,
                outside_type, outside_id, edge.relation
            ));
        }
    }

    if cross_refs.is_empty() {
        text += "  No cross-project references.\n";
    } else {
        text += &format!("  {} cross-project edge(s):\n", cross_refs.len());
        for cr in cross_refs.iter().take(10) {
            text += &format!("{}\n", cr);
        }
    }
    // Cross-project refs are informational, not scored
    scores.push(100.0); // neutral

    // ---- 7. Overall Health Score ----
    let health_score = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f64>() / scores.len() as f64
    };

    text += &format!("\n## Overall Health Score: {:.0}/100\n", health_score);

    // Breakdown
    text += "\nMetric breakdown:\n";
    let metric_names = [
        "Causal chain completeness",
        "Hypothesis coverage",
        "Literature utilization",
        "Edge density",
        "Temporal coherence",
        "Cross-project (info only)",
    ];
    for (i, name) in metric_names.iter().enumerate() {
        if i < scores.len() {
            let bar_len = (scores[i] / 5.0) as usize;
            let bar: String = std::iter::repeat('#').take(bar_len).collect();
            let empty: String = std::iter::repeat('-').take(20 - bar_len).collect();
            text += &format!("  {:30} [{}{:}] {:.0}\n", name, bar, empty, scores[i]);
        }
    }

    if health_score >= 80.0 {
        text += "\nVerdict: HEALTHY -- causal backbone is well-connected.\n";
    } else if health_score >= 50.0 {
        text += "\nVerdict: NEEDS ATTENTION -- some structural gaps in the knowledge graph.\n";
    } else {
        text += "\nVerdict: CRITICAL -- significant gaps in causal backbone. Review issues above.\n";
    }

    if !issues.is_empty() {
        text += &format!("\n## Issues Summary ({}):\n", issues.len());
        for (i, issue) in issues.iter().enumerate() {
            text += &format!("  {}. {}\n", i + 1, issue.trim());
        }
    }

    text
}
