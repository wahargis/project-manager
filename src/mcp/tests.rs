//! Tests for MCP tool implementations (Sprint 2).

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, ExperimentStatus, PhaseStatus, HypothesisStatus, NodeType, EdgeType};

fn test_store() -> SqliteStore {
    SqliteStore::in_memory().unwrap()
}

// Helper: create a project with a phase and experiment
fn setup_project(store: &SqliteStore) -> (i64, i64, i64) {
    let proj = store.create_project("test-project", Some("tp")).unwrap();
    let phase = store.create_phase(proj.id, "Phase 1", 40, &[]).unwrap();
    let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();
    (proj.id, phase.id, exp.id)
}

// === Issue #7: MCP split verification ===

#[test]
fn test_split_dashboard_works() {
    let store = test_store();
    let _ = setup_project(&store);
    let result = super::dashboard::tool_dashboard(&store);
    assert!(result.contains("Cross-Project Dashboard"));
}

#[test]
fn test_split_next_works() {
    let store = test_store();
    let _ = setup_project(&store);
    let result = super::dashboard::tool_next(&store, "test-project");
    assert!(result.contains("Next Phases"));
}

#[test]
fn test_split_scaffold_works() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let result = super::dashboard::tool_scaffold(&store, "test-project", phase_id);
    assert!(result.contains("Phase #"));
}

#[test]
fn test_split_session_init_works() {
    let store = test_store();
    let _ = setup_project(&store);
    let result = super::dashboard::tool_session_init(&store);
    assert!(result.contains("Session Init") || result.contains("pending"));
}

#[test]
fn test_split_review_works() {
    let store = test_store();
    let _ = setup_project(&store);
    let result = super::review::tool_review(&store, "test-project");
    assert!(result.contains("Research Review"));
}

#[test]
fn test_split_stats_works() {
    let store = test_store();
    let _ = setup_project(&store);
    let result = super::review::tool_stats(&store, "test-project");
    assert!(result.contains("Phases:"));
}

#[test]
fn test_split_add_edge_works() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let f1 = store.create_finding(Some(exp_id), "finding 1").unwrap();
    let f2 = store.create_finding(Some(exp_id), "finding 2").unwrap();
    let result = super::edges::tool_add_edge(&store, "finding", f1.id, "finding", f2.id, "supports");
    assert!(result.contains("Edge #"));
}

#[test]
fn test_split_kg_traverse_works() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let f1 = store.create_finding(Some(exp_id), "finding for traverse").unwrap();
    let result = super::edges::tool_kg_traverse(&store, "finding", f1.id);
    assert!(result.contains("ROOT:"));
}

// === Issue #6: Decision project_id + why-required ===

#[test]
fn test_decision_with_project_name() {
    let store = test_store();
    let _ = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, Some("test-project"));
    assert!(result.contains("Decision #"));
    // Verify project_id was stored
    let decisions = store.list_decisions(1).unwrap();
    assert!(!decisions.is_empty());
    assert_eq!(decisions[0].project_id, Some(1));
}

#[test]
fn test_decision_with_project_alias() {
    let store = test_store();
    let _ = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, Some("tp"));
    assert!(result.contains("Decision #"));
}

#[test]
fn test_decision_with_invalid_project() {
    let store = test_store();
    let _ = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, Some("nonexistent"));
    assert!(result.contains("Project not found"));
}

#[test]
fn test_decision_why_required_validation() {
    let store = test_store();
    let _ = setup_project(&store);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, None, None, None);
    assert!(result.contains("VALIDATION ERROR"));
    assert!(result.contains("why"));
}

#[test]
fn test_decision_without_project_still_works() {
    let store = test_store();
    let _ = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, None);
    assert!(result.contains("Decision #"));
}

#[test]
fn test_list_decisions_uses_direct_project_id() {
    let store = test_store();
    let (proj_id, _, exp_id) = setup_project(&store);
    // Create decision with project_id
    store.create_decision(Some(exp_id), "decision with project", Some("why"), Some(proj_id)).unwrap();
    // Create decision without project_id
    store.create_decision(None, "decision without project", Some("why"), None).unwrap();
    let decisions = store.list_decisions(proj_id).unwrap();
    // Should include both: the one with project_id=proj_id and the one with project_id=NULL
    assert_eq!(decisions.len(), 2);
}

// === Issue #8: Phase Nodes Redesign ===

#[test]
fn test_phase_update_description() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let result = super::nodes::tool_phase_update(
        &store, phase_id, Some("A detailed description"), Some("Goal 1"), Some("Criteria 1"), None
    );
    assert!(result.contains("fields updated"));
    let phase = store.get_phase(phase_id).unwrap();
    assert_eq!(phase.description, Some("A detailed description".to_string()));
    assert_eq!(phase.goals, Some("Goal 1".to_string()));
    assert_eq!(phase.success_criteria, Some("Criteria 1".to_string()));
}

#[test]
fn test_phase_update_to_in_progress_sets_started_at() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let phase_before = store.get_phase(phase_id).unwrap();
    assert!(phase_before.started_at.is_none());
    let result = super::nodes::tool_phase_update(&store, phase_id, None, None, None, Some("in_progress"));
    assert!(result.contains("InProgress"));
    let phase_after = store.get_phase(phase_id).unwrap();
    assert!(phase_after.started_at.is_some());
    assert_eq!(phase_after.status, PhaseStatus::InProgress);
}

#[test]
fn test_phase_update_completion_gating_pending_experiments() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    // Experiment is still pending, so completion should be rejected
    let result = super::nodes::tool_phase_update(&store, phase_id, None, None, None, Some("complete"));
    assert!(result.contains("Cannot complete phase"));
    assert!(result.contains("pending experiment"));
}

#[test]
fn test_phase_update_completion_succeeds_all_resolved() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    // Resolve the experiment
    store.update_experiment_status(exp_id, ExperimentStatus::Pass, Some("done")).unwrap();
    let result = super::nodes::tool_phase_update(&store, phase_id, None, None, None, Some("complete"));
    assert!(result.contains("Complete"));
    let phase = store.get_phase(phase_id).unwrap();
    assert!(phase.completed_at.is_some());
    assert_eq!(phase.status, PhaseStatus::Complete);
}

#[test]
fn test_phase_update_invalid_status() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let result = super::nodes::tool_phase_update(&store, phase_id, None, None, None, Some("foobar"));
    assert!(result.contains("VALIDATION ERROR"));
}

#[test]
fn test_phase_update_nonexistent() {
    let store = test_store();
    let result = super::nodes::tool_phase_update(&store, 999, None, None, None, None);
    assert!(result.contains("Phase not found"));
}

#[test]
fn test_scaffold_shows_rollup() {
    let store = test_store();
    let (_proj_id, phase_id, exp_id) = setup_project(&store);
    // Add more experiments with different statuses
    let exp2 = store.create_experiment(Some(phase_id), "Exp 2").unwrap();
    store.update_experiment_status(exp2.id, ExperimentStatus::Pass, Some("ok")).unwrap();
    let exp3 = store.create_experiment(Some(phase_id), "Exp 3").unwrap();
    store.update_experiment_status(exp3.id, ExperimentStatus::Fail, Some("bad")).unwrap();
    // Create a finding
    store.create_finding(Some(exp_id), "A finding").unwrap();
    // Create a hypothesis
    store.create_hypothesis(Some(phase_id), "A hypothesis").unwrap();

    let result = super::dashboard::tool_scaffold(&store, "test-project", phase_id);
    assert!(result.contains("Experiment Summary:"));
    assert!(result.contains("3 total"));
    assert!(result.contains("1 pending"));
    assert!(result.contains("1 pass"));
    assert!(result.contains("1 fail"));
    assert!(result.contains("Findings: 1"));
    assert!(result.contains("Open hypotheses: 1"));
}

#[test]
fn test_next_shows_phase_dependencies() {
    let store = test_store();
    let proj = store.create_project("dep-test", None).unwrap();
    let p1 = store.create_phase(proj.id, "Base Phase", 40, &[]).unwrap();
    let _p2 = store.create_phase(proj.id, "Dependent Phase", 30, &[p1.id]).unwrap();
    // Complete p1 so p2 becomes actionable
    store.update_phase_status(p1.id, PhaseStatus::Complete).unwrap();
    let result = super::dashboard::tool_next(&store, "dep-test");
    assert!(result.contains("Dependent Phase"), "output: {}", result);
    assert!(result.contains("Depends on:"), "output: {}", result);
}

// === Issue #9: Hypothesis Lifecycle Enforcement ===

#[test]
fn test_hyp_proposed_to_testing_requires_evidence() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    // No edges yet — should reject
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "testing", None, None, None, None, None);
    assert!(result.contains("Cannot transition to testing"));
    assert!(result.contains("no supporting evidence"));
}

#[test]
fn test_hyp_proposed_to_testing_succeeds_with_evidence() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Supporting evidence").unwrap();
    // Add supporting edge
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "testing", None, None, None, None, None);
    assert!(result.contains("updated to Testing"));
}

#[test]
fn test_hyp_testing_to_refuted_requires_finding() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Supporting evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    // Move to testing first
    store.update_hypothesis(hyp.id, HypothesisStatus::Testing, None, None).unwrap();
    // Try to refute without finding_id
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "refuted", None, None, None, None, None);
    assert!(result.contains("Cannot refute"));
    assert!(result.contains("finding_id"));
}

#[test]
fn test_hyp_testing_to_refuted_creates_contradiction_edge() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Supporting evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    store.update_hypothesis(hyp.id, HypothesisStatus::Testing, None, None).unwrap();
    let disproving = store.create_finding(Some(exp_id), "Disproving evidence").unwrap();
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "refuted", None, Some(disproving.id), None, None, None);
    assert!(result.contains("Refuted"));
    assert!(result.contains("Auto-created edge"));
    // Verify the contradiction edge was created
    let edges = store.get_edges_to(NodeType::Hypothesis, hyp.id).unwrap();
    let contradicts: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::Contradicts).collect();
    assert_eq!(contradicts.len(), 1);
    assert_eq!(contradicts[0].source_id, disproving.id);
}

#[test]
fn test_hyp_confirmed_suggests_principle() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    store.update_hypothesis(hyp.id, HypothesisStatus::Testing, None, None).unwrap();
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "confirmed", None, None, None, None, None);
    assert!(result.contains("Confirmed"));
    assert!(result.contains("pm_principle_add"));
}

#[test]
fn test_hyp_update_prediction_criteria_confidence() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let result = super::nodes::tool_hyp_update(
        &store, hyp.id, "proposed", None, None,
        Some("Will improve by 10%"), Some("p < 0.05"), Some(0.85)
    );
    assert!(result.contains("Proposed"));
    let updated = store.get_hypothesis(hyp.id).unwrap();
    assert_eq!(updated.prediction, Some("Will improve by 10%".to_string()));
    assert_eq!(updated.criteria, Some("p < 0.05".to_string()));
    assert!((updated.confidence.unwrap() - 0.85).abs() < 0.001);
}

#[test]
fn test_review_shows_orphaned_hypotheses() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    // Create hypothesis with no edges
    store.create_hypothesis(Some(phase_id), "Orphan hypothesis").unwrap();
    let result = super::review::tool_review(&store, "test-project");
    assert!(result.contains("orphaned hypothesis"));
}

#[test]
fn test_review_no_orphan_warning_when_all_linked() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Linked hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    let result = super::review::tool_review(&store, "test-project");
    assert!(!result.contains("orphaned hypothesis"));
}

// === Issue #10: Finding Minimum Length Enforcement ===

#[test]
fn test_finding_too_short_rejected() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let result = super::nodes::tool_log_finding(&store, exp_id, "too short");
    assert!(result.contains("VALIDATION ERROR"));
    assert!(result.contains("100 required"));
}

#[test]
fn test_finding_exactly_100_chars_accepted() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let text = "a".repeat(100);
    let result = super::nodes::tool_log_finding(&store, exp_id, &text);
    assert!(result.contains("Finding #"));
    assert!(!result.contains("VALIDATION ERROR"));
}

#[test]
fn test_finding_orphan_warning() {
    let store = test_store();
    let _ = setup_project(&store);
    let text = "a".repeat(150);
    // Pass eid=0 which means no experiment
    let result = super::nodes::tool_log_finding(&store, 0, &text);
    assert!(result.contains("Finding #"));
    assert!(result.contains("orphaned findings"));
}

#[test]
fn test_finding_with_experiment_no_orphan_warning() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let result = super::nodes::tool_log_finding(&store, exp_id, &text);
    assert!(result.contains("Finding #"));
    assert!(!result.contains("orphaned"));
}

// === Dispatch integration tests ===

#[test]
fn test_dispatch_pm_phase_update() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let args = serde_json::json!({
        "phase_id": phase_id,
        "description": "New description",
        "status": "in_progress"
    });
    let result = super::dispatch_tool(&store, "pm_phase_update", &args);
    assert!(result.contains("InProgress"));
}

#[test]
fn test_dispatch_pm_decision_with_project() {
    let store = test_store();
    let _ = setup_project(&store);
    let args = serde_json::json!({
        "what": "a]".to_string() + &"b".repeat(60),
        "why": "c".repeat(60),
        "project": "test-project"
    });
    let result = super::dispatch_tool(&store, "pm_decision", &args);
    assert!(result.contains("Decision #"));
}

#[test]
fn test_dispatch_unknown_tool() {
    let store = test_store();
    let args = serde_json::json!({});
    let result = super::dispatch_tool(&store, "pm_nonexistent", &args);
    assert!(result.contains("Unknown tool"));
}
