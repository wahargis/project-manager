use super::*;
use super::sqlite::SqliteStore;

fn test_store() -> SqliteStore {
    SqliteStore::in_memory().unwrap()
}

// --- P1.1: Project CRUD ---

#[test]
fn create_project_returns_project_with_id() {
    let store = test_store();
    let p = store.create_project("volta-renaissance", Some("vr")).unwrap();
    assert!(p.id > 0);
    assert_eq!(p.name, "volta-renaissance");
    assert_eq!(p.alias, Some("vr".to_string()));
    assert_eq!(p.status, ProjectStatus::Active);
}

#[test]
fn get_project_by_id() {
    let store = test_store();
    let created = store.create_project("test", None).unwrap();
    let fetched = store.get_project(created.id).unwrap();
    assert_eq!(fetched.name, "test");
}

#[test]
fn list_projects_returns_all() {
    let store = test_store();
    store.create_project("a", None).unwrap();
    store.create_project("b", None).unwrap();
    let all = store.list_projects().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn update_project_status() {
    let store = test_store();
    let p = store.create_project("test", None).unwrap();
    store.update_project_status(p.id, ProjectStatus::Paused).unwrap();
    let fetched = store.get_project(p.id).unwrap();
    assert_eq!(fetched.status, ProjectStatus::Paused);
}

// --- P1.2: Phase CRUD ---

#[test]
fn create_phase_with_dependencies() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let p1 = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let p2 = store.create_phase(proj.id, "Phase 2", 30, &[p1.id]).unwrap();
    assert_eq!(p2.depends_on, vec![p1.id]);
    assert_eq!(p2.impact, 30);
}

#[test]
fn list_phases_for_project() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    store.create_phase(proj.id, "A", 10, &[]).unwrap();
    store.create_phase(proj.id, "B", 20, &[]).unwrap();
    let phases = store.list_phases(proj.id).unwrap();
    assert_eq!(phases.len(), 2);
}

// --- P1.3: Experiment CRUD ---

#[test]
fn create_experiment_with_phase() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test exp").unwrap();
    assert_eq!(exp.status, ExperimentStatus::Pending);
    assert_eq!(exp.phase_id, Some(phase.id));
}

#[test]
fn update_experiment_status_to_pass() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    store.update_experiment_status(exp.id, ExperimentStatus::Pass, Some("0/256 mismatches")).unwrap();
    let fetched = store.get_experiment(exp.id).unwrap();
    assert_eq!(fetched.status, ExperimentStatus::Pass);
    assert_eq!(fetched.result, Some("0/256 mismatches".to_string()));
}

// --- P1.4: Finding CRUD ---

#[test]
fn create_finding_linked_to_experiment() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let finding = store.create_finding(Some(exp.id), "Q4_K GEMV is compute-bound").unwrap();
    assert_eq!(finding.experiment_id, Some(exp.id));
    assert!(finding.text.contains("compute-bound"));
}

// --- P1.5: Edge CRUD (KG) ---

#[test]
fn create_edge_supports() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let f1 = store.create_finding(Some(exp.id), "finding 1").unwrap();
    let f2 = store.create_finding(Some(exp.id), "finding 2").unwrap();
    let edge = store.create_edge(
        NodeType::Finding, f1.id,
        NodeType::Finding, f2.id,
        EdgeType::Supports,
    ).unwrap();
    assert_eq!(edge.relation, EdgeType::Supports);
}

#[test]
fn get_edges_from_finding() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let f1 = store.create_finding(Some(exp.id), "source").unwrap();
    let f2 = store.create_finding(Some(exp.id), "target").unwrap();
    store.create_edge(NodeType::Finding, f1.id, NodeType::Finding, f2.id, EdgeType::Contradicts).unwrap();
    let edges = store.get_edges_from(NodeType::Finding, f1.id).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].relation, EdgeType::Contradicts);
}

// --- P1.6: Decision CRUD ---

#[test]
fn create_decision_with_rationale() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let dec = store.create_decision(Some(exp.id), "Use Rust", Some("Type safety for KG")).unwrap();
    assert_eq!(dec.what, "Use Rust");
    assert_eq!(dec.why, Some("Type safety for KG".to_string()));
}

// --- Additional tests for v3 completeness ---

#[test]
fn list_experiments_by_phase() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let p1 = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let p2 = store.create_phase(proj.id, "P2", 20, &[]).unwrap();
    store.create_experiment(Some(p1.id), "exp_p1").unwrap();
    store.create_experiment(Some(p2.id), "exp_p2a").unwrap();
    store.create_experiment(Some(p2.id), "exp_p2b").unwrap();
    
    let p1_exps = store.list_experiments(Some(p1.id)).unwrap();
    assert_eq!(p1_exps.len(), 1);
    let p2_exps = store.list_experiments(Some(p2.id)).unwrap();
    assert_eq!(p2_exps.len(), 2);
    let all_exps = store.list_experiments(None).unwrap();
    assert_eq!(all_exps.len(), 3);
}

#[test]
fn update_phase_status_round_trip() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "test", 10, &[]).unwrap();
    
    for status in [PhaseStatus::InProgress, PhaseStatus::Complete, PhaseStatus::Deprioritized, PhaseStatus::Paused, PhaseStatus::Pending] {
        store.update_phase_status(phase.id, status.clone()).unwrap();
        let fetched = store.get_phase(phase.id).unwrap();
        assert_eq!(fetched.status, status);
    }
}

#[test]
fn finding_list_by_experiment() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let e1 = store.create_experiment(Some(phase.id), "e1").unwrap();
    let e2 = store.create_experiment(Some(phase.id), "e2").unwrap();
    store.create_finding(Some(e1.id), "f1").unwrap();
    store.create_finding(Some(e1.id), "f2").unwrap();
    store.create_finding(Some(e2.id), "f3").unwrap();
    
    assert_eq!(store.list_findings(Some(e1.id)).unwrap().len(), 2);
    assert_eq!(store.list_findings(Some(e2.id)).unwrap().len(), 1);
    assert_eq!(store.list_findings(None).unwrap().len(), 3);
}

#[test]
fn edges_bidirectional() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "e1").unwrap();
    let f1 = store.create_finding(Some(exp.id), "A").unwrap();
    let f2 = store.create_finding(Some(exp.id), "B").unwrap();
    store.create_edge(NodeType::Finding, f1.id, NodeType::Finding, f2.id, EdgeType::Supports).unwrap();
    
    // Forward: from f1
    let from = store.get_edges_from(NodeType::Finding, f1.id).unwrap();
    assert_eq!(from.len(), 1);
    assert_eq!(from[0].target_id, f2.id);
    
    // Reverse: to f2
    let to = store.get_edges_to(NodeType::Finding, f2.id).unwrap();
    assert_eq!(to.len(), 1);
    assert_eq!(to[0].source_id, f1.id);
}
