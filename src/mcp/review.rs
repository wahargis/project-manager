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
