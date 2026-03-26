use warp::Filter;
use crate::store::sqlite::SqliteStore;
use crate::store::Store;
use crate::dag::DagEngine;
use std::sync::Arc;

pub async fn serve(db_path: &str, port: u16) {
    let db = Arc::new(db_path.to_string());

    let db1 = db.clone();
    let projects = warp::path!("api" / "projects")
        .map(move || {
            let store = SqliteStore::new(&db1).unwrap();
            warp::reply::json(&store.list_projects().unwrap_or_default())
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
            // Filter findings by project: project → phases → experiments → findings
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
            // Also get findings with no experiment (experiment_id = None won't be caught above)
            warp::reply::json(&project_findings)
        });

    let db4 = db.clone();
    let edges = warp::path!("api" / "projects" / i64 / "edges")
        .map(move |_project_id: i64| {
            let store = SqliteStore::new(&db4).unwrap();
            // Return ALL edges — the frontend filters by node membership
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
            // Also get research with no phase
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
        .map(move |project_id: i64| {
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

    let routes = index.or(projects).or(phases).or(findings).or(edges).or(experiments).or(decisions).or(research).or(principles).or(hypotheses).or(constraints).or(literature).or(feedback).or(dashboard);

    println!("PM dashboard at http://localhost:{}", port);
    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
}
