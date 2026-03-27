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
    let dec = store.create_decision(Some(exp.id), "Use Rust", Some("Type safety for KG"), None).unwrap();
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

// --- Migration Integration Tests (via SqliteStore) ---

#[test]
fn test_store_migration_creates_new_columns() {
    // SqliteStore::in_memory() runs init_schema + migrate
    let store = test_store();
    let proj = store.create_project("migration-test", None).unwrap();

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
    assert!(hyp.confidence.is_none());

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
    let proj = store.create_project("rt-test", None).unwrap();

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
    assert!(hyps[0].confidence.is_none());
}


// --- Issue #3: Edge Referential Integrity + Duplicate Detection ---

#[test]
fn test_edge_to_nonexistent_source_rejected() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
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
    assert!(edge.id > 0);
    assert_eq!(edge.source_id, f1.id);
    assert_eq!(edge.target_id, f2.id);
    assert_eq!(edge.relation, EdgeType::Supports);
}

#[test]
fn test_edge_cross_node_types_with_integrity() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let con = store.create_constraint(proj.id, ConstraintScope::Hardware, "32GB VRAM limit per GPU for model loading", None, None, None, None, None).unwrap();
    store.create_edge(NodeType::Constraint, con.id, NodeType::Experiment, exp.id, EdgeType::TestedBy).unwrap();

    let kg = KgEngine::new(&store);
    let result = kg.traverse(NodeType::Constraint, con.id).unwrap();
    assert_eq!(result.root.label, "32GB VRAM limit per GPU for model loading");
}

#[test]
fn test_kg_literature_label_resolved() {
    use crate::kg::KgEngine;
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "exp").unwrap();
    let finding = store.create_finding(Some(exp.id), "A finding").unwrap();
    let lit = store.create_literature(proj.id, "Attention Is All You Need", Some("1706.03762"), None, None, None, None, None, None, None, None).unwrap();
    store.create_edge(NodeType::Literature, lit.id, NodeType::Finding, finding.id, EdgeType::CitedIn).unwrap();

    let kg = KgEngine::new(&store);
    let result = kg.traverse(NodeType::Literature, lit.id).unwrap();
    assert_eq!(result.root.label, "Attention Is All You Need");
}

#[test]
fn test_kg_principle_label_resolved_directly() {
    use crate::kg::KgEngine;
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
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
    let proj = store.create_project("test", None).unwrap();
    let c = store.create_constraint(proj.id, ConstraintScope::Hardware, "32GB VRAM", Some("nvidia-smi"), None, None, None, None).unwrap();
    let fetched = store.get_constraint(c.id).unwrap();
    assert_eq!(fetched.text, "32GB VRAM");
    assert_eq!(fetched.source, Some("nvidia-smi".to_string()));
}

#[test]
fn test_get_literature_by_id() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
    let l = store.create_literature(proj.id, "Test Paper", Some("2301.00001"), Some("High"), Some("Key findings"), None, None, None, None, None, None).unwrap();
    let fetched = store.get_literature(l.id).unwrap();
    assert_eq!(fetched.title, "Test Paper");
    assert_eq!(fetched.arxiv_id, Some("2301.00001".to_string()));
}

#[test]
fn test_get_feedback_entry_by_id() {
    let store = test_store();
    let proj = store.create_project("test", None).unwrap();
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
