use warp::Filter;
use crate::store::sqlite::SqliteStore;
use crate::store::Store;
use crate::dag::DagEngine;
use std::sync::Mutex;
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
        .map(move |_id: i64| {
            let store = SqliteStore::new(&db3).unwrap();
            warp::reply::json(&store.list_findings(None).unwrap_or_default())
        });

    let db4 = db.clone();
    let edges = warp::path!("api" / "projects" / i64 / "edges")
        .map(move |_id: i64| {
            let store = SqliteStore::new(&db4).unwrap();
            let mut all_edges = Vec::new();
            if let Ok(findings) = store.list_findings(None) {
                for f in &findings {
                    if let Ok(e) = store.get_edges_from(crate::store::NodeType::Finding, f.id) {
                        all_edges.extend(e);
                    }
                }
            }
            warp::reply::json(&all_edges)
        });

    let db5 = db.clone();
    let experiments = warp::path!("api" / "projects" / i64 / "experiments")
        .map(move |_id: i64| {
            let store = SqliteStore::new(&db5).unwrap();
            warp::reply::json(&store.list_experiments(None).unwrap_or_default())
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

    let routes = index.or(projects).or(phases).or(findings).or(edges).or(experiments).or(dashboard);

    println!("PM dashboard at http://localhost:{}", port);
    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
}
