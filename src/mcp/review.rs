//! MCP tool implementations for review/stats operations.
//!
//! Contains: pm_review, pm_stats

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, NodeType, HypothesisStatus};
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
    // Count project-scoped edges
    let phase_ids: std::collections::HashSet<i64> = phases.iter().map(|p| p.id).collect();
    let edge_count = store.list_all_edges().map(|edges| {
        edges.iter().filter(|e| {
            use crate::store::NodeType;
            match e.source_type {
                NodeType::Phase => phase_ids.contains(&e.source_id),
                _ => true,
            }
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
