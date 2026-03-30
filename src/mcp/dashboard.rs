//! MCP tool implementations for dashboard/session/scaffold operations.
//!
//! Contains: pm_dashboard, pm_next, pm_session_init, pm_scaffold
//! Sprint 4: DAG→TaskTracker scaffold (#16) — structured, actionable output
//! with TaskCreate guidance and stale hypothesis / orphaned finding detection.

use crate::util::truncate_safe;
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

/// Build a knowledge briefing for the active phase of a project.
/// Surfaces findings, constraints, untested hypotheses, and contradictions.
fn build_knowledge_briefing(store: &SqliteStore, proj: &crate::store::Project) -> String {
    let dag = DagEngine::new(store, proj.id);
    let phases = match dag.next_phases() {
        Ok(p) => p,
        Err(_) => return String::new(),
    };

    // Find the active phase (in-progress with highest impact, or first next)
    let active_phase = match phases.iter().find(|p| p.status == PhaseStatus::InProgress) {
        Some(p) => p.clone(),
        None => match phases.first() {
            Some(p) => p.clone(),
            None => return String::new(),
        },
    };

    let phase_ref = active_phase.project_seq
        .map(|s| format!("#{}", s))
        .unwrap_or_else(|| format!("#{}", active_phase.id));

    let mut out = String::from("\n=== Knowledge Briefing ===\n\n");
    out += &format!("## Active Phase: {} {}\n\n", phase_ref, active_phase.name);

    // (a) Top findings from the active phase -- get experiments, collect findings, sort by recency
    let mut phase_findings: Vec<crate::store::Finding> = Vec::new();
    if let Ok(exps) = store.list_experiments(Some(active_phase.id)) {
        for exp in &exps {
            if let Ok(fs) = store.list_findings(Some(exp.id)) {
                phase_findings.extend(fs);
            }
        }
    }
    phase_findings.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let top_findings: Vec<_> = phase_findings.iter().take(5).collect();
    if !top_findings.is_empty() {
        out += &format!("### Recent Findings (Phase {}):\n", phase_ref);
        for f in &top_findings {
            let fref = f.project_seq.map(|s| format!("F#{}", s)).unwrap_or_else(|| format!("F#{}", f.id));
            let excerpt = truncate_safe(&f.text, 150);
            out += &format!("  {}: {}\n", fref, excerpt);
        }
        out += "\n";
    }

    // (b) Active constraints -- all constraints for the project with severity
    if let Ok(constraints) = store.list_constraints(proj.id) {
        if !constraints.is_empty() {
            out += "### Active Constraints:\n";
            for c in &constraints {
                let sev = c.severity.as_deref().unwrap_or("hard");
                let cref = c.project_seq.map(|s| format!("C#{}", s)).unwrap_or_else(|| format!("C#{}", c.id));
                let excerpt = truncate_safe(&c.text, 150);
                out += &format!("  {} [{}]: {}\n", cref, sev, excerpt);
            }
            out += "\n";
        }
    }

    // (c) Untested hypotheses -- Proposed status, from all phases of the project
    let mut proposed_hyps: Vec<crate::store::Hypothesis> = Vec::new();
    if let Ok(all_phases) = store.list_phases(proj.id) {
        for ph in &all_phases {
            if let Ok(hyps) = store.list_hypotheses(Some(ph.id)) {
                for h in hyps {
                    if h.status == HypothesisStatus::Proposed {
                        proposed_hyps.push(h);
                    }
                }
            }
        }
    }
    if !proposed_hyps.is_empty() {
        out += "### Untested Hypotheses:\n";
        for h in &proposed_hyps {
            let href = h.project_seq.map(|s| format!("H#{}", s)).unwrap_or_else(|| format!("H#{}", h.id));
            let excerpt = truncate_safe(&h.text, 100);
            out += &format!("  {} [proposed]: {}\n", href, excerpt);
        }
        out += "\n";
    }

    // (d) Contradictions in the phase neighborhood
    let kg = crate::kg::KgEngine::new(store);
    let contradictions = kg.find_contradictions(&phase_findings).unwrap_or_default();
    if !contradictions.is_empty() {
        out += "### Contradictions in Neighborhood:\n";
        for (f1, f2) in contradictions.iter().take(3) {
            let t1 = truncate_safe(&f1.text, 60);
            let t2 = truncate_safe(&f2.text, 60);
            let f1ref = f1.project_seq.map(|s| format!("F#{}", s)).unwrap_or_else(|| format!("F#{}", f1.id));
            let f2ref = f2.project_seq.map(|s| format!("F#{}", s)).unwrap_or_else(|| format!("F#{}", f2.id));
            out += &format!("  {} vs {}: \"{}\" <-> \"{}\"\n", f1ref, f2ref, t1, t2);
        }
        out += "\n";
    }

    out
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
                        let pref = top.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", top.id));
                        out += &format!("  [{}] {} {} [impact:{}] {}\n", parent.name, s, pref, top.impact, top.name);
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
                        let pref = top.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", top.id));
                        out += &format!("  [{}] {} {} [impact:{}] {}\n", parent.name, s, pref, top.impact, top.name);
                    }
                }
                // Subproject phases
                for sub in &subs {
                    let dag = DagEngine::new(store, sub.id);
                    if let Ok(next) = dag.next_phases() {
                        if let Some(top) = next.first() {
                            let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                            let pref = top.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", top.id));
                        out += &format!("  [{}/{}] {} {} [impact:{}] {}\n", parent.name, sub.name, s, pref, top.impact, top.name);
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
                    let pref = phase.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", phase.id));
                    out += &format!("  {} {} [impact:{}] {}\n", s, pref, phase.impact, phase.name);

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

    let pref = phase.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", phase.id));
    let mut text = format!("=== Phase {} ({}) ===\n\n", pref, phase.name);

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
            let eref = e.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", e.id));
            text += &format!("TASK {}: Exp {} \u{2014} {}\n", i + 1, eref, e.name);
            text += &format!("  Status: pending | Phase: {} ({})\n", pref, phase.name);
            if let Some(notes) = &e.notes {
                text += &format!("  Notes: {}\n", truncate_safe(&notes, 200));
            }
            // TaskCreate-ready format
            let desc = format!("Phase #{} ({}). {}", phase.id, phase.name,
                e.notes.as_deref().unwrap_or("Execute this experiment and record findings."));
            text += &format!("  -> TaskCreate: subject=\"{} Exp #{}: {}\" description=\"{}\"\n\n",
                project, e.id, e.name, truncate_safe(&desc, 200));
        }
    }

    // Active constraints (#13)
    if let Ok(constraints) = store.list_constraints(proj.id) {
        if !constraints.is_empty() {
            text += "--- Active Constraints ---\n\n";
            for c in &constraints {
                let sev = c.severity.as_deref().unwrap_or("hard");
                let src = c.source.as_deref().unwrap_or("unknown");
                let t = truncate_safe(&c.text, 80);
                let cref = c.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", c.id));
                text += &format!("  C{} [{}]: {} (source: {})\n", cref, sev, t, src);
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
                let t = truncate_safe(&p.text, 100);
                let prref = p.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", p.id));
                text += &format!("  P{} [{}|{}]: {}\n", prref, scope, enforcement, t);
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
                        let pref = phase.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", phase.id));
                        out += &format!("## [{}] Phase {} [impact:{}] {}\n", proj_label, pref, phase.impact, phase.name);
                        out += &format!("  Status: {} | Experiments: {} pending, {} pass, {} fail\n\n",
                            status_str, pending.len(), pass_count, fail_count);

                        if pending.is_empty() {
                            out += &format!("  -> This phase has no pending experiments. Use pm_scaffold to create tasks.\n\n");
                        } else {
                            for exp in pending.iter().take(5) {
                                task_num += 1;
                                let eref = exp.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", exp.id));
                                out += &format!("  TASK {}: Exp {} \u{2014} {}\n", task_num, eref, exp.name);
                                let pref2 = phase.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", phase.id));
                                out += &format!("    Status: pending | Phase: {} ({})\n", pref2, phase.name);
                                if let Some(notes) = &exp.notes {
                                    out += &format!("    {}\n", truncate_safe(&notes, 150));
                                }
                                // TaskCreate-ready format
                                let desc = format!("Phase #{} ({}). {}",
                                    phase.id, phase.name,
                                    exp.notes.as_deref().unwrap_or("Execute experiment and record findings."));
                                out += &format!("    -> TaskCreate: subject=\"{} Exp #{}: {}\" description=\"{}\"\n\n",
                                    proj.name, exp.id, exp.name, truncate_safe(&desc, 200));
                            }
                        }
                    }

                    // Check for stale hypotheses (proposed > 7 days without testing)
                    if let Ok(hyps) = store.list_hypotheses(Some(phase.id)) {
                        for h in &hyps {
                            if h.status == HypothesisStatus::Proposed && is_stale(&h.created_at, 7) {
                                stale_hyps.push(format!("  H#{}: {} (proposed {} days ago, phase #{})",
                                    h.id,
                                    truncate_safe(&h.text, 80),
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
                                truncate_safe(&f.text, 80)));
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

    // Append knowledge briefing for each active project
    let mut briefing = String::new();
    if let Ok(projects2) = store.list_projects() {
        for proj in &projects2 {
            if proj.status != crate::store::ProjectStatus::Active { continue; }
            let b = build_knowledge_briefing(store, proj);
            if !b.is_empty() {
                briefing += &b;
            }
        }
    }

    if out.is_empty() && briefing.is_empty() {
        "No pending tasks in actionable phases.".to_string()
    } else {
        let mut result = format!("=== Session Init: Actionable Tasks ===\n\n{}Create these as task tracker items and work through them.", out);
        if !briefing.is_empty() {
            result += &briefing;
        }
        result
    }
}

pub fn tool_project_create(store: &SqliteStore, name: &str, alias: Option<&str>, parent: Option<&str>) -> String {
    if name.is_empty() {
        return "Error: project name is required.".to_string();
    }

    let parent_id = if let Some(parent_name) = parent {
        match store.list_projects() {
            Ok(projects) => {
                match projects.iter().find(|p| p.name == parent_name || p.alias.as_deref() == Some(parent_name)) {
                    Some(p) => Some(p.id),
                    None => return format!("Error: parent project {} not found.", parent_name),
                }
            }
            Err(e) => return format!("Error listing projects: {}", e),
        }
    } else {
        None
    };

    match store.create_project(name, alias, parent_id) {
        Ok(proj) => {
            if let Some(pname) = parent {
                format!("Subproject #{} {} created under parent {}.", proj.id, proj.name, pname)
            } else {
                format!("Project #{} {} created.", proj.id, proj.name)
            }
        }
        Err(e) => format!("Error creating project: {}", e),
    }
}

pub fn tool_project_list(store: &SqliteStore) -> String {
    let projects = match store.list_projects() {
        Ok(p) => p,
        Err(e) => return format!("Error: {}", e),
    };

    let mut out = String::from("Projects:\n");

    // Separate into top-level and children
    let top_level: Vec<_> = projects.iter().filter(|p| p.parent_id.is_none()).collect();
    let children: Vec<_> = projects.iter().filter(|p| p.parent_id.is_some()).collect();

    for proj in &top_level {
        let subs: Vec<_> = children.iter().filter(|c| c.parent_id == Some(proj.id)).collect();
        let counts = node_counts_for_project(store, proj.id);
        let kind = if subs.is_empty() { "standalone" } else { "parent" };
        out += &format!("  #{} {} ({})  {}\n", proj.id, proj.name, kind, format_counts(&counts));

        for sub in &subs {
            let sub_counts = node_counts_for_project(store, sub.id);
            let alias_str = sub.alias.as_ref().map(|a| format!(" [{}]", a)).unwrap_or_default();
            out += &format!("    └─ #{} {}{}\n", sub.id, sub.name, alias_str);
            out += &format!("       {}\n", format_counts(&sub_counts));
        }
    }

    out
}

struct NodeCountsInternal {
    phases: usize,
    experiments: usize,
    findings: usize,
    decisions: usize,
    hypotheses: usize,
    literature: usize,
    principles: usize,
    constraints: usize,
}

fn node_counts_for_project(store: &SqliteStore, project_id: i64) -> NodeCountsInternal {
    let phases = store.list_phases(project_id).map(|v| v.len()).unwrap_or(0);
    let mut experiments = 0usize;
    let mut findings = 0usize;
    if let Ok(ph_list) = store.list_phases(project_id) {
        for ph in &ph_list {
            if let Ok(exps) = store.list_experiments(Some(ph.id)) {
                experiments += exps.len();
                for exp in &exps {
                    findings += store.list_findings(Some(exp.id)).map(|f| f.len()).unwrap_or(0);
                }
            }
        }
    }
    let decisions = store.list_decisions(project_id).map(|v| v.len()).unwrap_or(0);
    let mut hypotheses = 0usize;
    if let Ok(ph_list2) = store.list_phases(project_id) {
        for ph in &ph_list2 {
            hypotheses += store.list_hypotheses(Some(ph.id)).map(|v| v.len()).unwrap_or(0);
        }
    }
    let literature = store.list_literature(project_id).map(|v| v.len()).unwrap_or(0);
    let principles = store.list_principles(project_id).map(|v| v.len()).unwrap_or(0);
    let constraints = store.list_constraints(project_id).map(|v| v.len()).unwrap_or(0);

    NodeCountsInternal { phases, experiments, findings, decisions, hypotheses, literature, principles, constraints }
}

fn format_counts(c: &NodeCountsInternal) -> String {
    let mut parts = Vec::new();
    if c.phases > 0 { parts.push(format!("Phases: {}", c.phases)); }
    if c.experiments > 0 { parts.push(format!("Experiments: {}", c.experiments)); }
    if c.findings > 0 { parts.push(format!("Findings: {}", c.findings)); }
    if c.decisions > 0 { parts.push(format!("Decisions: {}", c.decisions)); }
    if c.literature > 0 { parts.push(format!("Literature: {}", c.literature)); }
    if c.principles > 0 { parts.push(format!("Principles: {}", c.principles)); }
    if c.constraints > 0 { parts.push(format!("Constraints: {}", c.constraints)); }
    if parts.is_empty() { return "(empty)".to_string(); }
    parts.join(" | ")
}

pub fn tool_project_set_status(store: &SqliteStore, name: &str, active: bool) -> String {
    if name.is_empty() {
        return "Error: project name is required".to_string();
    }
    // Resolve project by name or alias
    let project = match store.list_projects() {
        Ok(projects) => projects.into_iter().find(|p| p.name == name || p.alias.as_deref() == Some(name)),
        Err(e) => return format!("Error listing projects: {}", e),
    };
    match project {
        Some(p) => {
            let status = if active {
                crate::store::ProjectStatus::Active
            } else {
                crate::store::ProjectStatus::Archived
            };
            match store.update_project_status(p.id, status) {
                Ok(()) => {
                    let action = if active { "activated" } else { "deactivated" };
                    format!("Project '{}' (#{}) {}.", p.name, p.id, action)
                }
                Err(e) => format!("Error updating project status: {}", e),
            }
        }
        None => format!("Project '{}' not found. Use pm_project_list to see available projects.", name),
    }
}


// --- Temporal Awareness Tools (Feature 5) ---

pub fn tool_session_start(store: &SqliteStore, project: Option<&str>) -> String {
    let project_id = if let Some(name) = project {
        match store.list_projects() {
            Ok(projects) => projects.iter()
                .find(|p| p.name == name || p.alias.as_deref() == Some(name))
                .map(|p| p.id),
            Err(e) => return format!("Error: {}", e),
        }
    } else {
        None
    };

    match store.create_session(project_id) {
        Ok(session) => {
            let proj_str = project_id.map(|_| format!(" (project: {})", project.unwrap_or("?"))).unwrap_or_default();
            format!("Session #{} started at {}{}", session.id, session.started_at.format("%Y-%m-%d %H:%M:%S"), proj_str)
        }
        Err(e) => format!("Error creating session: {}", e),
    }
}

pub fn tool_session_end(store: &SqliteStore, summary: Option<&str>) -> String {
    match store.get_current_session() {
        Ok(Some(session)) => {
            match store.end_session(session.id, summary) {
                Ok(()) => {
                    let sum_str = summary.map(|s| format!("
Summary: {}", s)).unwrap_or_default();
                    format!("Session #{} ended.{}", session.id, sum_str)
                }
                Err(e) => format!("Error ending session: {}", e),
            }
        }
        Ok(None) => "No active session to end. Start one with pm_session_start.".to_string(),
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_since(store: &SqliteStore, since: Option<&str>, session_id: Option<i64>) -> String {
    let timestamp = if let Some(sid) = session_id {
        // Look up session start time
        match store.list_sessions(None) {
            Ok(sessions) => {
                match sessions.iter().find(|s| s.id == sid) {
                    Some(session) => session.started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    None => return format!("Session #{} not found.", sid),
                }
            }
            Err(e) => return format!("Error: {}", e),
        }
    } else if let Some(s) = since {
        // Pad date-only to datetime
        if s.len() == 10 { format!("{} 00:00:00", s) } else { s.to_string() }
    } else {
        return "Error: provide either 'since' (date) or 'session_id'.".to_string();
    };

    match store.nodes_since(&timestamp) {
        Ok(delta) => {
            let mut out = format!("=== Changes since {} ===\n\n", delta.since);

            let mut total = 0;
            let sections: Vec<(&str, usize)> = vec![
                ("Phases", delta.phases.len()),
                ("Experiments", delta.experiments.len()),
                ("Findings", delta.findings.len()),
                ("Decisions", delta.decisions.len()),
                ("Hypotheses", delta.hypotheses.len()),
                ("Research", delta.research.len()),
                ("Literature", delta.literature.len()),
                ("Principles", delta.principles.len()),
                ("Constraints", delta.constraints.len()),
                ("Feedback", delta.feedback.len()),
            ];

            for (name, count) in &sections {
                if *count > 0 {
                    total += count;
                    out += &format!("  {}: {} new/modified\n", name, count);
                }
            }

            if total == 0 {
                out += "  No changes found.\n";
            } else {
                out += &format!("\n  Total: {} nodes changed\n", total);
            }

            // Show details for small deltas
            if total <= 20 {
                if !delta.findings.is_empty() {
                    out += "\n--- Findings ---\n";
                    for f in &delta.findings {
                        let t = truncate_safe(&f.text, 80);
                        out += &format!("  F#{}: {}\n", f.id, t);
                    }
                }
                if !delta.decisions.is_empty() {
                    out += "\n--- Decisions ---\n";
                    for d in &delta.decisions {
                        let t = truncate_safe(&d.what, 80);
                        out += &format!("  D#{}: {}\n", d.id, t);
                    }
                }
                if !delta.experiments.is_empty() {
                    out += "\n--- Experiments ---\n";
                    for e in &delta.experiments {
                        out += &format!("  E#{}: {} ({:?})\n", e.id, e.name, e.status);
                    }
                }
            }

            out
        }
        Err(e) => format!("Error: {}", e),
    }
}

// --- Session Context Tool (F3: graph-topology-based context retrieval) ---

pub fn tool_session_context(store: &SqliteStore, project: &str) -> String {
    // 1. Resolve project by name or alias
    let proj = match store.list_projects().ok().and_then(|ps| {
        ps.into_iter().find(|p| p.name == project || p.alias.as_deref() == Some(project))
    }) {
        Some(p) => p,
        None => return format!("Project not found: {}", project),
    };

    // 2. Find the in-progress phase with highest impact
    let dag = DagEngine::new(store, proj.id);
    let phases = match dag.next_phases() {
        Ok(p) => p,
        Err(e) => return format!("Error loading phases: {}", e),
    };

    let active_phase = match phases.iter().find(|p| p.status == PhaseStatus::InProgress) {
        Some(p) => p.clone(),
        None => match phases.first() {
            Some(p) => p.clone(),
            None => return format!("No actionable phases found for project: {}", project),
        },
    };

    let phase_ref = active_phase.project_seq
        .map(|s| format!("#{}", s))
        .unwrap_or_else(|| format!("#{}", active_phase.id));
    let phase_status = match active_phase.status {
        PhaseStatus::InProgress => "IN-PROGRESS",
        PhaseStatus::Pending => "PENDING",
        PhaseStatus::Paused => "PAUSED",
        _ => "OTHER",
    };

    // 3. Call phase_subgraph to get all reachable nodes within 3 hops
    let subgraph = crate::kg::traversal::phase_subgraph(store, active_phase.id, 3);

    // 4. Group subgraph nodes by type
    use crate::store::NodeType;
    let mut finding_ids: Vec<i64> = Vec::new();
    let mut hypothesis_ids: Vec<i64> = Vec::new();
    let mut decision_ids: Vec<i64> = Vec::new();
    let mut literature_ids: Vec<i64> = Vec::new();
    let mut experiment_ids: Vec<i64> = Vec::new();

    for node in &subgraph.nodes {
        match node.node_type {
            NodeType::Finding => finding_ids.push(node.node_id),
            NodeType::Hypothesis => hypothesis_ids.push(node.node_id),
            NodeType::Decision => decision_ids.push(node.node_id),
            NodeType::Literature => literature_ids.push(node.node_id),
            NodeType::Experiment => experiment_ids.push(node.node_id),
            _ => {}
        }
    }

    // Build output
    let mut out = format!("=== Session Context: {} \u{2014} Phase {}: {} ===\n\n", project, phase_ref, active_phase.name);

    // ## Active Phase
    out += "## Active Phase\n";
    out += &format!("{} [{}] [impact:{}]\n", active_phase.name, phase_status, active_phase.impact);
    if let Some(ref desc) = active_phase.description {
        out += &format!("  {}\n", desc);
    }
    // Experiment rollup
    if let Ok(exps) = store.list_experiments(Some(active_phase.id)) {
        let pending = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).count();
        let pass = exps.iter().filter(|e| e.status == ExperimentStatus::Pass).count();
        let fail = exps.iter().filter(|e| e.status == ExperimentStatus::Fail).count();
        out += &format!("  Experiments: {} total ({} pending, {} pass, {} fail)\n", exps.len(), pending, pass, fail);
    }
    out += "\n";

    // 5. Findings: sort by created_at descending, take top 5
    if !finding_ids.is_empty() {
        let mut findings: Vec<crate::store::Finding> = finding_ids.iter()
            .filter_map(|id| store.get_finding(*id).ok())
            .collect();
        findings.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let top_findings: Vec<_> = findings.iter().take(5).collect();
        if !top_findings.is_empty() {
            out += &format!("## Recent Findings (top {})\n", top_findings.len());
            for f in &top_findings {
                let fref = f.project_seq.map(|s| format!("F#{}", s)).unwrap_or_else(|| format!("F#{}", f.id));
                let excerpt = { let t = truncate_safe(&f.text, 120); if f.text.len() > t.len() { format!("{}...", t) } else { f.text.clone() } };
                out += &format!("  {}: {}\n", fref, excerpt);
            }
            out += "\n";
        }
    }

    // 6. Hypotheses: filter to proposed/testing
    if !hypothesis_ids.is_empty() {
        let active_hyps: Vec<crate::store::Hypothesis> = hypothesis_ids.iter()
            .filter_map(|id| store.get_hypothesis(*id).ok())
            .filter(|h| h.status == HypothesisStatus::Proposed || h.status == HypothesisStatus::Testing)
            .collect();
        if !active_hyps.is_empty() {
            out += "## Active Hypotheses\n";
            for h in &active_hyps {
                let href = h.project_seq.map(|s| format!("H#{}", s)).unwrap_or_else(|| format!("H#{}", h.id));
                let status_str = match h.status {
                    HypothesisStatus::Proposed => "proposed",
                    HypothesisStatus::Testing => "testing",
                    _ => "other",
                };
                let excerpt = { let t = truncate_safe(&h.text, 120); if h.text.len() > t.len() { format!("{}...", t) } else { h.text.clone() } };
                out += &format!("  {} [{}]: {}\n", href, status_str, excerpt);
            }
            out += "\n";
        }
    }

    // 7. Decisions: take all
    if !decision_ids.is_empty() {
        let decisions: Vec<crate::store::Decision> = decision_ids.iter()
            .filter_map(|id| store.get_decision(*id).ok())
            .collect();
        if !decisions.is_empty() {
            out += "## Key Decisions\n";
            for d in &decisions {
                let dref = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D#{}", d.id));
                let excerpt = { let t = truncate_safe(&d.what, 120); if d.what.len() > t.len() { format!("{}...", t) } else { d.what.clone() } };
                out += &format!("  {}: {}\n", dref, excerpt);
            }
            out += "\n";
        }
    }

    // 8. Literature: take top 3
    if !literature_ids.is_empty() {
        let lit: Vec<crate::store::LiteratureEntry> = literature_ids.iter()
            .filter_map(|id| store.get_literature(*id).ok())
            .collect();
        let top_lit: Vec<_> = lit.iter().take(3).collect();
        if !top_lit.is_empty() {
            out += "## Relevant Literature\n";
            for l in &top_lit {
                let lref = l.project_seq.map(|s| format!("L#{}", s)).unwrap_or_else(|| format!("L#{}", l.id));
                out += &format!("  {}: {}\n", lref, l.title);
            }
            out += "\n";
        }
    }

    // 9. Suggested next actions based on phase status
    out += "## Suggested Next Actions\n";
    if let Ok(exps) = store.list_experiments(Some(active_phase.id)) {
        let pending: Vec<_> = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).collect();
        if !pending.is_empty() {
            out += &format!("  - {} pending experiments to execute\n", pending.len());
            if let Some(top) = pending.first() {
                let eref = top.project_seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E#{}", top.id));
                out += &format!("  - Next: {} \u{2014} {}\n", eref, top.name);
            }
        } else if active_phase.status == PhaseStatus::InProgress {
            out += "  - All experiments resolved \u{2014} review results and consider completing this phase\n";
        }
    }

    // Check for stale hypotheses in the subgraph
    let stale_hyps: Vec<_> = hypothesis_ids.iter()
        .filter_map(|id| store.get_hypothesis(*id).ok())
        .filter(|h| h.status == HypothesisStatus::Proposed && is_stale(&h.created_at, 7))
        .collect();
    if !stale_hyps.is_empty() {
        out += &format!("  - {} stale hypothesis(es) (proposed >7 days) \u{2014} test, refine, or close\n", stale_hyps.len());
    }

    // Subgraph stats
    out += &format!("\n[Graph: {} nodes, {} edges within 3 hops of phase]\n", subgraph.nodes.len(), subgraph.edges.len());

    // Append knowledge briefing
    let briefing = build_knowledge_briefing(store, &proj);
    if !briefing.is_empty() {
        out += &briefing;
    }

    out
}
