use warp::Filter;
use crate::store::sqlite::SqliteStore;
use crate::store::Store;
use crate::dag::DagEngine;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
struct NodeCounts {
    phases: usize,
    experiments: usize,
    findings: usize,
    decisions: usize,
    hypotheses: usize,
    literature: usize,
    principles: usize,
    constraints: usize,
}

#[derive(Serialize)]
struct ProjectTree {
    #[serde(flatten)]
    project: crate::store::Project,
    children: Vec<crate::store::Project>,
    node_counts: NodeCounts,
}

fn compute_node_counts(store: &SqliteStore, project_id: i64) -> NodeCounts {
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
    let hypotheses = store.list_hypotheses(None).map(|v| v.len()).unwrap_or(0);
    let literature = store.list_literature(project_id).map(|v| v.len()).unwrap_or(0);
    let principles = store.list_principles(project_id).map(|v| v.len()).unwrap_or(0);
    let constraints = store.list_constraints(project_id).map(|v| v.len()).unwrap_or(0);

    NodeCounts { phases, experiments, findings, decisions, hypotheses, literature, principles, constraints }
}

#[derive(Serialize)]
struct KgNode {
    node_type: String,
    node_id: i64,
    label: String,
    text: String,
    subproject_id: Option<i64>,
    project_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct KgResponse {
    nodes: Vec<KgNode>,
    edges: Vec<crate::store::Edge>,
    subprojects: Vec<SubprojectInfo>,
}

#[derive(Serialize)]
struct SubprojectInfo {
    id: i64,
    name: String,
    alias: Option<String>,
    depth: usize,
    parent_subproject_id: Option<i64>,
}

fn collect_kg_nodes(store: &SqliteStore, project_id: i64, subproject_id: Option<i64>, nodes: &mut Vec<KgNode>) {
    if let Ok(phases) = store.list_phases(project_id) {
        for p in &phases {
            let status_str = format!("{:?}", p.status);
            let extra = serde_json::json!({
                "impact": p.impact,
                "description": p.description,
                "goals": p.goals,
                "success_criteria": p.success_criteria,
                "depends_on": p.depends_on,
                "started_at": p.started_at.map(|t| t.to_string()),
                "completed_at": p.completed_at.map(|t| t.to_string()),
            });
            let ph_label = p.project_seq.map(|s| format!("Ph#{}", s)).unwrap_or_else(|| format!("Ph{}", p.id));
            nodes.push(KgNode {
                node_type: "phase".into(), node_id: p.id, label: ph_label,
                text: p.name.clone(), subproject_id, project_seq: p.project_seq, status: Some(status_str),
                extra: Some(extra),
            });

            if let Ok(exps) = store.list_experiments(Some(p.id)) {
                for e in &exps {
                    let status_str = format!("{:?}", e.status);
                    let extra = serde_json::json!({
                        "hypothesis": e.hypothesis,
                        "result": e.result,
                        "notes": e.notes,
                        "phase_id": p.id,
                    });
                    let e_label = e.project_seq.map(|s| format!("E#{}", s)).unwrap_or_else(|| format!("E{}", e.id));
                    nodes.push(KgNode {
                        node_type: "experiment".into(), node_id: e.id, label: e_label,
                        text: e.name.clone(), subproject_id, project_seq: e.project_seq, status: Some(status_str),
                        extra: Some(extra),
                    });

                    if let Ok(findings) = store.list_findings(Some(e.id)) {
                        for f in &findings {
                            let f_label = f.project_seq.map(|s| format!("F#{}", s)).unwrap_or_else(|| format!("F{}", f.id));
                            nodes.push(KgNode {
                                node_type: "finding".into(), node_id: f.id, label: f_label,
                                text: f.text.clone(), subproject_id, project_seq: f.project_seq, status: None,
                                extra: Some(serde_json::json!({"experiment_id": e.id})),
                            });
                        }
                    }
                }
            }

            if let Ok(hyps) = store.list_hypotheses(Some(p.id)) {
                for h in &hyps {
                    let status_str = format!("{:?}", h.status);
                    let extra = serde_json::json!({
                        "prediction": h.prediction,
                        "criteria": h.criteria,
                        "confidence": h.confidence,
                        "experiment_id": h.experiment_id,
                        "finding_id": h.finding_id,
                    });
                    let h_label = h.project_seq.map(|s| format!("H#{}", s)).unwrap_or_else(|| format!("H{}", h.id));
                    nodes.push(KgNode {
                        node_type: "hypothesis".into(), node_id: h.id, label: h_label,
                        text: h.text.clone(), subproject_id, project_seq: h.project_seq, status: Some(status_str),
                        extra: Some(extra),
                    });
                }
            }
        }
    }

    if let Ok(phases) = store.list_phases(project_id) {
        for p in &phases {
            if let Ok(items) = store.list_research(Some(p.id)) {
                for r in &items {
                    let extra = serde_json::json!({
                        "report": r.report,
                        "phase_id": p.id,
                    });
                    let r_label = r.project_seq.map(|s| format!("R#{}", s)).unwrap_or_else(|| format!("R{}", r.id));
                    nodes.push(KgNode {
                        node_type: "research".into(), node_id: r.id, label: r_label,
                        text: r.name.clone(), subproject_id, project_seq: r.project_seq, status: Some(format!("{:?}", r.status)),
                        extra: Some(extra),
                    });
                }
            }
        }
    }

    if let Ok(decs) = store.list_decisions(project_id) {
        for d in &decs {
            let extra = serde_json::json!({
                "why": d.why,
                "experiment_id": d.experiment_id,
            });
            let d_label = d.project_seq.map(|s| format!("D#{}", s)).unwrap_or_else(|| format!("D{}", d.id));
            nodes.push(KgNode {
                node_type: "decision".into(), node_id: d.id, label: d_label,
                text: d.what.clone(), subproject_id, project_seq: d.project_seq, status: None,
                extra: Some(extra),
            });
        }
    }

    if let Ok(prins) = store.list_principles(project_id) {
        for p in &prins {
            let extra = serde_json::json!({
                "scope": format!("{:?}", p.scope),
                "rationale": p.rationale,
                "enforcement_level": p.enforcement_level,
                "superseded_by": p.superseded_by,
            });
            let p_label = p.project_seq.map(|s| format!("P#{}", s)).unwrap_or_else(|| format!("P{}", p.id));
            nodes.push(KgNode {
                node_type: "principle".into(), node_id: p.id, label: p_label,
                text: p.text.clone(), subproject_id, project_seq: p.project_seq, status: Some(format!("{:?}", p.status)),
                extra: Some(extra),
            });
        }
    }

    if let Ok(cons) = store.list_constraints(project_id) {
        for c in &cons {
            let extra = serde_json::json!({
                "scope": format!("{:?}", c.scope),
                "severity": c.severity,
                "resource": c.resource,
                "measured_value": c.measured_value,
                "expires_at": c.expires_at,
                "source": c.source,
            });
            let c_label = c.project_seq.map(|s| format!("C#{}", s)).unwrap_or_else(|| format!("C{}", c.id));
            nodes.push(KgNode {
                node_type: "constraint".into(), node_id: c.id, label: c_label,
                text: c.text.clone(), subproject_id, project_seq: c.project_seq, status: None,
                extra: Some(extra),
            });
        }
    }

    if let Ok(lits) = store.list_literature(project_id) {
        for l in &lits {
            let extra = serde_json::json!({
                "authors": l.authors,
                "venue": l.venue,
                "year": l.year,
                "arxiv_id": l.arxiv_id,
                "url": l.url,
                "code_url": l.code_url,
                "summary": l.summary,
                "relevance": l.relevance,
                "key_findings": l.key_findings,
            });
            let l_label = l.project_seq.map(|s| format!("L#{}", s)).unwrap_or_else(|| format!("L{}", l.id));
            nodes.push(KgNode {
                node_type: "literature".into(), node_id: l.id, label: l_label,
                text: l.title.clone(), subproject_id, project_seq: l.project_seq, status: l.status.clone(),
                extra: Some(extra),
            });
        }
    }

    if let Ok(fbs) = store.list_feedback(project_id) {
        for f in &fbs {
            let fb_label = f.project_seq.map(|s| format!("Fb#{}", s)).unwrap_or_else(|| format!("Fb{}", f.id));
            nodes.push(KgNode {
                node_type: "feedback".into(), node_id: f.id, label: fb_label,
                text: f.text.clone(), subproject_id, project_seq: f.project_seq, status: None, extra: None,
            });
        }
    }
}


fn collect_subproject_tree(store: &SqliteStore, project_id: i64, depth: usize, parent_sub_id: Option<i64>, subprojects: &mut Vec<SubprojectInfo>, nodes: &mut Vec<KgNode>) {
    if let Ok(subs) = store.list_subprojects(project_id) {
        for sub in &subs {
            subprojects.push(SubprojectInfo {
                id: sub.id,
                name: sub.name.clone(),
                alias: sub.alias.clone(),
                depth,
                parent_subproject_id: parent_sub_id,
            });
            collect_kg_nodes(store, sub.id, Some(sub.id), nodes);
            // Recurse into grandchildren
            collect_subproject_tree(store, sub.id, depth + 1, Some(sub.id), subprojects, nodes);
        }
    }
}

pub async fn serve(db_path: &str, port: u16) {
    let db = Arc::new(db_path.to_string());

    let db1 = db.clone();
    let projects = warp::path!("api" / "projects")
        .and(warp::get())
        .map(move || {
            let store = SqliteStore::new(&db1).unwrap();
            let all = store.list_projects().unwrap_or_default();
            let trees: Vec<ProjectTree> = all.iter().map(|p| {
                let children = store.list_subprojects(p.id).unwrap_or_default();
                let counts = compute_node_counts(&store, p.id);
                ProjectTree { project: p.clone(), children, node_counts: counts }
            }).collect();
            warp::reply::json(&trees)
        });

    let db_kg = db.clone();
    let kg = warp::path!("api" / "projects" / i64 / "kg")
        .and(warp::get())
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db_kg).unwrap();
            let mut nodes = Vec::new();
            let mut subprojects = Vec::new();

            collect_kg_nodes(&store, project_id, None, &mut nodes);

            // Recursively collect all descendant subprojects and their nodes
            collect_subproject_tree(&store, project_id, 0, None, &mut subprojects, &mut nodes);

            let edges = store.list_all_edges().unwrap_or_default();
            let response = KgResponse { nodes, edges, subprojects };
            warp::reply::json(&response)
        });

    let db2 = db.clone();
    let phases = warp::path!("api" / "projects" / i64 / "phases")
        .map(move |id: i64| {
            let store = SqliteStore::new(&db2).unwrap();
            warp::reply::json(&store.list_phases(id).unwrap_or_default())
        });

    let db3 = db.clone();
    let findings = warp::path!("api" / "projects" / i64 / "findings")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db3).unwrap();
            let mut project_findings = Vec::new();
            if let Ok(phases) = store.list_phases(project_id) {
                for phase in &phases {
                    if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                        for exp in &exps {
                            if let Ok(findings) = store.list_findings(Some(exp.id)) {
                                project_findings.extend(findings);
                            }
                        }
                    }
                }
            }
            warp::reply::json(&project_findings)
        });

    let db4 = db.clone();
    let edges = warp::path!("api" / "projects" / i64 / "edges")
        .map(move |_project_id: i64| {
            let store = SqliteStore::new(&db4).unwrap();
            warp::reply::json(&store.list_all_edges().unwrap_or_default())
        });

    let db5 = db.clone();
    let experiments = warp::path!("api" / "projects" / i64 / "experiments")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db5).unwrap();
            let mut project_exps = Vec::new();
            if let Ok(phases) = store.list_phases(project_id) {
                for phase in &phases {
                    if let Ok(exps) = store.list_experiments(Some(phase.id)) {
                        project_exps.extend(exps);
                    }
                }
            }
            warp::reply::json(&project_exps)
        });

    let db7 = db.clone();
    let decisions = warp::path!("api" / "projects" / i64 / "decisions")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db7).unwrap();
            warp::reply::json(&store.list_decisions(project_id).unwrap_or_default())
        });

    let db_research = db.clone();
    let research = warp::path!("api" / "projects" / i64 / "research")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db_research).unwrap();
            let mut project_research = Vec::new();
            if let Ok(phases) = store.list_phases(project_id) {
                for phase in &phases {
                    if let Ok(items) = store.list_research(Some(phase.id)) {
                        project_research.extend(items);
                    }
                }
            }
            if let Ok(items) = store.list_research(None) {
                project_research.extend(items);
            }
            warp::reply::json(&project_research)
        });

    let db_principles = db.clone();
    let principles = warp::path!("api" / "projects" / i64 / "principles")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db_principles).unwrap();
            warp::reply::json(&store.list_principles(project_id).unwrap_or_default())
        });

    let db_hypotheses = db.clone();
    let hypotheses = warp::path!("api" / "projects" / i64 / "hypotheses")
        .map(move |_project_id: i64| {
            let store = SqliteStore::new(&db_hypotheses).unwrap();
            warp::reply::json(&store.list_hypotheses(None).unwrap_or_default())
        });

    let db_constraints = db.clone();
    let constraints = warp::path!("api" / "projects" / i64 / "constraints")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db_constraints).unwrap();
            warp::reply::json(&store.list_constraints(project_id).unwrap_or_default())
        });

    let db_literature = db.clone();
    let literature = warp::path!("api" / "projects" / i64 / "literature")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db_literature).unwrap();
            warp::reply::json(&store.list_literature(project_id).unwrap_or_default())
        });

    let db_feedback = db.clone();
    let feedback = warp::path!("api" / "projects" / i64 / "feedback")
        .map(move |project_id: i64| {
            let store = SqliteStore::new(&db_feedback).unwrap();
            warp::reply::json(&store.list_feedback(project_id).unwrap_or_default())
        });

    let db_toggle = db.clone();
    let toggle_status = warp::path!("api" / "projects" / i64 / "status")
        .and(warp::put())
        .and(warp::body::json())
        .map(move |id: i64, body: serde_json::Value| {
            let store = SqliteStore::new(&db_toggle).unwrap();
            let status_str = body.get("status").and_then(|s| s.as_str()).unwrap_or("active");
            let ps = match status_str {
                "archived" => crate::store::ProjectStatus::Archived,
                "paused" => crate::store::ProjectStatus::Paused,
                _ => crate::store::ProjectStatus::Active,
            };
            match store.update_project_status(id, ps) {
                Ok(()) => warp::reply::json(&serde_json::json!({"ok": true})),
                Err(e) => warp::reply::json(&serde_json::json!({"error": e.to_string()})),
            }
        });

    let db6 = db.clone();
    let dashboard = warp::path!("api" / "dashboard")
        .map(move || {
            let store = SqliteStore::new(&db6).unwrap();
            let mut text = String::from("Cross-Project Dashboard\n\n");
            if let Ok(projects) = store.list_projects() {
                for proj in &projects {
                    if proj.status != crate::store::ProjectStatus::Active { continue; }
                    let dag = DagEngine::new(&store, proj.id);
                    if let Ok(next) = dag.next_phases() {
                        if let Some(top) = next.first() {
                            text += &format!("[{}] #{} [impact:{}] {}\n", proj.name, top.id, top.impact, top.name);
                        }
                    }
                }
            }
            warp::reply::json(&serde_json::json!({"text": text}))
        });

    let index = warp::path::end()
        .map(|| warp::reply::html(include_str!("web/index.html")));

    let routes = index
        .or(kg)
        .or(projects)
        .or(phases)
        .or(findings)
        .or(edges)
        .or(experiments)
        .or(decisions)
        .or(research)
        .or(principles)
        .or(hypotheses)
        .or(constraints)
        .or(literature)
        .or(feedback)
        .or(dashboard)
        .or(toggle_status);

    println!("PM dashboard at http://localhost:{}", port);
    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
}
