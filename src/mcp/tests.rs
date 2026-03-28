//! Tests for MCP tool implementations (Sprint 2).

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, ExperimentStatus, PhaseStatus, HypothesisStatus, NodeType, EdgeType};

fn test_store() -> SqliteStore {
    SqliteStore::in_memory().unwrap()
}

// Helper: create a project with a phase and experiment
fn setup_project(store: &SqliteStore) -> (i64, i64, i64) {
    let proj = store.create_project("test-project", Some("tp"), None).unwrap();
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
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, None, Some("test-project"));
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
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, None, Some("tp"));
    assert!(result.contains("Decision #"));
}

#[test]
fn test_decision_with_invalid_project() {
    let store = test_store();
    let _ = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, None, Some("nonexistent"));
    assert!(result.contains("Project not found"));
}

#[test]
fn test_decision_why_required_validation() {
    let store = test_store();
    let _ = setup_project(&store);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, None, None, None, None);
    assert!(result.contains("VALIDATION ERROR"));
    assert!(result.contains("why"));
}

#[test]
fn test_decision_without_project_still_works() {
    let store = test_store();
    let _ = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, None, None);
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
    let proj = store.create_project("dep-test", None, None).unwrap();
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
    assert!(result.contains("Contradicts"), "Expected Contradicts in: {}", result);
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

// === Sprint 3: Issue #11 — Literature Nodes Redesign ===

#[test]
fn test_lit_add_with_all_new_fields() {
    let store = test_store();
    let _ = setup_project(&store);
    let args = serde_json::json!({
        "project": "test-project",
        "title": "Attention Is All You Need",
        "authors": "Vaswani et al.",
        "arxiv_id": "1706.03762",
        "venue": "NeurIPS",
        "year": 2017,
        "url": "https://arxiv.org/abs/1706.03762",
        "code_url": "https://github.com/tensorflow/tensor2tensor",
        "summary": "Introduces the Transformer architecture based on self-attention",
        "relevance": &"x".repeat(100),
        "key_findings": &"y".repeat(200)
    });
    let result = super::nodes::tool_lit_add(&store, &args);
    assert!(result.contains("Literature #"), "result: {}", result);
    assert!(result.contains("Attention Is All You Need"));

    // Verify round-trip: all fields persisted
    let lits = store.list_literature(1).unwrap();
    assert_eq!(lits.len(), 1);
    assert_eq!(lits[0].authors, Some("Vaswani et al.".to_string()));
    assert_eq!(lits[0].venue, Some("NeurIPS".to_string()));
    assert_eq!(lits[0].year, Some(2017));
    assert_eq!(lits[0].url, Some("https://arxiv.org/abs/1706.03762".to_string()));
    assert_eq!(lits[0].code_url, Some("https://github.com/tensorflow/tensor2tensor".to_string()));
    assert_eq!(lits[0].summary, Some("Introduces the Transformer architecture based on self-attention".to_string()));
    assert_eq!(lits[0].status, Some("unread".to_string()));
}

#[test]
fn test_lit_status_lifecycle() {
    let store = test_store();
    let proj = store.create_project("test-project", Some("tp"), None).unwrap();
    let lit = store.create_literature(proj.id, "Paper", Some("1234.56789"), None, None, Some("Author"), None, None, None, None, None).unwrap();
    assert_eq!(lit.status, Some("unread".to_string()));

    // unread -> read
    let result = super::nodes::tool_lit_status(&store, lit.id, "read");
    assert!(result.contains("status updated to 'read'"));
    let fetched = store.get_literature(lit.id).unwrap();
    assert_eq!(fetched.status, Some("read".to_string()));

    // read -> cited
    let result = super::nodes::tool_lit_status(&store, lit.id, "cited");
    assert!(result.contains("status updated to 'cited'"));

    // cited -> tested
    let result = super::nodes::tool_lit_status(&store, lit.id, "tested");
    assert!(result.contains("status updated to 'tested'"));

    // tested -> integrated
    let result = super::nodes::tool_lit_status(&store, lit.id, "integrated");
    assert!(result.contains("status updated to 'integrated'"));
    let fetched = store.get_literature(lit.id).unwrap();
    assert_eq!(fetched.status, Some("integrated".to_string()));
}

#[test]
fn test_lit_status_invalid_status() {
    let store = test_store();
    let proj = store.create_project("test-project", Some("tp"), None).unwrap();
    let lit = store.create_literature(proj.id, "Paper", Some("1234.56789"), None, None, Some("Author"), None, None, None, None, None).unwrap();
    let result = super::nodes::tool_lit_status(&store, lit.id, "foobar");
    assert!(result.contains("VALIDATION ERROR"));
}

#[test]
fn test_lit_status_nonexistent() {
    let store = test_store();
    let result = super::nodes::tool_lit_status(&store, 999, "read");
    assert!(result.contains("Error") || result.contains("not found"));
}

// === Sprint 3: Issue #12 — Principle Nodes Redesign ===

#[test]
fn test_principle_auto_edge_from_finding() {
    let store = test_store();
    let (proj_id, _, exp_id) = setup_project(&store);
    let text = "a".repeat(100);
    let finding = store.create_finding(Some(exp_id), &text).unwrap();
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "project",
        "text": &"b".repeat(60),
        "rationale": "Because we observed this in the experiment",
        "finding_id": finding.id
    });
    let result = super::nodes::tool_principle_add(&store, &args);
    assert!(result.contains("Principle #"), "result: {}", result);
    assert!(result.contains("DerivedFrom"), "Expected DerivedFrom edge in: {}", result);
    assert!(result.contains(&format!("Finding #{}", finding.id)), "Expected finding ID in: {}", result);

    // Verify the edge was created
    let principles = store.list_principles(proj_id).unwrap();
    assert_eq!(principles.len(), 1);
    let edges = store.get_edges_from(NodeType::Principle, principles[0].id).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].relation, EdgeType::DerivedFrom);
    assert_eq!(edges[0].target_id, finding.id);
}

#[test]
fn test_principle_auto_edge_from_decision() {
    let store = test_store();
    let (proj_id, _, _) = setup_project(&store);
    let what = "a".repeat(60);
    let why = "b".repeat(60);
    let dec = store.create_decision(None, &what, Some(&why), Some(proj_id)).unwrap();
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "project",
        "text": &"c".repeat(60),
        "rationale": "Because of the decision",
        "decision_id": dec.id
    });
    let result = super::nodes::tool_principle_add(&store, &args);
    assert!(result.contains("Principle #"));
    assert!(result.contains("DerivedFrom"));
    assert!(result.contains(&format!("Decision #{}", dec.id)));
}

#[test]
fn test_principle_with_rationale_and_enforcement() {
    let store = test_store();
    let _ = setup_project(&store);
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "universal",
        "text": &"a".repeat(60),
        "rationale": "Prevents driver wedge states",
        "enforcement_level": "mandatory"
    });
    let result = super::nodes::tool_principle_add(&store, &args);
    assert!(result.contains("Principle #"));

    let principles = store.list_principles(1).unwrap();
    assert_eq!(principles[0].rationale, Some("Prevents driver wedge states".to_string()));
    assert_eq!(principles[0].enforcement_level, Some("mandatory".to_string()));
}

#[test]
fn test_principle_surfaced_in_log_finding() {
    let store = test_store();
    let (proj_id, _, exp_id) = setup_project(&store);
    // Create a principle
    store.create_principle(proj_id, crate::store::PrincipleScope::Project, "Never force-kill GPU processes — use systemctl stop", Some("Prevents driver wedge"), Some("mandatory")).unwrap();
    // Log a finding
    let text = "a".repeat(150);
    let result = super::nodes::tool_log_finding(&store, exp_id, &text);
    assert!(result.contains("Active principles"), "Expected principles surfaced in: {}", result);
    assert!(result.contains("Never force-kill"), "Expected principle text in: {}", result);
}

// === Sprint 3: Issue #13 — Constraint Nodes Redesign ===

#[test]
fn test_constraint_with_new_fields() {
    let store = test_store();
    let _ = setup_project(&store);
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "hardware",
        "text": &"a".repeat(60),
        "source": "nvidia-smi",
        "severity": "hard",
        "resource": "GPU VRAM",
        "measured_value": "32768 MB",
        "expires_at": "2026-12-31"
    });
    let result = super::nodes::tool_constraint_add(&store, &args);
    assert!(result.contains("Constraint #"), "result: {}", result);

    let constraints = store.list_constraints(1).unwrap();
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].severity, Some("hard".to_string()));
    assert_eq!(constraints[0].resource, Some("GPU VRAM".to_string()));
    assert_eq!(constraints[0].measured_value, Some("32768 MB".to_string()));
    assert_eq!(constraints[0].expires_at, Some("2026-12-31".to_string()));
}

#[test]
fn test_constraint_expiry_detection() {
    let store = test_store();
    let (proj_id, _, _) = setup_project(&store);
    // Create an expired constraint (date in the past)
    store.create_constraint(
        proj_id,
        crate::store::ConstraintScope::Hardware,
        &"a".repeat(60),
        Some("test source"),
        Some("hard"),
        Some("GPU"),
        None,
        Some("2025-01-01"), // expired
    ).unwrap();
    // Create a non-expired constraint
    store.create_constraint(
        proj_id,
        crate::store::ConstraintScope::Software,
        &"b".repeat(60),
        Some("test source"),
        Some("soft"),
        None,
        None,
        Some("2099-12-31"), // far future
    ).unwrap();

    let result = super::review::tool_review(&store, "test-project");
    assert!(result.contains("Expired constraints: 1"), "Expected 1 expired constraint in: {}", result);
    assert!(result.contains("expired 2025-01-01"), "Expected expiry date in: {}", result);
}

#[test]
fn test_scaffold_shows_constraints() {
    let store = test_store();
    let (proj_id, phase_id, _) = setup_project(&store);
    store.create_constraint(
        proj_id,
        crate::store::ConstraintScope::Hardware,
        &"a".repeat(60),
        Some("hardware spec"),
        Some("hard"),
        None,
        None,
        None,
    ).unwrap();
    let result = super::dashboard::tool_scaffold(&store, "test-project", phase_id);
    assert!(result.contains("Active Constraints"), "Expected constraints in scaffold: {}", result);
    assert!(result.contains("[hard]"), "Expected severity in: {}", result);
}

// === Sprint 3: Issue #14 — Orphan Detection ===

#[test]
fn test_orphan_detection() {
    let store = test_store();
    let (proj_id, phase_id, exp_id) = setup_project(&store);
    // Create orphaned nodes (no edges)
    let text = "a".repeat(100);
    store.create_finding(Some(exp_id), &text).unwrap();
    store.create_finding(Some(exp_id), &text).unwrap();
    store.create_decision(None, "orphan decision", Some("why"), Some(proj_id)).unwrap();
    store.create_hypothesis(Some(phase_id), "orphan hypothesis").unwrap();
    store.create_literature(proj_id, "orphan paper", Some("1234.5"), None, None, Some("Author"), None, None, None, None, None).unwrap();
    store.create_principle(proj_id, crate::store::PrincipleScope::Project, "orphan principle", None, None).unwrap();
    store.create_constraint(proj_id, crate::store::ConstraintScope::Hardware, "orphan constraint", Some("source"), None, None, None, None).unwrap();

    let result = super::review::tool_review(&store, "test-project");
    assert!(result.contains("Orphaned nodes:"), "Expected orphan detection in: {}", result);
    // Should find orphans for multiple types
    assert!(result.contains("Finding:") || result.contains("finding"), "Expected finding orphans in: {}", result);
    assert!(result.contains("Literature:") || result.contains("literature"), "Expected literature orphans in: {}", result);
}

#[test]
fn test_orphan_detection_excludes_linked_nodes() {
    let store = test_store();
    let (proj_id, _phase_id, exp_id) = setup_project(&store);
    let text = "a".repeat(100);
    let f1 = store.create_finding(Some(exp_id), &text).unwrap();
    let f2 = store.create_finding(Some(exp_id), &text).unwrap();
    // Link f1 to f2 — now neither is orphaned
    store.create_edge(NodeType::Finding, f1.id, NodeType::Finding, f2.id, EdgeType::Supports).unwrap();
    // Create an orphan for comparison
    let lit = store.create_literature(proj_id, "unlinked paper", Some("1234.5"), None, None, Some("Author"), None, None, None, None, None).unwrap();

    let finding_orphans = store.get_orphaned_nodes("finding", proj_id).unwrap();
    assert!(finding_orphans.is_empty(), "Linked findings should not be orphaned: {:?}", finding_orphans);

    let lit_orphans = store.get_orphaned_nodes("literature", proj_id).unwrap();
    assert_eq!(lit_orphans.len(), 1);
    assert_eq!(lit_orphans[0], lit.id);
}

// === Sprint 3: Issue #15 — Post-Creation Edge Suggestions ===

#[test]
fn test_lit_add_edge_suggestions() {
    let store = test_store();
    let proj = store.create_project("test-project", Some("tp"), None).unwrap();
    // Create phase with a matching word
    store.create_phase(proj.id, "Kernel Optimization Research", 40, &[]).unwrap();
    let _exp = store.create_experiment(None, "Exp1").unwrap();

    let args = serde_json::json!({
        "project": "test-project",
        "title": "Kernel Fusion Optimization for GPU Inference",
        "authors": "Smith et al.",
        "arxiv_id": "2401.12345",
        "relevance": &"x".repeat(100),
        "key_findings": &"y".repeat(200)
    });
    let result = super::nodes::tool_lit_add(&store, &args);
    assert!(result.contains("Literature #"), "result: {}", result);
    // Should suggest edge to phase with "Optimization" overlap
    assert!(result.contains("pm_add_edge source_type=literature"), "Expected edge suggestion in: {}", result);
    assert!(result.contains("target_type=phase"), "Expected phase edge suggestion in: {}", result);
}

#[test]
fn test_constraint_add_edge_suggestions() {
    let store = test_store();
    let proj = store.create_project("test-project", Some("tp"), None).unwrap();
    let phase = store.create_phase(proj.id, "VRAM Optimization Phase", 40, &[]).unwrap();
    let _exp = store.create_experiment(Some(phase.id), "Pending experiment").unwrap();

    let args = serde_json::json!({
        "project": "test-project",
        "scope": "hardware",
        "text": &("VRAM ".to_string() + &"a".repeat(56)),
        "source": "nvidia-smi"
    });
    let result = super::nodes::tool_constraint_add(&store, &args);
    assert!(result.contains("Constraint #"), "result: {}", result);
    // Should suggest edge to experiment (pending experiments in all phases)
    assert!(result.contains("pm_add_edge source_type=constraint"), "Expected edge suggestion in: {}", result);
}

#[test]
fn test_principle_add_suggests_constraint_edges() {
    let store = test_store();
    let (proj_id, _, _) = setup_project(&store);
    store.create_constraint(proj_id, crate::store::ConstraintScope::Hardware, &"a".repeat(60), Some("hardware spec"), None, None, None, None).unwrap();
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "project",
        "text": &"b".repeat(60),
        "rationale": "Because of constraints"
    });
    let result = super::nodes::tool_principle_add(&store, &args);
    assert!(result.contains("Principle #"));
    assert!(result.contains("Active constraints"), "Expected constraint suggestions in: {}", result);
    assert!(result.contains("pm_add_edge source_type=principle"), "Expected edge suggestion in: {}", result);
}

#[test]
fn test_finding_suggests_experiment_and_literature_edges() {
    let store = test_store();
    let (proj_id, _, exp_id) = setup_project(&store);
    // Add some literature
    store.create_literature(proj_id, "Recent Paper", Some("2401.00001"), None, None, Some("Author"), None, None, None, None, None).unwrap();
    let text = "a".repeat(150);
    let result = super::nodes::tool_log_finding(&store, exp_id, &text);
    assert!(result.contains("Finding #"));
    // Should have causal guidance section
    assert!(result.contains("Causal Links"), "Expected causal links section in: {}", result);
    // Should suggest downstream edges (findings inform decisions/hypotheses)
    assert!(result.contains("target_type=Decision") || result.contains("target_type=Hypothesis"), "Expected downstream suggestions in: {}", result);
    // Should suggest edge to literature
    assert!(result.contains("Recent literature"), "Expected literature suggestion in: {}", result);
}

// === Dispatch integration for new tools ===

#[test]
fn test_dispatch_pm_lit_status() {
    let store = test_store();
    let proj = store.create_project("test-project", Some("tp"), None).unwrap();
    let lit = store.create_literature(proj.id, "Paper", Some("1234.5"), None, None, Some("Auth"), None, None, None, None, None).unwrap();
    let args = serde_json::json!({
        "literature_id": lit.id,
        "status": "read"
    });
    let result = super::dispatch_tool(&store, "pm_lit_status", &args);
    assert!(result.contains("status updated to 'read'"), "result: {}", result);
}

// === Issue #18: Subproject Dashboard Grouping ===

#[test]
fn test_dashboard_groups_subprojects_under_parent() {
    let store = test_store();
    let parent = store.create_project("home-cloud", None, None).unwrap();
    let child = store.create_project("execution-engine", None, Some(parent.id)).unwrap();
    store.create_phase(child.id, "Refactor EE", 40, &[]).unwrap();
    let result = super::dashboard::tool_dashboard(&store);
    assert!(result.contains("## home-cloud"), "Should have parent header: {}", result);
    assert!(result.contains("[home-cloud/execution-engine]"), "Should show parent/child format: {}", result);
}

#[test]
fn test_dashboard_standalone_project_no_group_header() {
    let store = test_store();
    store.create_project("standalone", None, None).unwrap();
    let result = super::dashboard::tool_dashboard(&store);
    // Standalone projects should NOT get a "##" header
    assert!(!result.contains("## standalone"), "Standalone should not have group header: {}", result);
}

#[test]
fn test_dashboard_mixed_standalone_and_grouped() {
    let store = test_store();
    // Standalone project
    let standalone = store.create_project("volta-renaissance", None, None).unwrap();
    store.create_phase(standalone.id, "Phase 1", 50, &[]).unwrap();
    // Parent with subprojects
    let parent = store.create_project("home-cloud", None, None).unwrap();
    let child1 = store.create_project("execution-engine", None, Some(parent.id)).unwrap();
    let child2 = store.create_project("infrastructure", None, Some(parent.id)).unwrap();
    store.create_phase(child1.id, "EE Phase", 30, &[]).unwrap();
    store.create_phase(child2.id, "Infra Phase", 20, &[]).unwrap();

    let result = super::dashboard::tool_dashboard(&store);
    assert!(result.contains("[volta-renaissance]"), "Should show standalone: {}", result);
    assert!(result.contains("## home-cloud"), "Should have parent header: {}", result);
    assert!(result.contains("[home-cloud/execution-engine]"), "Should show child1: {}", result);
    assert!(result.contains("[home-cloud/infrastructure]"), "Should show child2: {}", result);
}

// --- Issue #18: Subproject session_init and list_subprojects ---

#[test]
fn test_session_init_shows_subproject_hierarchy() {
    let store = test_store();
    let parent = store.create_project("home-cloud", None, None).unwrap();
    let child = store.create_project("execution-engine", None, Some(parent.id)).unwrap();
    let phase = store.create_phase(child.id, "Refactor EE", 40, &[]).unwrap();
    store.create_experiment(Some(phase.id), "Test refactor").unwrap();
    let result = super::dashboard::tool_session_init(&store);
    // Session init should show the hierarchical label
    assert!(result.contains("home-cloud/execution-engine") || result.contains("execution-engine"),
        "Session init should reference subproject: {}", result);
}

#[test]
fn test_dashboard_parent_own_phases_shown() {
    let store = test_store();
    let parent = store.create_project("home-cloud", None, None).unwrap();
    let _child = store.create_project("execution-engine", None, Some(parent.id)).unwrap();
    store.create_phase(parent.id, "Platform Phase", 50, &[]).unwrap();
    let result = super::dashboard::tool_dashboard(&store);
    assert!(result.contains("## home-cloud"), "Should have parent header: {}", result);
    assert!(result.contains("Platform Phase"), "Should show parent's own phase: {}", result);
}

#[test]
fn test_list_subprojects_via_store() {
    let store = test_store();
    let parent = store.create_project("home-cloud", None, None).unwrap();
    store.create_project("execution-engine", None, Some(parent.id)).unwrap();
    store.create_project("agent-system", None, Some(parent.id)).unwrap();
    let subs = store.list_subprojects(parent.id).unwrap();
    assert_eq!(subs.len(), 2);
    let names: Vec<_> = subs.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"execution-engine"));
    assert!(names.contains(&"agent-system"));
}

#[test]
fn test_create_subproject_invalid_parent_rejected() {
    let store = test_store();
    let result = store.create_project("orphan", None, Some(9999));
    assert!(result.is_err(), "Should reject invalid parent_id");
}

// === Issue #19: Causal Backbone Edge Enforcement ===

#[test]
fn test_log_finding_auto_creates_phase_contains_edge() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let result = super::nodes::tool_log_finding(&store, exp_id, &text);
    assert!(result.contains("Finding #"));
    // Should auto-create Phase --Contains--> Finding
    assert!(result.contains("Phase#") && result.contains("--Contains-->") && result.contains("Finding#"),
        "Expected Phase --Contains--> Finding auto-edge in: {}", result);
    // Verify both auto-edges exist
    let findings = store.list_findings(Some(exp_id)).unwrap();
    let fid = findings.last().unwrap().id;
    let edges = store.get_edges_to(NodeType::Finding, fid).unwrap();
    let contains: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::Contains && e.source_type == NodeType::Phase).collect();
    assert_eq!(contains.len(), 1, "Expected exactly 1 Phase --Contains--> Finding edge");
    assert_eq!(contains[0].source_id, phase_id);
    let produced: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::ProducedBy).collect();
    assert_eq!(produced.len(), 1, "Expected exactly 1 Experiment --ProducedBy--> Finding edge");
}

#[test]
fn test_log_finding_no_phase_edge_without_experiment() {
    let store = test_store();
    let _ = setup_project(&store);
    let text = "a".repeat(150);
    // eid=0 means no experiment
    let result = super::nodes::tool_log_finding(&store, 0, &text);
    assert!(result.contains("Finding #"));
    // Should NOT contain Phase --Contains-->
    assert!(!result.contains("--Contains-->"), "Should not auto-create phase edge without experiment: {}", result);
}

#[test]
fn test_log_finding_causal_guidance_section() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let result = super::nodes::tool_log_finding(&store, exp_id, &text);
    assert!(result.contains("=== Causal Links ==="), "Expected causal links section in: {}", result);
    assert!(result.contains("Auto-created:"), "Expected auto-created section in: {}", result);
    assert!(result.contains("Suggested (specify if applicable):"), "Expected suggestions in: {}", result);
    // Should suggest decision and hypothesis edges
    assert!(result.contains("informs a decision"), "Expected decision suggestion: {}", result);
    assert!(result.contains("supports a hypothesis"), "Expected hypothesis suggestion: {}", result);
}

#[test]
fn test_decision_warns_no_causal_upstream() {
    let store = test_store();
    let _ = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, None, None);
    assert!(result.contains("Decision #"));
    assert!(result.contains("WARNING"), "Expected warning about no causal upstream: {}", result);
    assert!(result.contains("experiment_id") && result.contains("finding_ids"),
        "Expected guidance to provide causal upstream: {}", result);
}

#[test]
fn test_decision_auto_creates_experiment_informed_edge() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), Some(exp_id), None, None);
    assert!(result.contains("Decision #"));
    // Should auto-create Experiment --Informed--> Decision
    assert!(result.contains("Experiment#") && result.contains("--Informed-->") && result.contains("Decision#"),
        "Expected Experiment --Informed--> Decision auto-edge in: {}", result);
    // No warning since experiment_id was provided
    assert!(!result.contains("WARNING"), "Should not warn when experiment_id provided: {}", result);
    // Verify edge in DB
    let decisions = store.list_decisions(0).unwrap();
    let did = decisions.last().unwrap().id;
    let edges = store.get_edges_to(NodeType::Decision, did).unwrap();
    let informed: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::Informed && e.source_type == NodeType::Experiment).collect();
    assert_eq!(informed.len(), 1);
    assert_eq!(informed[0].source_id, exp_id);
}

#[test]
fn test_decision_auto_creates_finding_informed_edges() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let f1 = store.create_finding(Some(exp_id), &text).unwrap();
    let f2 = store.create_finding(Some(exp_id), &text).unwrap();
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let fids = format!("{},{}", f1.id, f2.id);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), None, Some(&fids), None);
    assert!(result.contains("Decision #"));
    // Should auto-create Finding --Informed--> Decision for each
    assert!(result.contains(&format!("Finding#{}", f1.id)), "Expected finding #{} edge: {}", f1.id, result);
    assert!(result.contains(&format!("Finding#{}", f2.id)), "Expected finding #{} edge: {}", f2.id, result);
    // No warning since finding_ids was provided
    assert!(!result.contains("WARNING"), "Should not warn when finding_ids provided: {}", result);
}

#[test]
fn test_decision_causal_guidance_suggests_downstream() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), Some(exp_id), None, None);
    assert!(result.contains("=== Causal Links ==="), "Expected causal links: {}", result);
    assert!(result.contains("spawns a new experiment"), "Expected experiment downstream suggestion: {}", result);
    assert!(result.contains("updates a hypothesis"), "Expected hypothesis downstream suggestion: {}", result);
    assert!(result.contains("derives a principle"), "Expected principle downstream suggestion: {}", result);
}

#[test]
fn test_exp_complete_auto_creates_phase_contains_experiment_edge() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let result = super::nodes::tool_exp_complete(&store, exp_id, "pass", "result text", None);
    assert!(result.contains("Experiment #"));
    // Should auto-create Phase --Contains--> Experiment
    assert!(result.contains(&format!("Phase#{}", phase_id)) && result.contains("--Contains-->") && result.contains(&format!("Experiment#{}", exp_id)),
        "Expected Phase --Contains--> Experiment auto-edge in: {}", result);
}

#[test]
fn test_exp_complete_auto_creates_phase_finding_edges() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let result = super::nodes::tool_exp_complete(&store, exp_id, "pass", "result text", Some("A finding from the experiment"));
    assert!(result.contains("Finding #"));
    // Should auto-create Phase --Contains--> Finding
    assert!(result.contains(&format!("Phase#{}", phase_id)) && result.contains("--Contains--> Finding"),
        "Expected Phase --Contains--> Finding auto-edge in: {}", result);
    // Should auto-create Experiment --ProducedBy--> Finding
    assert!(result.contains(&format!("Experiment#{}", exp_id)) && result.contains("--ProducedBy-->"),
        "Expected Experiment --ProducedBy--> Finding auto-edge in: {}", result);
}

#[test]
fn test_exp_complete_causal_guidance() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let result = super::nodes::tool_exp_complete(&store, exp_id, "pass", "result text", None);
    assert!(result.contains("=== Causal Links ==="), "Expected causal links: {}", result);
    assert!(result.contains("tested a hypothesis"), "Expected hypothesis suggestion: {}", result);
    assert!(result.contains("warrants a decision"), "Expected decision suggestion: {}", result);
}

#[test]
fn test_hyp_update_testing_auto_creates_tested_by_edge() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Supporting evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    // Transition to testing with experiment_id
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "testing", Some(exp_id), None, None, None, None);
    assert!(result.contains("Testing"), "Expected Testing status: {}", result);
    // Should auto-create Experiment --TestedBy--> Hypothesis
    assert!(result.contains("TestedBy"), "Expected TestedBy auto-edge in: {}", result);
    // Verify edge in DB
    let edges = store.get_edges_to(NodeType::Hypothesis, hyp.id).unwrap();
    let tested: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::TestedBy).collect();
    assert_eq!(tested.len(), 1, "Expected 1 TestedBy edge, got {}", tested.len());
    assert_eq!(tested[0].source_id, exp_id);
}

#[test]
fn test_hyp_update_confirmed_auto_creates_supports_edge() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Supporting evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    store.update_hypothesis(hyp.id, HypothesisStatus::Testing, None, None).unwrap();
    // Confirm with finding_id
    let confirming = store.create_finding(Some(exp_id), "Confirming evidence").unwrap();
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "confirmed", None, Some(confirming.id), None, None, None);
    assert!(result.contains("Confirmed"), "Expected Confirmed status: {}", result);
    // Should auto-create Finding --Supports--> Hypothesis
    let edges = store.get_edges_to(NodeType::Hypothesis, hyp.id).unwrap();
    let supports: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::Supports).collect();
    // Should have 2 supports edges: original + confirmation
    assert_eq!(supports.len(), 2, "Expected 2 Supports edges (original + confirmation), got {}: {:?}", supports.len(), supports);
}

#[test]
fn test_hyp_update_testing_causal_guidance() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Supporting evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "testing", Some(exp_id), None, None, None, None);
    assert!(result.contains("=== Causal Links ==="), "Expected causal links: {}", result);
    assert!(result.contains("Log findings"), "Expected finding logging suggestion: {}", result);
    assert!(result.contains("confirms or refutes"), "Expected confirm/refute suggestion: {}", result);
}

#[test]
fn test_hyp_update_refuted_causal_guidance() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let hyp = store.create_hypothesis(Some(phase_id), "Test hypothesis").unwrap();
    let finding = store.create_finding(Some(exp_id), "Supporting evidence").unwrap();
    store.create_edge(NodeType::Finding, finding.id, NodeType::Hypothesis, hyp.id, EdgeType::Supports).unwrap();
    store.update_hypothesis(hyp.id, HypothesisStatus::Testing, None, None).unwrap();
    let disproving = store.create_finding(Some(exp_id), "Disproving evidence").unwrap();
    let result = super::nodes::tool_hyp_update(&store, hyp.id, "refuted", None, Some(disproving.id), None, None, None);
    assert!(result.contains("Refuted"), "Expected Refuted status: {}", result);
    assert!(result.contains("Contradicts"), "Expected Contradicts in auto-edges: {}", result);
    assert!(result.contains("new hypothesis"), "Expected new hypothesis suggestion: {}", result);
}

#[test]
fn test_research_complete_auto_creates_phase_contains_edge() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let research = store.create_research(Some(phase_id), "Research task").unwrap();
    let result = super::nodes::tool_research_complete(&store, research.id, "complete", Some("Report text"), Some(phase_id), None);
    assert!(result.contains("Research #"));
    // Should auto-create Phase --Contains--> Research
    assert!(result.contains("Phase#") && result.contains("--Contains-->") && result.contains("Research#"),
        "Expected Phase --Contains--> Research auto-edge in: {}", result);
    // Verify edge in DB
    let edges = store.get_edges_to(NodeType::Research, research.id).unwrap();
    let contains: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::Contains).collect();
    assert_eq!(contains.len(), 1);
    assert_eq!(contains[0].source_id, phase_id);
}

#[test]
fn test_research_complete_auto_derives_phase_from_record() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let research = store.create_research(Some(phase_id), "Research task").unwrap();
    // Don't pass phase_id — should auto-derive from research.phase_id
    let result = super::nodes::tool_research_complete(&store, research.id, "complete", Some("Report text"), None, None);
    assert!(result.contains("Research #"));
    // Should still auto-create Phase --Contains--> Research from the record's phase_id
    let edges = store.get_edges_to(NodeType::Research, research.id).unwrap();
    let contains: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::Contains).collect();
    assert_eq!(contains.len(), 1, "Expected phase edge derived from research record");
}

#[test]
fn test_research_complete_auto_creates_finding_informed_edges() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let f1 = store.create_finding(Some(exp_id), &text).unwrap();
    let f2 = store.create_finding(Some(exp_id), &text).unwrap();
    let research = store.create_research(Some(phase_id), "Research task").unwrap();
    let fids = format!("{},{}", f1.id, f2.id);
    let result = super::nodes::tool_research_complete(&store, research.id, "complete", Some("Report"), Some(phase_id), Some(&fids));
    // Should auto-create Finding --Informed--> Research for each
    assert!(result.contains(&format!("Finding#{}", f1.id)), "Expected finding #{} edge: {}", f1.id, result);
    assert!(result.contains(&format!("Finding#{}", f2.id)), "Expected finding #{} edge: {}", f2.id, result);
    // Verify edges in DB
    let edges = store.get_edges_to(NodeType::Research, research.id).unwrap();
    let informed: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::Informed).collect();
    assert_eq!(informed.len(), 2, "Expected 2 Finding --Informed--> Research edges");
}

#[test]
fn test_research_complete_causal_guidance() {
    let store = test_store();
    let (_, phase_id, _) = setup_project(&store);
    let research = store.create_research(Some(phase_id), "Research task").unwrap();
    let result = super::nodes::tool_research_complete(&store, research.id, "complete", Some("Report"), None, None);
    assert!(result.contains("=== Causal Links ==="), "Expected causal links: {}", result);
    assert!(result.contains("produced findings"), "Expected findings suggestion: {}", result);
    assert!(result.contains("informs a decision"), "Expected decision suggestion: {}", result);
}

#[test]
fn test_constraint_add_auto_creates_tested_by_edge() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "hardware",
        "text": &"a".repeat(60),
        "source": "benchmark",
        "experiment_id": exp_id
    });
    let result = super::nodes::tool_constraint_add(&store, &args);
    assert!(result.contains("Constraint #"));
    // Should auto-create Experiment --TestedBy--> Constraint
    assert!(result.contains("TestedBy"), "Expected TestedBy auto-edge in: {}", result);
    // Verify edge in DB
    let constraints = store.list_constraints(1).unwrap();
    let cid = constraints.last().unwrap().id;
    let edges = store.get_edges_to(NodeType::Constraint, cid).unwrap();
    let tested: Vec<_> = edges.iter().filter(|e| e.relation == EdgeType::TestedBy).collect();
    assert_eq!(tested.len(), 1);
    assert_eq!(tested[0].source_id, exp_id);
}

#[test]
fn test_constraint_add_causal_guidance() {
    let store = test_store();
    let _ = setup_project(&store);
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "hardware",
        "text": &"a".repeat(60),
        "source": "test"
    });
    let result = super::nodes::tool_constraint_add(&store, &args);
    assert!(result.contains("Constraint #"));
    // Causal guidance section should be present even without auto-edges
    // (suggestions for pending experiments exist from setup_project)
    assert!(result.contains("=== Causal Links ===") || result.contains("Suggest:"),
        "Expected some edge guidance in: {}", result);
}

#[test]
fn test_decision_with_both_experiment_and_findings() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let f1 = store.create_finding(Some(exp_id), &text).unwrap();
    let why_text = "a".repeat(60);
    let what_text = "b".repeat(60);
    let fids = format!("{}", f1.id);
    let result = super::nodes::tool_decision(&store, &what_text, Some(&why_text), Some(exp_id), Some(&fids), None);
    assert!(result.contains("Decision #"));
    // Both edges should be auto-created
    assert!(result.contains("Experiment#") && result.contains("--Informed-->"),
        "Expected experiment informed edge: {}", result);
    assert!(result.contains(&format!("Finding#{}", f1.id)),
        "Expected finding informed edge: {}", result);
    // Should not warn since we have causal upstream
    assert!(!result.contains("WARNING"), "Should not warn: {}", result);
}

#[test]
fn test_parse_ids_helper() {
    // Verify comma-separated parsing works correctly
    let ids = super::nodes::parse_ids("1,2,3");
    assert_eq!(ids, vec![1, 2, 3]);
    let ids = super::nodes::parse_ids("42");
    assert_eq!(ids, vec![42]);
    let ids = super::nodes::parse_ids("");
    assert!(ids.is_empty());
    let ids = super::nodes::parse_ids("1, 2 , 3");
    assert_eq!(ids, vec![1, 2, 3]);
    let ids = super::nodes::parse_ids("abc,1,xyz");
    assert_eq!(ids, vec![1]);
}

#[test]
fn test_dispatch_decision_with_finding_ids() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let f1 = store.create_finding(Some(exp_id), &text).unwrap();
    let args = serde_json::json!({
        "what": "b".repeat(60),
        "why": "c".repeat(60),
        "finding_ids": format!("{}", f1.id)
    });
    let result = super::dispatch_tool(&store, "pm_decision", &args);
    assert!(result.contains("Decision #"), "result: {}", result);
    assert!(result.contains(&format!("Finding#{}", f1.id)), "Expected finding edge in dispatch: {}", result);
}

#[test]
fn test_dispatch_research_complete_with_phase_and_findings() {
    let store = test_store();
    let (_, phase_id, exp_id) = setup_project(&store);
    let text = "a".repeat(150);
    let f1 = store.create_finding(Some(exp_id), &text).unwrap();
    let research = store.create_research(Some(phase_id), "Research task").unwrap();
    let args = serde_json::json!({
        "research_id": research.id,
        "status": "complete",
        "report": "Done",
        "phase_id": phase_id,
        "finding_ids": format!("{}", f1.id)
    });
    let result = super::dispatch_tool(&store, "pm_research_complete", &args);
    assert!(result.contains("Research #"), "result: {}", result);
    assert!(result.contains("Phase#") && result.contains("--Contains-->"), "Expected phase edge in dispatch: {}", result);
    assert!(result.contains(&format!("Finding#{}", f1.id)), "Expected finding edge in dispatch: {}", result);
}

#[test]
fn test_dispatch_constraint_with_experiment_id() {
    let store = test_store();
    let (_, _, exp_id) = setup_project(&store);
    let args = serde_json::json!({
        "project": "test-project",
        "scope": "hardware",
        "text": &"a".repeat(60),
        "source": "benchmark",
        "experiment_id": exp_id
    });
    let result = super::dispatch_tool(&store, "pm_constraint_add", &args);
    assert!(result.contains("Constraint #"), "result: {}", result);
    assert!(result.contains("TestedBy"), "Expected TestedBy edge in dispatch: {}", result);
}

// === pm_search tests ===

#[test]
fn test_search_finds_matching_text() {
    let store = test_store();
    let (_proj_id, _phase_id, exp_id) = setup_project(&store);
    // Create a finding with distinctive text
    let text = "a]".to_string() + &"MoE GEMV kernel fusion achieves 40% speedup on V100 with quantized weights using mul_mat_vec_q Q4_K implementation".to_string();
    store.create_finding(Some(exp_id), &text).unwrap();
    let result = super::review::tool_search(&store, "MoE GEMV");
    assert!(result.contains("Search Results for"), "Expected header in: {}", result);
    assert!(result.contains("Finding #"), "Expected finding result in: {}", result);
    assert!(result.contains("MoE GEMV"), "Expected search text in excerpt: {}", result);
    assert!(result.contains("pm_kg_traverse"), "Expected traverse hint in: {}", result);
    assert!(result.contains("1 results found"), "Expected 1 result in: {}", result);
}

#[test]
fn test_search_returns_empty_for_no_match() {
    let store = test_store();
    let _ = setup_project(&store);
    let result = super::review::tool_search(&store, "xyzzy_nonexistent_garbage_12345");
    assert!(result.contains("No results found"), "Expected no results in: {}", result);
}
