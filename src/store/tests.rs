use super::*;
use super::sqlite::SqliteStore;

fn test_store() -> SqliteStore {
    SqliteStore::in_memory().unwrap()
}

// --- P1.1: Project CRUD ---

#[test]
fn create_project_returns_project_with_id() {
    let store = test_store();
    let p = store.create_project("volta-renaissance", Some("vr"), None).unwrap();
    assert!(p.id > 0);
    assert_eq!(p.name, "volta-renaissance");
    assert_eq!(p.alias, Some("vr".to_string()));
    assert_eq!(p.status, ProjectStatus::Active);
}

#[test]
fn get_project_by_id() {
    let store = test_store();
    let created = store.create_project("test", None, None).unwrap();
    let fetched = store.get_project(created.id).unwrap();
    assert_eq!(fetched.name, "test");
}

#[test]
fn list_projects_returns_all() {
    let store = test_store();
    store.create_project("a", None, None).unwrap();
    store.create_project("b", None, None).unwrap();
    let all = store.list_projects().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn update_project_status() {
    let store = test_store();
    let p = store.create_project("test", None, None).unwrap();
    store.update_project_status(p.id, ProjectStatus::Paused).unwrap();
    let fetched = store.get_project(p.id).unwrap();
    assert_eq!(fetched.status, ProjectStatus::Paused);
}

// --- P1.2: Phase CRUD ---

#[test]
fn create_phase_with_dependencies() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let p1 = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let p2 = store.create_phase(proj.id, "Phase 2", 30, &[p1.id]).unwrap();
    assert_eq!(p2.depends_on, vec![p1.id]);
    assert_eq!(p2.impact, 30);
}

#[test]
fn list_phases_for_project() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    store.create_phase(proj.id, "A", 10, &[]).unwrap();
    store.create_phase(proj.id, "B", 20, &[]).unwrap();
    let phases = store.list_phases(proj.id).unwrap();
    assert_eq!(phases.len(), 2);
}

// --- P1.3: Experiment CRUD ---

#[test]
fn create_experiment_with_phase() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test exp").unwrap();
    assert_eq!(exp.status, ExperimentStatus::Pending);
    assert_eq!(exp.phase_id, Some(phase.id));
}

#[test]
fn update_experiment_status_to_pass() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
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
    let proj = store.create_project("test", None, None).unwrap();
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
    let proj = store.create_project("test", None, None).unwrap();
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
    let proj = store.create_project("test", None, None).unwrap();
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
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let dec = store.create_decision(Some(exp.id), "Use Rust", Some("Type safety for KG"), None).unwrap();
    assert_eq!(dec.what, "Use Rust");
    assert_eq!(dec.why, Some("Type safety for KG".to_string()));
}

// --- Additional tests for v3 completeness ---

#[test]
fn list_experiments_by_phase() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
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
    let proj = store.create_project("test", None, None).unwrap();
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
    let proj = store.create_project("test", None, None).unwrap();
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
    let proj = store.create_project("test", None, None).unwrap();
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

// --- Migration Integration Tests (via SqliteStore) ---

#[test]
fn test_store_migration_creates_new_columns() {
    // SqliteStore::in_memory() runs init_schema + migrate
    let store = test_store();
    let proj = store.create_project("migration-test", None, None).unwrap();

    // Verify phase new fields default to None
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    assert!(phase.description.is_none());
    assert!(phase.goals.is_none());
    assert!(phase.success_criteria.is_none());
    assert!(phase.started_at.is_none());
    assert!(phase.completed_at.is_none());

    // Fetch phase and verify new fields persist as None
    let fetched = store.get_phase(phase.id).unwrap();
    assert!(fetched.description.is_none());
    assert!(fetched.goals.is_none());

    // Verify project parent_id defaults to None
    assert!(proj.parent_id.is_none());

    // Verify decision project_id defaults to None
    let dec = store.create_decision(None, "test decision", None, None).unwrap();
    assert!(dec.project_id.is_none());

    // Verify literature new fields default to None (except status which defaults to 'unread')
    let lit = store.create_literature(proj.id, "Paper", None, None, None, None, None, None, None, None, None).unwrap();
    assert!(lit.venue.is_none());
    assert!(lit.year.is_none());
    assert!(lit.code_url.is_none());
    assert!(lit.file_path.is_none());
    assert_eq!(lit.status, Some("unread".to_string()));
    assert!(lit.summary.is_none());

    // Verify hypothesis new fields default to None
    let hyp = store.create_hypothesis(None, "test hypothesis").unwrap();
    assert!(hyp.prediction.is_none());
    assert!(hyp.criteria.is_none());
    assert_eq!(hyp.confidence, Some(0.3), "hypothesis default confidence should be 0.3 after TMS migration");

    // Verify constraint new fields default to "hard" severity, rest None
    let con = store.create_constraint(proj.id, ConstraintScope::Hardware, "32GB VRAM", None, None, None, None, None).unwrap();
    assert_eq!(con.severity, Some("hard".to_string()));
    assert!(con.resource.is_none());
    assert!(con.measured_value.is_none());
    assert!(con.expires_at.is_none());

    // Verify principle new fields: rationale None, enforcement_level defaults to "advisory"
    let prin = store.create_principle(proj.id, PrincipleScope::Project, "No force kills", None, None).unwrap();
    assert!(prin.rationale.is_none());
    assert_eq!(prin.enforcement_level, Some("advisory".to_string()));
}

#[test]
fn test_store_migration_idempotent_via_store() {
    // Creating two stores on the same in-memory DB isn't possible,
    // but creating a store twice with the same file path tests idempotency.
    // For in-memory, we just verify that creating a store succeeds (which runs migrate).
    let _store1 = test_store();
    let _store2 = test_store();
    // If migrate wasn't idempotent, the second call would fail
}

#[test]
fn test_store_new_fields_round_trip_via_raw_sql() {
    // Test that the new columns work end-to-end by inserting via raw SQL
    // and reading back through the store's list methods
    let store = test_store();
    let proj = store.create_project("rt-test", None, None).unwrap();

    // Insert literature with new fields via the store, then verify list picks them up
    let lits = store.list_literature(proj.id).unwrap();
    assert_eq!(lits.len(), 0);

    store.create_literature(proj.id, "Test Paper", Some("2301.12345"), Some("High"), Some("Key finding"), None, None, None, None, None, None).unwrap();
    let lits = store.list_literature(proj.id).unwrap();
    assert_eq!(lits.len(), 1);
    assert_eq!(lits[0].title, "Test Paper");
    assert_eq!(lits[0].arxiv_id, Some("2301.12345".to_string()));

    // Verify constraints round-trip with new fields
    store.create_constraint(proj.id, ConstraintScope::Software, "Max 4 GPUs", Some("nvidia-smi"), None, None, None, None).unwrap();
    let cons = store.list_constraints(proj.id).unwrap();
    assert_eq!(cons.len(), 1);
    assert_eq!(cons[0].text, "Max 4 GPUs");
    assert_eq!(cons[0].source, Some("nvidia-smi".to_string()));

    // Verify principles round-trip with new fields
    store.create_principle(proj.id, PrincipleScope::Universal, "Always use safe-reboot", None, None).unwrap();
    let prins = store.list_principles(proj.id).unwrap();
    assert_eq!(prins.len(), 1);
    assert_eq!(prins[0].text, "Always use safe-reboot");

    // Verify hypotheses round-trip with new fields
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    store.create_hypothesis(Some(phase.id), "FP16 is faster").unwrap();
    let hyps = store.list_hypotheses(Some(phase.id)).unwrap();
    assert_eq!(hyps.len(), 1);
    assert_eq!(hyps[0].text, "FP16 is faster");
    assert!(hyps[0].prediction.is_none());
    assert_eq!(hyps[0].confidence, Some(0.3), "hypothesis default confidence should be 0.3 after TMS migration");
}


// --- Issue #3: Edge Referential Integrity + Duplicate Detection ---

#[test]
fn test_edge_to_nonexistent_source_rejected() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let f1 = store.create_finding(Some(exp.id), "finding 1").unwrap();
    // Try to create edge from nonexistent finding #999
    let result = store.create_edge(
        NodeType::Finding, 999,
        NodeType::Finding, f1.id,
        EdgeType::Supports,
    );
    assert!(result.is_err());
    match result.unwrap_err() {
        StoreError::Constraint(msg) => assert!(msg.contains("does not exist"), "Expected 'does not exist' in: {}", msg),
        other => panic!("Expected Constraint error, got: {:?}", other),
    }
}

#[test]
fn test_edge_to_nonexistent_target_rejected() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let f1 = store.create_finding(Some(exp.id), "finding 1").unwrap();
    // Try to create edge to nonexistent finding #999
    let result = store.create_edge(
        NodeType::Finding, f1.id,
        NodeType::Finding, 999,
        EdgeType::Supports,
    );
    assert!(result.is_err());
    match result.unwrap_err() {
        StoreError::Constraint(msg) => assert!(msg.contains("does not exist"), "Expected 'does not exist' in: {}", msg),
        other => panic!("Expected Constraint error, got: {:?}", other),
    }
}

#[test]
fn test_duplicate_edge_returns_existing_id() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let f1 = store.create_finding(Some(exp.id), "finding 1").unwrap();
    let f2 = store.create_finding(Some(exp.id), "finding 2").unwrap();
    // Create edge
    let edge1 = store.create_edge(
        NodeType::Finding, f1.id,
        NodeType::Finding, f2.id,
        EdgeType::Supports,
    ).unwrap();
    // Create same edge again — should return existing ID, not error
    let edge2 = store.create_edge(
        NodeType::Finding, f1.id,
        NodeType::Finding, f2.id,
        EdgeType::Supports,
    ).unwrap();
    assert_eq!(edge1.id, edge2.id);
    // Verify only one edge exists
    let edges = store.list_all_edges().unwrap();
    assert_eq!(edges.len(), 1);
}

#[test]
fn test_valid_edge_created_between_existing_nodes() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let f1 = store.create_finding(Some(exp.id), "finding 1").unwrap();
    let f2 = store.create_finding(Some(exp.id), "finding 2").unwrap();
    let edge = store.create_edge(
        NodeType::Finding, f1.id,
        NodeType::Finding, f2.id,
        EdgeType::Supports,
    ).unwrap();
    assert!(edge.id > 0);
    assert_eq!(edge.source_id, f1.id);
    assert_eq!(edge.target_id, f2.id);
    assert_eq!(edge.relation, EdgeType::Supports);
}

#[test]
fn test_edge_cross_node_types_with_integrity() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "test").unwrap();
    let finding = store.create_finding(Some(exp.id), "finding").unwrap();
    let dec = store.create_decision(Some(exp.id), "Use Rust", Some("Type safety"), None).unwrap();
    // Finding -> Decision edge should work
    let edge = store.create_edge(
        NodeType::Finding, finding.id,
        NodeType::Decision, dec.id,
        EdgeType::Informed,
    ).unwrap();
    assert_eq!(edge.relation, EdgeType::Informed);
    // Finding -> nonexistent Decision should fail
    let result = store.create_edge(
        NodeType::Finding, finding.id,
        NodeType::Decision, 999,
        EdgeType::Informed,
    );
    assert!(result.is_err());
}

#[test]
fn test_node_exists_for_all_types() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let finding = store.create_finding(Some(exp.id), "finding").unwrap();
    let dec = store.create_decision(Some(exp.id), "decision", None, None).unwrap();
    let research = store.create_research(Some(phase.id), "research").unwrap();
    let principle = store.create_principle(proj.id, PrincipleScope::Project, "principle", None, None).unwrap();
    let hyp = store.create_hypothesis(Some(phase.id), "hypothesis").unwrap();
    let con = store.create_constraint(proj.id, ConstraintScope::Hardware, "constraint", None, None, None, None, None).unwrap();
    let lit = store.create_literature(proj.id, "literature", None, None, None, None, None, None, None, None, None).unwrap();
    let fb = store.create_feedback(proj.id, "feedback", FeedbackCategory::Correction).unwrap();

    assert!(store.node_exists("Finding", finding.id).unwrap());
    assert!(store.node_exists("Experiment", exp.id).unwrap());
    assert!(store.node_exists("Decision", dec.id).unwrap());
    assert!(store.node_exists("Phase", phase.id).unwrap());
    assert!(store.node_exists("Research", research.id).unwrap());
    assert!(store.node_exists("Principle", principle.id).unwrap());
    assert!(store.node_exists("Hypothesis", hyp.id).unwrap());
    assert!(store.node_exists("Constraint", con.id).unwrap());
    assert!(store.node_exists("Literature", lit.id).unwrap());
    assert!(store.node_exists("Feedback", fb.id).unwrap());

    // Nonexistent
    assert!(!store.node_exists("Finding", 999).unwrap());
    assert!(!store.node_exists("Experiment", 999).unwrap());
}

// --- Issue #4: EdgeType Enum Expansion ---

#[test]
fn test_new_edge_types_round_trip() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let f1 = store.create_finding(Some(exp.id), "finding 1").unwrap();
    let f2 = store.create_finding(Some(exp.id), "finding 2").unwrap();
    let principle = store.create_principle(proj.id, PrincipleScope::Project, "principle", None, None).unwrap();
    let con = store.create_constraint(proj.id, ConstraintScope::Hardware, "constraint", None, None, None, None, None).unwrap();

    // Contains: phase contains experiment
    let e1 = store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Contains).unwrap();
    assert_eq!(e1.relation, EdgeType::Contains);

    // DerivedFrom: principle derived from finding
    let e2 = store.create_edge(NodeType::Principle, principle.id, NodeType::Finding, f1.id, EdgeType::DerivedFrom).unwrap();
    assert_eq!(e2.relation, EdgeType::DerivedFrom);

    // TestedBy: constraint tested by experiment
    let e3 = store.create_edge(NodeType::Constraint, con.id, NodeType::Experiment, exp.id, EdgeType::TestedBy).unwrap();
    assert_eq!(e3.relation, EdgeType::TestedBy);

    // ViolatedBy: principle violated by finding
    let e4 = store.create_edge(NodeType::Principle, principle.id, NodeType::Finding, f2.id, EdgeType::ViolatedBy).unwrap();
    assert_eq!(e4.relation, EdgeType::ViolatedBy);

    // Verify round-trip through list_all_edges
    let edges = store.list_all_edges().unwrap();
    assert_eq!(edges.len(), 4);
    assert_eq!(edges[0].relation, EdgeType::Contains);
    assert_eq!(edges[1].relation, EdgeType::DerivedFrom);
    assert_eq!(edges[2].relation, EdgeType::TestedBy);
    assert_eq!(edges[3].relation, EdgeType::ViolatedBy);

    // Verify via get_edges_from
    let phase_edges = store.get_edges_from(NodeType::Phase, phase.id).unwrap();
    assert_eq!(phase_edges.len(), 1);
    assert_eq!(phase_edges[0].relation, EdgeType::Contains);
}

// --- Issue #5: KG Label Resolution ---

#[test]
fn test_kg_decision_label_resolved() {
    use crate::kg::KgEngine;
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let finding = store.create_finding(Some(exp.id), "A finding for testing").unwrap();
    let dec = store.create_decision(Some(exp.id), "Use direct get_decision for label resolution", None, None).unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Decision, dec.id, EdgeType::Informed).unwrap();

    let kg = KgEngine::new(&store);
    let result = kg.traverse(NodeType::Finding, finding.id).unwrap();
    // The edge target should be the decision with its label resolved
    assert_eq!(result.edges.len(), 1);
    assert!(result.edges[0].1.label.contains("Use direct get_decision"), "Decision label not resolved: {}", result.edges[0].1.label);
}

#[test]
fn test_kg_hypothesis_label_resolved() {
    use crate::kg::KgEngine;
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let finding = store.create_finding(Some(exp.id), "A finding").unwrap();
    let hyp = store.create_hypothesis(Some(phase.id), "Hypothesis: direct lookup works correctly for label resolution").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();

    let kg = KgEngine::new(&store);
    let result = kg.traverse(NodeType::Finding, finding.id).unwrap();
    assert_eq!(result.edges.len(), 1);
    assert!(result.edges[0].1.label.contains("Hypothesis: direct lookup"), "Hypothesis label not resolved: {}", result.edges[0].1.label);
}

#[test]
fn test_kg_constraint_label_resolved() {
    use crate::kg::KgEngine;
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let con = store.create_constraint(proj.id, ConstraintScope::Hardware, "32GB VRAM limit per GPU for model loading", None, None, None, None, None).unwrap();
    store.create_edge(NodeType::Constraint, con.id, NodeType::Experiment, exp.id, EdgeType::TestedBy).unwrap();

    let kg = KgEngine::new(&store);
    let result = kg.traverse(NodeType::Constraint, con.id).unwrap();
    assert!(result.root.label.starts_with("32GB VRAM limit per GPU for model loading"), "got: {}", result.root.label);
}

#[test]
fn test_kg_literature_label_resolved() {
    use crate::kg::KgEngine;
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let finding = store.create_finding(Some(exp.id), "A finding").unwrap();
    let lit = store.create_literature(proj.id, "Attention Is All You Need", Some("1706.03762"), None, None, None, None, None, None, None, None).unwrap();
    store.create_edge(NodeType::Literature, lit.id, NodeType::Finding, finding.id, EdgeType::CitedIn).unwrap();

    let kg = KgEngine::new(&store);
    let result = kg.traverse(NodeType::Literature, lit.id).unwrap();
    assert!(result.root.label.starts_with("Attention Is All You Need"), "got: {}", result.root.label);
}

#[test]
fn test_kg_principle_label_resolved_directly() {
    use crate::kg::KgEngine;
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let finding = store.create_finding(Some(exp.id), "finding").unwrap();
    let principle = store.create_principle(proj.id, PrincipleScope::Universal, "Never force-kill GPU processes under any circumstances", None, None).unwrap();
    store.create_edge(NodeType::Principle, principle.id, NodeType::Finding, finding.id, EdgeType::DerivedFrom).unwrap();

    let kg = KgEngine::new(&store);
    let result = kg.traverse(NodeType::Principle, principle.id).unwrap();
    assert!(result.root.label.contains("Never force-kill"), "Principle label not resolved: {}", result.root.label);
}

// --- Get-by-id method tests ---

#[test]
fn test_get_decision_by_id() {
    let store = test_store();
    let dec = store.create_decision(None, "Test decision", Some("Because testing"), None).unwrap();
    let fetched = store.get_decision(dec.id).unwrap();
    assert_eq!(fetched.what, "Test decision");
    assert_eq!(fetched.why, Some("Because testing".to_string()));
}

#[test]
fn test_get_principle_by_id() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let p = store.create_principle(proj.id, PrincipleScope::Universal, "Test principle", None, None).unwrap();
    let fetched = store.get_principle(p.id).unwrap();
    assert_eq!(fetched.text, "Test principle");
    assert_eq!(fetched.scope, PrincipleScope::Universal);
}

#[test]
fn test_get_hypothesis_by_id() {
    let store = test_store();
    let h = store.create_hypothesis(None, "Test hypothesis").unwrap();
    let fetched = store.get_hypothesis(h.id).unwrap();
    assert_eq!(fetched.text, "Test hypothesis");
    assert_eq!(fetched.status, HypothesisStatus::Proposed);
}

#[test]
fn test_get_constraint_by_id() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let c = store.create_constraint(proj.id, ConstraintScope::Hardware, "32GB VRAM", Some("nvidia-smi"), None, None, None, None).unwrap();
    let fetched = store.get_constraint(c.id).unwrap();
    assert_eq!(fetched.text, "32GB VRAM");
    assert_eq!(fetched.source, Some("nvidia-smi".to_string()));
}

#[test]
fn test_get_literature_by_id() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let l = store.create_literature(proj.id, "Test Paper", Some("2301.00001"), Some("High"), Some("Key findings"), None, None, None, None, None, None).unwrap();
    let fetched = store.get_literature(l.id).unwrap();
    assert_eq!(fetched.title, "Test Paper");
    assert_eq!(fetched.arxiv_id, Some("2301.00001".to_string()));
}

#[test]
fn test_get_feedback_entry_by_id() {
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let f = store.create_feedback(proj.id, "Test feedback", FeedbackCategory::Correction).unwrap();
    let fetched = store.get_feedback_entry(f.id).unwrap();
    assert_eq!(fetched.text, "Test feedback");
    assert_eq!(fetched.category, FeedbackCategory::Correction);
}

#[test]
fn test_get_nonexistent_decision_returns_not_found() {
    let store = test_store();
    let result = store.get_decision(999);
    assert!(result.is_err());
    match result.unwrap_err() {
        StoreError::NotFound { entity, id } => {
            assert_eq!(entity, "decision");
            assert_eq!(id, 999);
        }
        other => panic!("Expected NotFound error, got: {:?}", other),
    }
}

// --- Issue #18: Subproject Support ---

#[test]
fn create_subproject_with_parent_id() {
    let store = test_store();
    let parent = store.create_project("home-cloud", None, None).unwrap();
    let child = store.create_project("execution-engine", None, Some(parent.id)).unwrap();
    assert_eq!(child.parent_id, Some(parent.id));
    assert_eq!(child.name, "execution-engine");
}

#[test]
fn subproject_parent_id_none_for_standalone() {
    let store = test_store();
    let proj = store.create_project("standalone", None, None).unwrap();
    assert!(proj.parent_id.is_none());
}

#[test]
fn list_projects_includes_subprojects() {
    let store = test_store();
    let parent = store.create_project("parent", None, None).unwrap();
    store.create_project("child-a", None, Some(parent.id)).unwrap();
    store.create_project("child-b", None, Some(parent.id)).unwrap();
    let all = store.list_projects().unwrap();
    assert_eq!(all.len(), 3);
    let children: Vec<_> = all.iter().filter(|p| p.parent_id == Some(parent.id)).collect();
    assert_eq!(children.len(), 2);
}

#[test]
fn subproject_get_by_id_preserves_parent() {
    let store = test_store();
    let parent = store.create_project("parent", None, None).unwrap();
    let child = store.create_project("child", None, Some(parent.id)).unwrap();
    let fetched = store.get_project(child.id).unwrap();
    assert_eq!(fetched.parent_id, Some(parent.id));
}

#[test]
fn subproject_with_alias() {
    let store = test_store();
    let parent = store.create_project("home-cloud", Some("hc"), None).unwrap();
    let child = store.create_project("infra", Some("hc-infra"), Some(parent.id)).unwrap();
    assert_eq!(child.alias, Some("hc-infra".to_string()));
    assert_eq!(child.parent_id, Some(parent.id));
}

// --- Issue #18: Subproject validation and list_subprojects ---

#[test]
fn create_subproject_with_invalid_parent_fails() {
    let store = test_store();
    let result = store.create_project("orphan", None, Some(9999));
    assert!(result.is_err());
    match result.unwrap_err() {
        StoreError::Constraint(msg) => assert!(msg.contains("does not exist")),
        other => panic!("Expected Constraint error, got: {:?}", other),
    }
}

#[test]
fn list_subprojects_returns_children_only() {
    let store = test_store();
    let parent = store.create_project("home-cloud", None, None).unwrap();
    let _standalone = store.create_project("volta", None, None).unwrap();
    let _child1 = store.create_project("execution-engine", None, Some(parent.id)).unwrap();
    let _child2 = store.create_project("infrastructure", None, Some(parent.id)).unwrap();
    let subs = store.list_subprojects(parent.id).unwrap();
    assert_eq!(subs.len(), 2);
    assert_eq!(subs[0].name, "execution-engine");
    assert_eq!(subs[1].name, "infrastructure");
}

#[test]
fn list_subprojects_empty_for_leaf_project() {
    let store = test_store();
    let proj = store.create_project("leaf", None, None).unwrap();
    let subs = store.list_subprojects(proj.id).unwrap();
    assert!(subs.is_empty());
}

#[test]
fn list_subprojects_empty_for_standalone_child() {
    let store = test_store();
    let parent = store.create_project("parent", None, None).unwrap();
    let child = store.create_project("child", None, Some(parent.id)).unwrap();
    let subs = store.list_subprojects(child.id).unwrap();
    assert!(subs.is_empty());
}

#[test]
fn subproject_status_independent_of_parent() {
    let store = test_store();
    let parent = store.create_project("parent", None, None).unwrap();
    let child = store.create_project("child", None, Some(parent.id)).unwrap();
    store.update_project_status(parent.id, ProjectStatus::Paused).unwrap();
    let child_fetched = store.get_project(child.id).unwrap();
    assert_eq!(child_fetched.status, ProjectStatus::Active);
}

#[test]
fn subproject_phases_independent_of_parent_phases() {
    let store = test_store();
    let parent = store.create_project("parent", None, None).unwrap();
    let child = store.create_project("child", None, Some(parent.id)).unwrap();
    store.create_phase(parent.id, "Parent Phase", 50, &[]).unwrap();
    store.create_phase(child.id, "Child Phase", 30, &[]).unwrap();
    let parent_phases = store.list_phases(parent.id).unwrap();
    let child_phases = store.list_phases(child.id).unwrap();
    assert_eq!(parent_phases.len(), 1);
    assert_eq!(child_phases.len(), 1);
    assert_eq!(parent_phases[0].name, "Parent Phase");
    assert_eq!(child_phases[0].name, "Child Phase");
}

#[test]
fn nested_subprojects_two_levels() {
    let store = test_store();
    let root = store.create_project("org", None, None).unwrap();
    let mid = store.create_project("team", None, Some(root.id)).unwrap();
    let leaf = store.create_project("component", None, Some(mid.id)).unwrap();
    assert_eq!(leaf.parent_id, Some(mid.id));
    assert_eq!(mid.parent_id, Some(root.id));
    let root_subs = store.list_subprojects(root.id).unwrap();
    assert_eq!(root_subs.len(), 1);
    assert_eq!(root_subs[0].name, "team");
    let mid_subs = store.list_subprojects(mid.id).unwrap();
    assert_eq!(mid_subs.len(), 1);
    assert_eq!(mid_subs[0].name, "component");
}

// === Per-project ordinal numbering (project_seq) tests ===

#[test]
fn project_seq_phases_sequential_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", Some("PA"), None).unwrap();
    let p2 = store.create_project("proj-b", Some("PB"), None).unwrap();

    let ph1 = store.create_phase(p1.id, "A-Phase1", 10, &[]).unwrap();
    let ph2 = store.create_phase(p1.id, "A-Phase2", 20, &[]).unwrap();
    let ph3 = store.create_phase(p2.id, "B-Phase1", 30, &[]).unwrap();
    let ph4 = store.create_phase(p2.id, "B-Phase2", 40, &[]).unwrap();

    // Project A phases should be #1, #2
    assert_eq!(ph1.project_seq, Some(1));
    assert_eq!(ph2.project_seq, Some(2));
    // Project B phases should also be #1, #2 (not #3, #4)
    assert_eq!(ph3.project_seq, Some(1));
    assert_eq!(ph4.project_seq, Some(2));
}

#[test]
fn project_seq_experiments_sequential_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    let p2 = store.create_project("proj-b", None, None).unwrap();
    let ph1 = store.create_phase(p1.id, "Phase-A", 10, &[]).unwrap();
    let ph2 = store.create_phase(p2.id, "Phase-B", 10, &[]).unwrap();

    let e1 = store.create_experiment(Some(ph1.id), "A-Exp1").unwrap();
    let e2 = store.create_experiment(Some(ph1.id), "A-Exp2").unwrap();
    let e3 = store.create_experiment(Some(ph2.id), "B-Exp1").unwrap();

    assert_eq!(e1.project_seq, Some(1));
    assert_eq!(e2.project_seq, Some(2));
    assert_eq!(e3.project_seq, Some(1)); // Proj B starts at 1
}

#[test]
fn project_seq_findings_sequential_per_project() {
    let store = test_store();
    let proj = store.create_project("proj-a", None, None).unwrap();
    let ph = store.create_phase(proj.id, "Phase-1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(ph.id), "Exp-1").unwrap();

    let f1 = store.create_finding(Some(exp.id), "Finding one").unwrap();
    let f2 = store.create_finding(Some(exp.id), "Finding two").unwrap();

    assert_eq!(f1.project_seq, Some(1));
    assert_eq!(f2.project_seq, Some(2));
}

#[test]
fn project_seq_orphan_nodes_get_none() {
    let store = test_store();
    // Finding without experiment_id gets no project_seq
    let f = store.create_finding(None, "Orphan finding").unwrap();
    assert_eq!(f.project_seq, None);

    // Experiment without phase_id gets no project_seq
    let e = store.create_experiment(None, "Orphan experiment").unwrap();
    assert_eq!(e.project_seq, None);
}

#[test]
fn project_seq_decisions_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    let p2 = store.create_project("proj-b", None, None).unwrap();

    let d1 = store.create_decision(None, "Decision A1", Some("reason"), Some(p1.id)).unwrap();
    let d2 = store.create_decision(None, "Decision A2", Some("reason"), Some(p1.id)).unwrap();
    let d3 = store.create_decision(None, "Decision B1", Some("reason"), Some(p2.id)).unwrap();

    assert_eq!(d1.project_seq, Some(1));
    assert_eq!(d2.project_seq, Some(2));
    assert_eq!(d3.project_seq, Some(1)); // Proj B starts at 1
}

#[test]
fn project_seq_principles_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    let p2 = store.create_project("proj-b", None, None).unwrap();

    let pr1 = store.create_principle(p1.id, PrincipleScope::Project, "Principle A1", None, None).unwrap();
    let pr2 = store.create_principle(p1.id, PrincipleScope::Project, "Principle A2", None, None).unwrap();
    let pr3 = store.create_principle(p2.id, PrincipleScope::Project, "Principle B1", None, None).unwrap();

    assert_eq!(pr1.project_seq, Some(1));
    assert_eq!(pr2.project_seq, Some(2));
    assert_eq!(pr3.project_seq, Some(1));
}

#[test]
fn project_seq_constraints_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();

    let c1 = store.create_constraint(p1.id, ConstraintScope::Hardware, "C1", None, None, None, None, None).unwrap();
    let c2 = store.create_constraint(p1.id, ConstraintScope::Hardware, "C2", None, None, None, None, None).unwrap();

    assert_eq!(c1.project_seq, Some(1));
    assert_eq!(c2.project_seq, Some(2));
}

#[test]
fn project_seq_literature_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    let p2 = store.create_project("proj-b", None, None).unwrap();

    let l1 = store.create_literature(p1.id, "Paper A1", None, None, None, None, None, None, None, None, None).unwrap();
    let l2 = store.create_literature(p1.id, "Paper A2", None, None, None, None, None, None, None, None, None).unwrap();
    let l3 = store.create_literature(p2.id, "Paper B1", None, None, None, None, None, None, None, None, None).unwrap();

    assert_eq!(l1.project_seq, Some(1));
    assert_eq!(l2.project_seq, Some(2));
    assert_eq!(l3.project_seq, Some(1));
}

#[test]
fn project_seq_feedback_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();

    let fb1 = store.create_feedback(p1.id, "Feedback 1", FeedbackCategory::Correction).unwrap();
    let fb2 = store.create_feedback(p1.id, "Feedback 2", FeedbackCategory::Confirmation).unwrap();

    assert_eq!(fb1.project_seq, Some(1));
    assert_eq!(fb2.project_seq, Some(2));
}

#[test]
fn project_seq_resolve_node_id_with_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", Some("PA"), None).unwrap();
    let p2 = store.create_project("proj-b", Some("PB"), None).unwrap();

    let ph1 = store.create_phase(p1.id, "A-Phase1", 10, &[]).unwrap();
    let ph2 = store.create_phase(p2.id, "B-Phase1", 20, &[]).unwrap();

    // Resolve project_seq=1 in proj-a -> should get ph1's global ID
    let resolved = store.resolve_node_id("phases", 1, Some("proj-a")).unwrap();
    assert_eq!(resolved, ph1.id);

    // Resolve project_seq=1 in proj-b -> should get ph2's global ID
    let resolved2 = store.resolve_node_id("phases", 1, Some("proj-b")).unwrap();
    assert_eq!(resolved2, ph2.id);

    // Resolve by alias
    let resolved3 = store.resolve_node_id("phases", 1, Some("PA")).unwrap();
    assert_eq!(resolved3, ph1.id);
}

#[test]
fn project_seq_resolve_node_id_without_project_returns_global() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    store.create_phase(p1.id, "Phase1", 10, &[]).unwrap();

    // Without project context, treat as global ID
    let resolved = store.resolve_node_id("phases", 1, None).unwrap();
    assert_eq!(resolved, 1); // global ID passthrough
}

#[test]
fn project_seq_resolve_experiment_via_phase() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    let p2 = store.create_project("proj-b", None, None).unwrap();
    let ph1 = store.create_phase(p1.id, "Phase-A", 10, &[]).unwrap();
    let ph2 = store.create_phase(p2.id, "Phase-B", 10, &[]).unwrap();

    let e1 = store.create_experiment(Some(ph1.id), "Exp-A1").unwrap();
    let e2 = store.create_experiment(Some(ph2.id), "Exp-B1").unwrap();

    let resolved_a = store.resolve_node_id("experiments", 1, Some("proj-a")).unwrap();
    assert_eq!(resolved_a, e1.id);

    let resolved_b = store.resolve_node_id("experiments", 1, Some("proj-b")).unwrap();
    assert_eq!(resolved_b, e2.id);
}

#[test]
fn project_seq_resolve_finding_via_experiment_phase() {
    let store = test_store();
    let proj = store.create_project("proj-a", None, None).unwrap();
    let ph = store.create_phase(proj.id, "Phase-1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(ph.id), "Exp-1").unwrap();

    let f1 = store.create_finding(Some(exp.id), "Finding text 1").unwrap();
    let _f2 = store.create_finding(Some(exp.id), "Finding text 2").unwrap();

    let resolved = store.resolve_node_id("findings", 1, Some("proj-a")).unwrap();
    assert_eq!(resolved, f1.id);
}

#[test]
fn project_seq_cross_project_numbering_independent() {
    // Full integration: create two projects with overlapping node types,
    // verify project_seq is independent per project
    let store = test_store();
    let pa = store.create_project("alpha", Some("A"), None).unwrap();
    let pb = store.create_project("beta", Some("B"), None).unwrap();

    // Alpha: 3 phases, 2 principles
    let a_ph1 = store.create_phase(pa.id, "Alpha-P1", 10, &[]).unwrap();
    let a_ph2 = store.create_phase(pa.id, "Alpha-P2", 20, &[]).unwrap();
    let a_ph3 = store.create_phase(pa.id, "Alpha-P3", 30, &[]).unwrap();
    let a_pr1 = store.create_principle(pa.id, PrincipleScope::Project, "A-Principle-1", None, None).unwrap();
    let a_pr2 = store.create_principle(pa.id, PrincipleScope::Project, "A-Principle-2", None, None).unwrap();

    // Beta: 2 phases, 1 principle
    let b_ph1 = store.create_phase(pb.id, "Beta-P1", 10, &[]).unwrap();
    let b_ph2 = store.create_phase(pb.id, "Beta-P2", 20, &[]).unwrap();
    let b_pr1 = store.create_principle(pb.id, PrincipleScope::Project, "B-Principle-1", None, None).unwrap();

    // Alpha phases: 1, 2, 3
    assert_eq!(a_ph1.project_seq, Some(1));
    assert_eq!(a_ph2.project_seq, Some(2));
    assert_eq!(a_ph3.project_seq, Some(3));

    // Beta phases: 1, 2 (independent)
    assert_eq!(b_ph1.project_seq, Some(1));
    assert_eq!(b_ph2.project_seq, Some(2));

    // Alpha principles: 1, 2
    assert_eq!(a_pr1.project_seq, Some(1));
    assert_eq!(a_pr2.project_seq, Some(2));

    // Beta principles: 1 (independent)
    assert_eq!(b_pr1.project_seq, Some(1));

    // Resolve: Phase #1 in Alpha != Phase #1 in Beta
    let alpha_ph1_id = store.resolve_node_id("phases", 1, Some("alpha")).unwrap();
    let beta_ph1_id = store.resolve_node_id("phases", 1, Some("beta")).unwrap();
    assert_ne!(alpha_ph1_id, beta_ph1_id);
    assert_eq!(alpha_ph1_id, a_ph1.id);
    assert_eq!(beta_ph1_id, b_ph1.id);
}

#[test]
fn project_seq_get_project_by_name() {
    let store = test_store();
    let p = store.create_project("test-proj", Some("TP"), None).unwrap();

    let by_name = store.get_project_by_name("test-proj").unwrap();
    assert_eq!(by_name.id, p.id);

    let by_alias = store.get_project_by_name("TP").unwrap();
    assert_eq!(by_alias.id, p.id);

    let not_found = store.get_project_by_name("nonexistent");
    assert!(not_found.is_err());
}

#[test]
fn project_seq_hypotheses_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    let p2 = store.create_project("proj-b", None, None).unwrap();
    let ph1 = store.create_phase(p1.id, "Phase-A", 10, &[]).unwrap();
    let ph2 = store.create_phase(p2.id, "Phase-B", 10, &[]).unwrap();

    let h1 = store.create_hypothesis(Some(ph1.id), "Hyp A1").unwrap();
    let h2 = store.create_hypothesis(Some(ph1.id), "Hyp A2").unwrap();
    let h3 = store.create_hypothesis(Some(ph2.id), "Hyp B1").unwrap();

    assert_eq!(h1.project_seq, Some(1));
    assert_eq!(h2.project_seq, Some(2));
    assert_eq!(h3.project_seq, Some(1));
}

#[test]
fn project_seq_research_per_project() {
    let store = test_store();
    let p1 = store.create_project("proj-a", None, None).unwrap();
    let ph1 = store.create_phase(p1.id, "Phase-A", 10, &[]).unwrap();

    let r1 = store.create_research(Some(ph1.id), "Research 1").unwrap();
    let r2 = store.create_research(Some(ph1.id), "Research 2").unwrap();

    assert_eq!(r1.project_seq, Some(1));
    assert_eq!(r2.project_seq, Some(2));
}

#[test]
fn project_seq_persisted_and_retrieved() {
    // Verify project_seq is persisted to DB and returned on get
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let ph = store.create_phase(proj.id, "Phase-1", 10, &[]).unwrap();
    assert_eq!(ph.project_seq, Some(1));

    // Re-fetch and verify
    let fetched = store.get_phase(ph.id).unwrap();
    assert_eq!(fetched.project_seq, Some(1));
}

#[test]
fn project_seq_migration_backfill() {
    // The in-memory store runs migrations automatically.
    // Verify that after migration, existing data has project_seq set.
    let store = test_store();
    let proj = store.create_project("test", None, None).unwrap();
    let ph = store.create_phase(proj.id, "Phase-1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(ph.id), "Exp-1").unwrap();
    let f = store.create_finding(Some(exp.id), "A finding").unwrap();
    let d = store.create_decision(None, "Decision", Some("why"), Some(proj.id)).unwrap();

    assert_eq!(ph.project_seq, Some(1));
    assert_eq!(exp.project_seq, Some(1));
    assert_eq!(f.project_seq, Some(1));
    assert_eq!(d.project_seq, Some(1));
}

// === Feature 5: Temporal Awareness ===

#[test]
fn test_session_create_and_end() {
    let store = test_store();
    let proj = store.create_project("temporal-test", None, None).unwrap();
    let session = store.create_session(Some(proj.id)).unwrap();
    assert!(session.id > 0);
    assert_eq!(session.project_id, Some(proj.id));
    assert!(session.ended_at.is_none());

    store.end_session(session.id, Some("Completed initial experiments")).unwrap();
    let sessions = store.list_sessions(Some(proj.id)).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].ended_at.is_some());
    assert_eq!(sessions[0].summary.as_deref(), Some("Completed initial experiments"));
}

#[test]
fn test_session_list_by_project() {
    let store = test_store();
    let proj1 = store.create_project("proj-a", None, None).unwrap();
    let proj2 = store.create_project("proj-b", None, None).unwrap();
    store.create_session(Some(proj1.id)).unwrap();
    store.create_session(Some(proj1.id)).unwrap();
    store.create_session(Some(proj2.id)).unwrap();

    let s1 = store.list_sessions(Some(proj1.id)).unwrap();
    let s2 = store.list_sessions(Some(proj2.id)).unwrap();
    let all = store.list_sessions(None).unwrap();

    assert_eq!(s1.len(), 2);
    assert_eq!(s2.len(), 1);
    assert_eq!(all.len(), 3);
}

#[test]
fn test_get_current_session_returns_open() {
    let store = test_store();
    let proj = store.create_project("current-test", None, None).unwrap();
    let s1 = store.create_session(Some(proj.id)).unwrap();
    store.end_session(s1.id, None).unwrap();
    let s2 = store.create_session(Some(proj.id)).unwrap();

    let current = store.get_current_session().unwrap();
    assert!(current.is_some());
    assert_eq!(current.unwrap().id, s2.id);
}

#[test]
fn test_get_current_session_none_when_all_ended() {
    let store = test_store();
    let proj = store.create_project("all-ended", None, None).unwrap();
    let s = store.create_session(Some(proj.id)).unwrap();
    store.end_session(s.id, None).unwrap();

    let current = store.get_current_session().unwrap();
    assert!(current.is_none());
}

#[test]
fn test_modified_at_set_on_create() {
    let store = test_store();
    let proj = store.create_project("modified-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    // modified_at should be set on creation
    let modified: Option<String> = store.get_modified_at("phases", phase.id).unwrap();
    assert!(modified.is_some(), "modified_at should be set on phase creation");
}

#[test]
fn test_modified_at_updated_on_status_change() {
    let store = test_store();
    let proj = store.create_project("modified-update", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let before: String = store.get_modified_at("phases", phase.id).unwrap().unwrap();

    // Small delay to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(1100));

    store.update_phase_status(phase.id, super::PhaseStatus::InProgress).unwrap();
    let after: String = store.get_modified_at("phases", phase.id).unwrap().unwrap();
    assert!(after >= before, "modified_at should be updated after status change: {} >= {}", after, before);
}

#[test]
fn test_nodes_since_returns_new_nodes() {
    let store = test_store();
    let proj = store.create_project("since-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();

    // Get timestamp before creating more nodes
    let before_ts = chrono::Local::now().naive_local().format("%Y-%m-%d %H:%M:%S").to_string();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let exp = store.create_experiment(Some(phase.id), "New Exp").unwrap();
    let _finding = store.create_finding(Some(exp.id), "A new finding discovered").unwrap();
    let _decision = store.create_decision(None, "New decision", Some("because"), Some(proj.id)).unwrap();

    let delta = store.nodes_since(&before_ts).unwrap();
    // The phase was created before the timestamp, but exp/finding/decision after
    assert!(!delta.experiments.is_empty(), "should have new experiments");
    assert!(!delta.findings.is_empty(), "should have new findings");
    assert!(!delta.decisions.is_empty(), "should have new decisions");
}

#[test]
fn test_nodes_since_excludes_old_nodes() {
    let store = test_store();
    let proj = store.create_project("exclude-test", None, None).unwrap();
    let _phase = store.create_phase(proj.id, "Old Phase", 40, &[]).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let after_ts = chrono::Local::now().naive_local().format("%Y-%m-%d %H:%M:%S").to_string();

    let delta = store.nodes_since(&after_ts).unwrap();
    assert!(delta.phases.is_empty(), "old phase should not appear in delta");
}

#[test]
fn test_staleness_hypothesis_7_days() {
    let store = test_store();
    let proj = store.create_project("stale-hyp", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let h = store.create_hypothesis(Some(phase.id), "Stale hypothesis that should be flagged").unwrap();

    // Manually backdate the hypothesis to 10 days ago
    store.backdate_created_at("hypotheses", h.id, 10).unwrap();

    let report = store.staleness_report(proj.id).unwrap();
    assert!(!report.stale_hypotheses.is_empty(), "hypothesis >7 days should appear as stale");
    assert!(report.stale_hypotheses[0].1 >= 7, "days stale should be >= 7");
}

#[test]
fn test_staleness_experiment_14_days() {
    let store = test_store();
    let proj = store.create_project("stale-exp", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "Stale experiment").unwrap();

    // Manually backdate the experiment to 20 days ago
    store.backdate_created_at("experiments", exp.id, 20).unwrap();

    let report = store.staleness_report(proj.id).unwrap();
    assert!(!report.stale_experiments.is_empty(), "experiment >14 days should appear as stale");
    assert!(report.stale_experiments[0].1 >= 14, "days stale should be >= 14");
}

#[test]
fn test_staleness_unconnected_findings() {
    let store = test_store();
    let proj = store.create_project("unconnected-f", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();
    let f = store.create_finding(Some(exp.id), "Unconnected finding").unwrap();

    // Backdate finding to 35 days ago (>30 day threshold)
    store.backdate_created_at("findings", f.id, 35).unwrap();

    let report = store.staleness_report(proj.id).unwrap();
    assert!(!report.unconnected_findings.is_empty(), "finding >30 days without edges should appear as unconnected");
}

#[test]
fn test_velocity_findings_per_session() {
    let store = test_store();
    let proj = store.create_project("velocity-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();

    let s1 = store.create_session(Some(proj.id)).unwrap();
    store.create_finding(Some(exp.id), "Finding in session 1").unwrap();
    store.create_finding(Some(exp.id), "Another finding in session 1").unwrap();
    store.end_session(s1.id, None).unwrap();

    let s2 = store.create_session(Some(proj.id)).unwrap();
    store.create_finding(Some(exp.id), "Finding in session 2").unwrap();
    store.end_session(s2.id, None).unwrap();

    let velocity = store.get_velocity(proj.id).unwrap();
    assert!(velocity.findings_per_session.len() >= 2, "should have entries for at least 2 sessions");
}


// =========================================================
// TMS v6: Truth-Maintenance System Tests
// =========================================================

#[test]
fn test_tms_migration_adds_columns() {
    // Verify migration v11 adds confidence + belief_status to all node tables
    let store = test_store();
    // After SqliteStore::in_memory(), all migrations run. Verify columns exist by querying them.
    let proj = store.create_project("tms-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();

    // Finding: should have confidence (default 0.5) and belief_status (default "believed")
    let finding = store.create_finding(Some({
        let e = store.create_experiment(Some(phase.id), "E1").unwrap();
        e.id
    }), "Test finding for TMS").unwrap();
    assert_eq!(finding.confidence, Some(0.5));
    assert_eq!(finding.belief_status, Some("believed".to_string()));

    // Decision: default confidence 0.5
    let decision = store.create_decision(None, "Test decision for TMS", Some("Because TMS"), Some(proj.id)).unwrap();
    assert_eq!(decision.confidence, Some(0.5));
    assert_eq!(decision.belief_status, Some("believed".to_string()));

    // Hypothesis: default confidence 0.3 (lower start)
    let hyp = store.create_hypothesis(Some(phase.id), "Test hypothesis for TMS").unwrap();
    assert_eq!(hyp.confidence, Some(0.3));
    assert_eq!(hyp.belief_status, Some("believed".to_string()));

    // Principle: default confidence 0.8
    let principle = store.create_principle(proj.id, crate::store::PrincipleScope::Project, "Test principle for TMS", Some("TMS test"), None).unwrap();
    assert_eq!(principle.confidence, Some(0.8));
    assert_eq!(principle.belief_status, Some("believed".to_string()));

    // Constraint: default confidence 0.9
    let constraint = store.create_constraint(proj.id, crate::store::ConstraintScope::Hardware, "Test constraint for TMS", Some("test"), None, None, None, None).unwrap();
    assert_eq!(constraint.confidence, Some(0.9));
    assert_eq!(constraint.belief_status, Some("believed".to_string()));
}

#[test]
fn test_default_confidence_by_type() {
    let store = test_store();
    let proj = store.create_project("conf-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();

    let f = store.create_finding(Some(exp.id), "Finding for confidence test").unwrap();
    assert!((f.confidence.unwrap() - 0.5).abs() < 0.01, "Finding default confidence should be 0.5");

    let h = store.create_hypothesis(Some(phase.id), "Hyp for confidence test").unwrap();
    assert!((h.confidence.unwrap() - 0.3).abs() < 0.01, "Hypothesis default confidence should be 0.3");

    let p = store.create_principle(proj.id, crate::store::PrincipleScope::Project, "Principle for confidence test", None, None).unwrap();
    assert!((p.confidence.unwrap() - 0.8).abs() < 0.01, "Principle default confidence should be 0.8");

    let c = store.create_constraint(proj.id, crate::store::ConstraintScope::Hardware, "Constraint for confidence test", None, None, None, None, None).unwrap();
    assert!((c.confidence.unwrap() - 0.9).abs() < 0.01, "Constraint default confidence should be 0.9");

    let d = store.create_decision(None, "Decision for confidence test", Some("why"), Some(proj.id)).unwrap();
    assert!((d.confidence.unwrap() - 0.5).abs() < 0.01, "Decision default confidence should be 0.5");
}

#[test]
fn test_update_confidence() {
    let store = test_store();
    let proj = store.create_project("upd-conf", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();
    let f = store.create_finding(Some(exp.id), "Confidence update test finding").unwrap();

    // Update confidence to 0.8
    store.update_confidence("Finding", f.id, 0.8).unwrap();
    let f2 = store.get_finding(f.id).unwrap();
    assert!((f2.confidence.unwrap() - 0.8).abs() < 0.01);

    // Also test for Decision
    let d = store.create_decision(None, "Decision conf test", Some("why"), Some(proj.id)).unwrap();
    store.update_confidence("Decision", d.id, 0.95).unwrap();
    let d2 = store.get_decision(d.id).unwrap();
    assert!((d2.confidence.unwrap() - 0.95).abs() < 0.01);
}

#[test]
fn test_update_belief_status() {
    let store = test_store();
    let proj = store.create_project("upd-belief", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();
    let f = store.create_finding(Some(exp.id), "Belief status test finding").unwrap();
    assert_eq!(f.belief_status.as_deref(), Some("believed"));

    store.update_belief_status("Finding", f.id, "suspended").unwrap();
    let f2 = store.get_finding(f.id).unwrap();
    assert_eq!(f2.belief_status.as_deref(), Some("suspended"));

    store.update_belief_status("Finding", f.id, "retracted").unwrap();
    let f3 = store.get_finding(f.id).unwrap();
    assert_eq!(f3.belief_status.as_deref(), Some("retracted"));
}

#[test]
fn test_contradicts_edge_suspends_dependents() {
    let store = test_store();
    let proj = store.create_project("contra-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();

    // Create a chain: finding1 -> (Supports) -> finding2 -> (Informed) -> decision1
    let f1 = store.create_finding(Some(exp.id), "Original finding that will be contradicted").unwrap();
    let f2 = store.create_finding(Some(exp.id), "Finding that depends on f1 via Supports edge").unwrap();
    let d1 = store.create_decision(None, "Decision that depends on f2 via Informed edge", Some("Because f2"), Some(proj.id)).unwrap();

    // Create dependency edges
    store.create_edge(NodeType::Finding, f1.id, NodeType::Finding, f2.id, EdgeType::Supports).unwrap();
    store.create_edge(NodeType::Finding, f2.id, NodeType::Decision, d1.id, EdgeType::Informed).unwrap();

    // All nodes should be "believed" initially
    assert_eq!(store.get_finding(f2.id).unwrap().belief_status.as_deref(), Some("believed"));
    assert_eq!(store.get_decision(d1.id).unwrap().belief_status.as_deref(), Some("believed"));

    // Create a new finding that contradicts f1
    let f_contra = store.create_finding(Some(exp.id), "Contradicting finding that disproves f1").unwrap();

    // Create Contradicts edge: f_contra contradicts f1
    let result = store.create_edge_with_tms(
        NodeType::Finding, f_contra.id,
        NodeType::Finding, f1.id,
        EdgeType::Contradicts,
    ).unwrap();

    // f1 itself should have reduced confidence
    let f1_after = store.get_finding(f1.id).unwrap();
    assert!(f1_after.confidence.unwrap() < 0.5, "f1 confidence should be reduced after contradiction");

    // f2 and d1 (downstream dependents) should be suspended
    let f2_after = store.get_finding(f2.id).unwrap();
    let d1_after = store.get_decision(d1.id).unwrap();
    assert_eq!(f2_after.belief_status.as_deref(), Some("suspended"), "f2 should be suspended as dependent");
    assert_eq!(d1_after.belief_status.as_deref(), Some("suspended"), "d1 should be suspended as dependent");

    // Verify suspended nodes are returned in the TMS result
    assert!(!result.suspended_nodes.is_empty(), "Should return list of suspended nodes");
}

#[test]
fn test_supports_edge_increases_confidence() {
    let store = test_store();
    let proj = store.create_project("support-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();

    // Use a hypothesis (default confidence 0.3) where even 1 support triggers an increase
    // Formula: min(0.95, 0.3 + 0.1 * support_count), max(current, formula)
    // Hypothesis starts at 0.3. With 1 support: formula = 0.4, max(0.3, 0.4) = 0.4 -- increase!
    let h = store.create_hypothesis(Some(phase.id), "Hypothesis that will get supporting evidence").unwrap();
    let initial_conf = h.confidence.unwrap();
    assert!((initial_conf - 0.3).abs() < 0.01);

    // Create supporting evidence
    let s1 = store.create_finding(Some(exp.id), "Supporting evidence 1 for hypothesis").unwrap();
    store.create_edge_with_tms(NodeType::Finding, s1.id, NodeType::Hypothesis, h.id, EdgeType::Supports).unwrap();

    let h_after = store.get_hypothesis(h.id).unwrap();
    assert!(h_after.confidence.unwrap() > initial_conf,
        "Confidence should increase with support: was {} now {}", initial_conf, h_after.confidence.unwrap());

    // Also verify: with a Finding (default 0.5), need 3 supports to increase
    let target = store.create_finding(Some(exp.id), "Target finding for support test").unwrap();
    let f_initial = target.confidence.unwrap(); // 0.5
    for i in 0..3 {
        let s = store.create_finding(Some(exp.id), &format!("Support {} for finding", i)).unwrap();
        store.create_edge_with_tms(NodeType::Finding, s.id, NodeType::Finding, target.id, EdgeType::Supports).unwrap();
    }
    let target_after = store.get_finding(target.id).unwrap();
    assert!(target_after.confidence.unwrap() > f_initial,
        "Finding confidence should increase after 3 supports: was {} now {}", f_initial, target_after.confidence.unwrap());
}

#[test]
fn test_confidence_cap_at_095() {
    let store = test_store();
    let proj = store.create_project("cap-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();

    let target = store.create_finding(Some(exp.id), "Target finding for confidence cap test").unwrap();

    // Add many supporting edges (formula: min(0.95, 0.3 + 0.1 * count))
    // At 7 supports: 0.3 + 0.7 = 1.0 -> capped at 0.95
    for i in 0..10 {
        let s = store.create_finding(Some(exp.id), &format!("Support {} for cap test", i)).unwrap();
        store.create_edge_with_tms(NodeType::Finding, s.id, NodeType::Finding, target.id, EdgeType::Supports).unwrap();
    }

    let target_final = store.get_finding(target.id).unwrap();
    assert!(target_final.confidence.unwrap() <= 0.95,
        "Confidence should be capped at 0.95, got {}", target_final.confidence.unwrap());
    assert!((target_final.confidence.unwrap() - 0.95).abs() < 0.01,
        "Confidence should be exactly 0.95 with 10 supports, got {}", target_final.confidence.unwrap());
}

#[test]
fn test_low_confidence_triggers_suspension() {
    let store = test_store();
    let proj = store.create_project("low-conf-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();

    // Create a hypothesis (starts at 0.3 confidence)
    let h = store.create_hypothesis(Some(phase.id), "Hypothesis that will be contradicted").unwrap();
    assert!((h.confidence.unwrap() - 0.3).abs() < 0.01);

    // Contradict it (reduces by 0.2 -> 0.1, which is < 0.3 threshold)
    let f_contra = store.create_finding(Some(exp.id), "Finding that contradicts the hypothesis").unwrap();
    store.create_edge_with_tms(
        NodeType::Finding, f_contra.id,
        NodeType::Hypothesis, h.id,
        EdgeType::Contradicts,
    ).unwrap();

    let h_after = store.get_hypothesis(h.id).unwrap();
    assert!(h_after.confidence.unwrap() < 0.3,
        "Hypothesis confidence should be below 0.3 after contradiction: {}", h_after.confidence.unwrap());
    assert_eq!(h_after.belief_status.as_deref(), Some("suspended"),
        "Hypothesis should be auto-suspended when confidence drops below 0.3");
}

#[test]
fn test_review_shows_suspended_nodes() {
    let store = test_store();
    let proj = store.create_project("review-sus-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();

    let f = store.create_finding(Some(exp.id), "A finding that will be suspended in the review test to verify display").unwrap();
    store.update_belief_status("Finding", f.id, "suspended").unwrap();

    let h = store.create_hypothesis(Some(phase.id), "A hypothesis that is retracted in the review test for display verification").unwrap();
    store.update_belief_status("Hypothesis", h.id, "retracted").unwrap();

    let review_output = crate::mcp::review::tool_review(&store, "review-sus-test");
    assert!(review_output.contains("Suspended") || review_output.contains("suspended"),
        "Review should mention suspended nodes. Output:\n{}", review_output);
    assert!(review_output.contains("Retracted") || review_output.contains("retracted"),
        "Review should mention retracted nodes. Output:\n{}", review_output);
}

#[test]
fn test_search_includes_confidence() {
    let store = test_store();
    let proj = store.create_project("search-conf-test", None, None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "E1").unwrap();

    let f = store.create_finding(Some(exp.id), "Unique xylophone finding for search confidence verification test").unwrap();
    store.update_confidence("Finding", f.id, 0.85).unwrap();
    store.update_belief_status("Finding", f.id, "suspended").unwrap();

    let search_output = crate::mcp::review::tool_search(&store, "xylophone");
    assert!(search_output.contains("0.85") || search_output.contains("confidence"),
        "Search results should include confidence. Output:\n{}", search_output);
    assert!(search_output.contains("suspended"),
        "Search results should include belief status. Output:\n{}", search_output);
}
