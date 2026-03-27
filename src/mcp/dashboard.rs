//! MCP tool implementations for dashboard/session/scaffold operations.
//!
//! Contains: pm_dashboard, pm_next, pm_session_init, pm_scaffold

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, PhaseStatus, ExperimentStatus};
use crate::dag::DagEngine;

pub fn tool_dashboard(store: &SqliteStore) -> String {
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

pub fn tool_next(store: &SqliteStore, project: &str) -> String {
    let mut out = String::new();
    if let Ok(projects) = store.list_projects() {
        if let Some(proj) = projects.iter().find(|p| p.name == project || p.alias.as_deref() == Some(project)) {
            let dag = DagEngine::new(store, proj.id);
            if let Ok(next) = dag.next_phases() {
                out += "=== Next Phases (by impact) ===\n\n";
                for phase in next.iter().take(3) {
                    let s = if phase.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                    out += &format!("  {} #{} [impact:{}] {}\n", s, phase.id, phase.impact, phase.name);

                    // Show phase dependency info
                    if !phase.depends_on.is_empty() {
                        let dep_strs: Vec<String> = phase.depends_on.iter().map(|d| format!("#{}", d)).collect();
                        out += &format!("    Depends on: {}\n", dep_strs.join(", "));
                    }
                }
            }
            if let Ok(Some(n)) = dag.stagnation_check(3) {
                out += &format!("\n  WARNING: STAGNATION \u{2014} {} consecutive failed experiments\n", n);
            }
            out += "\n## ACTION: Execute the top phase.";
        } else {
            out += &format!("Project not found: {}", project);
        }
    }
    out
}

pub fn tool_scaffold(store: &SqliteStore, project: &str, phase_id: i64) -> String {
    let _proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
        Some(p) => p,
        None => return format!("Project not found: {}", project),
    };
    let phase = match store.get_phase(phase_id) {
        Ok(p) => p,
        Err(e) => return format!("Phase not found: {}", e),
    };
    let exps = store.list_experiments(Some(phase_id)).unwrap_or_default();
    let pending: Vec<_> = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).collect();
    let pass_count = exps.iter().filter(|e| e.status == ExperimentStatus::Pass).count();
    let fail_count = exps.iter().filter(|e| e.status == ExperimentStatus::Fail).count();
    let inconclusive_count = exps.iter().filter(|e| e.status == ExperimentStatus::Inconclusive).count();

    let mut text = format!("=== Phase #{} ({}) ===\n\n", phase.id, phase.name);

    // Roll-up view: experiment counts by status
    text += &format!("## Experiment Summary: {} total ({} pending, {} pass, {} fail, {} inconclusive)\n\n",
        exps.len(), pending.len(), pass_count, fail_count, inconclusive_count);

    // Finding count
    let mut finding_count = 0;
    for e in &exps {
        finding_count += store.list_findings(Some(e.id)).map(|f| f.len()).unwrap_or(0);
    }
    text += &format!("Findings: {}\n", finding_count);

    // Open hypotheses count (scoped to this phase)
    let hyp_count = store.list_hypotheses(Some(phase_id)).map(|hs| {
        hs.iter().filter(|h| h.status == crate::store::HypothesisStatus::Proposed || h.status == crate::store::HypothesisStatus::Testing).count()
    }).unwrap_or(0);
    text += &format!("Open hypotheses: {}\n\n", hyp_count);

    // Phase description/goals if available
    if let Some(ref desc) = phase.description {
        text += &format!("Description: {}\n", desc);
    }
    if let Some(ref goals) = phase.goals {
        text += &format!("Goals: {}\n", goals);
    }
    if let Some(ref criteria) = phase.success_criteria {
        text += &format!("Success criteria: {}\n", criteria);
    }
    if phase.description.is_some() || phase.goals.is_some() || phase.success_criteria.is_some() {
        text += "\n";
    }

    // Phase dependencies
    if !phase.depends_on.is_empty() {
        let dep_strs: Vec<String> = phase.depends_on.iter().map(|d| {
            match store.get_phase(*d) {
                Ok(p) => format!("#{} ({}, {:?})", p.id, p.name, p.status),
                Err(_) => format!("#{}", d),
            }
        }).collect();
        text += &format!("Dependencies: {}\n\n", dep_strs.join(", "));
    }

    // Pending experiments as tasks
    if !pending.is_empty() {
        text += &format!("--- {} Pending Experiments ---\n\n", pending.len());
        for e in &pending {
            text += &format!("TASK: Exp #{}: {}\n", e.id, e.name);
            if let Some(notes) = &e.notes {
                text += &format!("  {}\n", &notes[..notes.len().min(200)]);
            }
            text += "\n";
        }
    }

    // Active constraints (#13)
    if let Ok(constraints) = store.list_constraints(phase.project_id) {
        if !constraints.is_empty() {
            text += "--- Active Constraints ---\n\n";
            for c in &constraints {
                let sev = c.severity.as_deref().unwrap_or("hard");
                let src = c.source.as_deref().unwrap_or("unknown");
                let t = if c.text.len() > 80 { &c.text[..80] } else { &c.text };
                text += &format!("  C#{} [{}]: {} (source: {})\n", c.id, sev, t, src);
            }
            text += "\n";
        }
    }

    text
}

pub fn tool_session_init(store: &SqliteStore) -> String {
    let mut out = String::new();
    if let Ok(projects) = store.list_projects() {
        for proj in &projects {
            if proj.status != crate::store::ProjectStatus::Active { continue; }
            let dag = DagEngine::new(store, proj.id);
            if let Ok(phases) = dag.next_phases() {
                let actionable: Vec<_> = phases.iter()
                    .filter(|p| p.status == PhaseStatus::InProgress || p.status == PhaseStatus::Pending)
                    .take(3)
                    .collect();

                for phase in &actionable {
                    if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                        let pending: Vec<_> = exps.iter()
                            .filter(|e| e.status == ExperimentStatus::Pending)
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
