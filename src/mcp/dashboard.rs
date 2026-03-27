//! MCP tool implementations for dashboard/session/scaffold operations.
//!
//! Contains: pm_dashboard, pm_next, pm_session_init, pm_scaffold
//! Sprint 4: DAG→TaskTracker scaffold (#16) — structured, actionable output
//! with TaskCreate guidance and stale hypothesis / orphaned finding detection.

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, PhaseStatus, ExperimentStatus, HypothesisStatus};
use crate::dag::DagEngine;
use chrono::{NaiveDateTime, Utc};

/// Check if a timestamp is older than `days` from now.
fn is_stale(ts: &NaiveDateTime, days: i64) -> bool {
    let now = Utc::now().naive_utc();
    let diff = now.signed_duration_since(*ts);
    diff.num_days() >= days
}

pub fn tool_dashboard(store: &SqliteStore) -> String {
    let mut out = String::from("=== Cross-Project Dashboard ===\n\n");
    if let Ok(projects) = store.list_projects() {
        let active: Vec<_> = projects.iter().filter(|p| p.status == crate::store::ProjectStatus::Active).collect();
        let parents: Vec<_> = active.iter().filter(|p| p.parent_id.is_none()).collect();
        let children: Vec<_> = active.iter().filter(|p| p.parent_id.is_some()).collect();

        for parent in &parents {
            let subs: Vec<_> = children.iter().filter(|c| c.parent_id == Some(parent.id)).collect();
            if subs.is_empty() {
                // Standalone project
                let dag = DagEngine::new(store, parent.id);
                if let Ok(next) = dag.next_phases() {
                    if let Some(top) = next.first() {
                        let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                        out += &format!("  [{}] {} #{} [impact:{}] {}\n", parent.name, s, top.id, top.impact, top.name);
                    }
                }
            } else {
                // Parent with subprojects
                out += &format!("## {}\n", parent.name);
                // Parent's own phases
                let dag = DagEngine::new(store, parent.id);
                if let Ok(next) = dag.next_phases() {
                    if let Some(top) = next.first() {
                        let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                        out += &format!("  [{}] {} #{} [impact:{}] {}\n", parent.name, s, top.id, top.impact, top.name);
                    }
                }
                // Subproject phases
                for sub in &subs {
                    let dag = DagEngine::new(store, sub.id);
                    if let Ok(next) = dag.next_phases() {
                        if let Some(top) = next.first() {
                            let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                            out += &format!("  [{}/{}] {} #{} [impact:{}] {}\n", parent.name, sub.name, s, top.id, top.impact, top.name);
                        }
                    }
                }
                out += "\n";
            }
        }
    }
    out += "## ACTION: Execute the highest-impact item above.";
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

                    // Experiment summary per phase
                    if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                        let pending = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).count();
                        let pass = exps.iter().filter(|e| e.status == ExperimentStatus::Pass).count();
                        let fail = exps.iter().filter(|e| e.status == ExperimentStatus::Fail).count();
                        if !exps.is_empty() {
                            out += &format!("    Experiments: {} pending, {} pass, {} fail\n", pending, pass, fail);
                        }
                    }
                }

                // Stagnation warning with suggested action
                if let Ok(Some(n)) = dag.stagnation_check(3) {
                    out += &format!("\n  WARNING: STAGNATION \u{2014} {} consecutive failed experiments\n", n);
                    out += "    Suggested actions:\n";
                    out += "    1. Review recent experiment hypotheses \u{2014} are assumptions valid?\n";
                    out += "    2. Check if constraints have changed since experiments were designed\n";
                    out += "    3. Consider pivoting to a different approach within this phase\n";
                }

                // TaskCreate format for top recommended action
                if let Some(top) = next.first() {
                    if let Ok(exps) = store.list_experiments(Some(top.id)) {
                        let pending: Vec<_> = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).collect();
                        if let Some(exp) = pending.first() {
                            out += &format!("\n## Top Recommended Action:\n");
                            out += &format!("  -> TaskCreate: subject=\"{} Exp #{}: {}\" description=\"Phase #{} ({}), impact:{}\"\n",
                                project, exp.id, exp.name, top.id, top.name, top.impact);
                        }
                    }
                }

                out += "\n## ACTION: Execute the top phase.";
            }
        } else {
            out += &format!("Project not found: {}", project);
        }
    }
    out
}

pub fn tool_scaffold(store: &SqliteStore, project: &str, phase_id: i64) -> String {
    let proj = match store.list_projects().ok().and_then(|ps| ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))) {
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
    let open_hyps: Vec<_> = store.list_hypotheses(Some(phase_id)).map(|hs| {
        hs.into_iter().filter(|h| h.status == HypothesisStatus::Proposed || h.status == HypothesisStatus::Testing).collect()
    }).unwrap_or_default();
    text += &format!("Open hypotheses: {}\n\n", open_hyps.len());

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

    // Pending experiments as TaskCreate-ready items
    if !pending.is_empty() {
        text += &format!("--- {} Pending Experiments ---\n\n", pending.len());
        for (i, e) in pending.iter().enumerate() {
            text += &format!("TASK {}: Exp #{} \u{2014} {}\n", i + 1, e.id, e.name);
            text += &format!("  Status: pending | Phase: #{} ({})\n", phase.id, phase.name);
            if let Some(notes) = &e.notes {
                text += &format!("  Notes: {}\n", &notes[..notes.len().min(200)]);
            }
            // TaskCreate-ready format
            let desc = format!("Phase #{} ({}). {}", phase.id, phase.name,
                e.notes.as_deref().unwrap_or("Execute this experiment and record findings."));
            text += &format!("  -> TaskCreate: subject=\"{} Exp #{}: {}\" description=\"{}\"\n\n",
                project, e.id, e.name, &desc[..desc.len().min(200)]);
        }
    }

    // Active constraints (#13)
    if let Ok(constraints) = store.list_constraints(proj.id) {
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

    // Active principles that apply
    if let Ok(principles) = store.list_principles(proj.id) {
        let active: Vec<_> = principles.iter()
            .filter(|p| p.status == crate::store::PrincipleStatus::Active)
            .collect();
        if !active.is_empty() {
            text += "--- Active Principles ---\n\n";
            for p in &active {
                let scope = match p.scope {
                    crate::store::PrincipleScope::Universal => "universal",
                    crate::store::PrincipleScope::Project => "project",
                    crate::store::PrincipleScope::Phase => "phase",
                };
                let enforcement = p.enforcement_level.as_deref().unwrap_or("advisory");
                let t = if p.text.len() > 100 { &p.text[..100] } else { &p.text };
                text += &format!("  P#{} [{}|{}]: {}\n", p.id, scope, enforcement, t);
            }
            text += "\n";
        }
    }

    text
}

pub fn tool_session_init(store: &SqliteStore) -> String {
    let mut out = String::new();
    let mut task_num = 0usize;
    let mut stale_hyps: Vec<String> = Vec::new();
    let mut orphaned_findings: Vec<String> = Vec::new();

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
                        let pass_count = exps.iter().filter(|e| e.status == ExperimentStatus::Pass).count();
                        let fail_count = exps.iter().filter(|e| e.status == ExperimentStatus::Fail).count();

                        let status_str = if phase.status == PhaseStatus::InProgress { "IN-PROGRESS" }
                            else if phase.status == PhaseStatus::Paused { "PAUSED" }
                            else { "NEXT" };

                        let proj_label = if let Some(pid) = proj.parent_id {
                            if let Ok(all_projs) = store.list_projects() {
                                all_projs.iter().find(|p| p.id == pid)
                                    .map(|p| format!("{}/{}", p.name, proj.name))
                                    .unwrap_or_else(|| proj.name.clone())
                            } else { proj.name.clone() }
                        } else { proj.name.clone() };
                        out += &format!("## [{}] Phase #{} [impact:{}] {}\n", proj_label, phase.id, phase.impact, phase.name);
                        out += &format!("  Status: {} | Experiments: {} pending, {} pass, {} fail\n\n",
                            status_str, pending.len(), pass_count, fail_count);

                        if pending.is_empty() {
                            out += &format!("  -> This phase has no pending experiments. Use pm_scaffold to create tasks.\n\n");
                        } else {
                            for exp in pending.iter().take(5) {
                                task_num += 1;
                                out += &format!("  TASK {}: Exp #{} \u{2014} {}\n", task_num, exp.id, exp.name);
                                out += &format!("    Status: pending | Phase: #{} ({})\n", phase.id, phase.name);
                                if let Some(notes) = &exp.notes {
                                    out += &format!("    {}\n", &notes[..notes.len().min(150)]);
                                }
                                // TaskCreate-ready format
                                let desc = format!("Phase #{} ({}). {}",
                                    phase.id, phase.name,
                                    exp.notes.as_deref().unwrap_or("Execute experiment and record findings."));
                                out += &format!("    -> TaskCreate: subject=\"{} Exp #{}: {}\" description=\"{}\"\n\n",
                                    proj.name, exp.id, exp.name, &desc[..desc.len().min(200)]);
                            }
                        }
                    }

                    // Check for stale hypotheses (proposed > 7 days without testing)
                    if let Ok(hyps) = store.list_hypotheses(Some(phase.id)) {
                        for h in &hyps {
                            if h.status == HypothesisStatus::Proposed && is_stale(&h.created_at, 7) {
                                stale_hyps.push(format!("  H#{}: {} (proposed {} days ago, phase #{})",
                                    h.id,
                                    if h.text.len() > 80 { &h.text[..80] } else { &h.text },
                                    Utc::now().naive_utc().signed_duration_since(h.created_at).num_days(),
                                    phase.id));
                            }
                        }
                    }
                }

                // Check for orphaned findings (no edges)
                if let Ok(orphan_ids) = store.get_orphaned_nodes("finding", proj.id) {
                    for fid in orphan_ids.iter().take(5) {
                        if let Ok(f) = store.get_finding(*fid) {
                            orphaned_findings.push(format!("  F#{}: {}",
                                f.id,
                                if f.text.len() > 80 { &f.text[..80] } else { &f.text }));
                        }
                    }
                }
            }
        }
    }

    // Append cleanup tasks
    if !stale_hyps.is_empty() {
        out += "--- Stale Hypotheses (proposed >7 days, untested) ---\n\n";
        for h in &stale_hyps {
            out += h;
            out += "\n";
        }
        out += "\n  -> Review these: test, refine, or close them.\n\n";
    }

    if !orphaned_findings.is_empty() {
        out += "--- Orphaned Findings (no KG edges) ---\n\n";
        for f in &orphaned_findings {
            out += f;
            out += "\n";
        }
        out += "\n  -> Link these to experiments/hypotheses with pm_add_edge.\n\n";
    }

    if out.is_empty() {
        "No pending tasks in actionable phases.".to_string()
    } else {
        format!("=== Session Init: Actionable Tasks ===\n\n{}Create these as task tracker items and work through them.", out)
    }
}
