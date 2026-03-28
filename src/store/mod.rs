use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found: {entity} #{id}")]
    NotFound { entity: String, id: i64 },
    #[error("constraint violation: {0}")]
    Constraint(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

// --- Core Types ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectStatus {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PhaseStatus {
    Pending,
    InProgress,
    Complete,
    Deprioritized,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExperimentStatus {
    Pending,
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PrincipleScope {
    Universal,
    Project,
    Phase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PrincipleStatus {
    Active,
    Superseded,
    Refined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HypothesisStatus {
    Proposed,
    Testing,
    Confirmed,
    Refuted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConstraintScope {
    Hardware,
    Software,
    Process,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FeedbackCategory {
    Correction,
    Confirmation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResearchStatus {
    Pending,
    InProgress,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EdgeType {
    ProducedBy,
    Informed,
    Supports,
    Contradicts,
    Supersedes,
    DependsOn,
    RelatedTo,
    CitedIn,
    Contains,
    DerivedFrom,
    TestedBy,
    ViolatedBy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    Finding,
    Experiment,
    Decision,
    Literature,
    Phase,
    Research,
    Principle,
    Hypothesis,
    Constraint,
    Feedback,
}

// --- Entity Structs ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub alias: Option<String>,
    pub status: ProjectStatus,
    pub parent_id: Option<i64>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub id: i64,
    pub project_id: i64,
    pub project_seq: Option<i64>,
    pub name: String,
    pub status: PhaseStatus,
    pub impact: i32,
    pub depends_on: Vec<i64>,
    pub description: Option<String>,
    pub goals: Option<String>,
    pub success_criteria: Option<String>,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: i64,
    pub phase_id: Option<i64>,
    pub project_seq: Option<i64>,
    pub name: String,
    pub status: ExperimentStatus,
    pub hypothesis: Option<String>,
    pub result: Option<String>,
    pub notes: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: i64,
    pub experiment_id: Option<i64>,
    pub project_seq: Option<i64>,
    pub text: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: i64,
    pub source_type: NodeType,
    pub source_id: i64,
    pub target_type: NodeType,
    pub target_id: i64,
    pub relation: EdgeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: i64,
    pub experiment_id: Option<i64>,
    pub project_id: Option<i64>,
    pub project_seq: Option<i64>,
    pub what: String,
    pub why: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Research {
    pub id: i64,
    pub phase_id: Option<i64>,
    pub project_seq: Option<i64>,
    pub name: String,
    pub report: Option<String>,
    pub status: ResearchStatus,
    pub created_at: NaiveDateTime,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principle {
    pub id: i64,
    pub project_id: i64,
    pub project_seq: Option<i64>,
    pub scope: PrincipleScope,
    pub text: String,
    pub status: PrincipleStatus,
    pub superseded_by: Option<i64>,
    pub rationale: Option<String>,
    pub enforcement_level: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: i64,
    pub phase_id: Option<i64>,
    pub project_seq: Option<i64>,
    pub text: String,
    pub status: HypothesisStatus,
    pub experiment_id: Option<i64>,
    pub finding_id: Option<i64>,
    pub prediction: Option<String>,
    pub criteria: Option<String>,
    pub confidence: Option<f64>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub id: i64,
    pub project_id: i64,
    pub project_seq: Option<i64>,
    pub scope: ConstraintScope,
    pub text: String,
    pub source: Option<String>,
    pub severity: Option<String>,
    pub resource: Option<String>,
    pub measured_value: Option<String>,
    pub expires_at: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteratureEntry {
    pub id: i64,
    pub project_id: i64,
    pub project_seq: Option<i64>,
    pub arxiv_id: Option<String>,
    pub title: String,
    pub authors: Option<String>,
    pub relevance: Option<String>,
    pub key_findings: Option<String>,
    pub url: Option<String>,
    pub venue: Option<String>,
    pub year: Option<i32>,
    pub code_url: Option<String>,
    pub file_path: Option<String>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEntry {
    pub id: i64,
    pub project_id: i64,
    pub project_seq: Option<i64>,
    pub text: String,
    pub category: FeedbackCategory,
    pub created_at: NaiveDateTime,
}


// --- Temporal Awareness Types (Feature 5) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    pub project_id: Option<i64>,
    pub started_at: NaiveDateTime,
    pub ended_at: Option<NaiveDateTime>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalDelta {
    pub since: String,
    pub phases: Vec<Phase>,
    pub experiments: Vec<Experiment>,
    pub findings: Vec<Finding>,
    pub decisions: Vec<Decision>,
    pub hypotheses: Vec<Hypothesis>,
    pub research: Vec<Research>,
    pub literature: Vec<LiteratureEntry>,
    pub principles: Vec<Principle>,
    pub constraints: Vec<Constraint>,
    pub feedback: Vec<FeedbackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StalenessReport {
    pub stale_hypotheses: Vec<(Hypothesis, i64)>,
    pub stale_experiments: Vec<(Experiment, i64)>,
    pub unconnected_findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityMetrics {
    pub findings_per_session: Vec<(i64, usize)>,
    pub experiments_per_week: Vec<(String, usize, usize)>,
    pub hypothesis_lifecycle_days: Vec<(i64, f64)>,
}

// --- Store Trait ---

pub trait Store {
    // Projects
    fn create_project(&self, name: &str, alias: Option<&str>, parent_id: Option<i64>) -> Result<Project>;
    fn get_project(&self, id: i64) -> Result<Project>;
    fn list_projects(&self) -> Result<Vec<Project>>;
    fn update_project_status(&self, id: i64, status: ProjectStatus) -> Result<()>;
    fn list_subprojects(&self, parent_id: i64) -> Result<Vec<Project>>;

    // Phases
    fn create_phase(&self, project_id: i64, name: &str, impact: i32, depends_on: &[i64]) -> Result<Phase>;
    fn get_phase(&self, id: i64) -> Result<Phase>;
    fn list_phases(&self, project_id: i64) -> Result<Vec<Phase>>;
    fn update_phase_status(&self, id: i64, status: PhaseStatus) -> Result<()>;

    // Experiments
    fn create_experiment(&self, phase_id: Option<i64>, name: &str) -> Result<Experiment>;
    fn get_experiment(&self, id: i64) -> Result<Experiment>;
    fn list_experiments(&self, phase_id: Option<i64>) -> Result<Vec<Experiment>>;
    fn update_experiment_status(&self, id: i64, status: ExperimentStatus, result: Option<&str>) -> Result<()>;

    // Findings
    fn create_finding(&self, experiment_id: Option<i64>, text: &str) -> Result<Finding>;
    fn get_finding(&self, id: i64) -> Result<Finding>;
    fn list_findings(&self, experiment_id: Option<i64>) -> Result<Vec<Finding>>;

    // Edges (KG)
    fn create_edge(&self, source_type: NodeType, source_id: i64, target_type: NodeType, target_id: i64, relation: EdgeType) -> Result<Edge>;
    fn get_edges_from(&self, source_type: NodeType, source_id: i64) -> Result<Vec<Edge>>;
    fn get_edges_to(&self, target_type: NodeType, target_id: i64) -> Result<Vec<Edge>>;
    fn list_all_edges(&self) -> Result<Vec<Edge>>;
    fn delete_edge(&self, id: i64) -> Result<()>;

    // Decisions
    fn create_decision(&self, experiment_id: Option<i64>, what: &str, why: Option<&str>, project_id: Option<i64>) -> Result<Decision>;
    fn list_decisions(&self, project_id: i64) -> Result<Vec<Decision>>;

    // Research
    fn create_research(&self, phase_id: Option<i64>, name: &str) -> Result<Research>;
    fn get_research(&self, id: i64) -> Result<Research>;
    fn list_research(&self, phase_id: Option<i64>) -> Result<Vec<Research>>;
    fn update_research(&self, id: i64, status: ResearchStatus, report: Option<&str>) -> Result<()>;
    // Principles
    fn create_principle(&self, project_id: i64, scope: PrincipleScope, text: &str, rationale: Option<&str>, enforcement_level: Option<&str>) -> Result<Principle>;
    fn list_principles(&self, project_id: i64) -> Result<Vec<Principle>>;
    fn update_principle_status(&self, id: i64, status: PrincipleStatus, superseded_by: Option<i64>) -> Result<()>;

    // Hypotheses
    fn create_hypothesis(&self, phase_id: Option<i64>, text: &str) -> Result<Hypothesis>;
    fn list_hypotheses(&self, phase_id: Option<i64>) -> Result<Vec<Hypothesis>>;
    fn update_hypothesis(&self, id: i64, status: HypothesisStatus, experiment_id: Option<i64>, finding_id: Option<i64>) -> Result<()>;

    // Constraints
    fn create_constraint(&self, project_id: i64, scope: ConstraintScope, text: &str, source: Option<&str>, severity: Option<&str>, resource: Option<&str>, measured_value: Option<&str>, expires_at: Option<&str>) -> Result<Constraint>;
    fn list_constraints(&self, project_id: i64) -> Result<Vec<Constraint>>;

    // Literature
    fn create_literature(&self, project_id: i64, title: &str, arxiv_id: Option<&str>, relevance: Option<&str>, key_findings: Option<&str>, authors: Option<&str>, venue: Option<&str>, year: Option<i32>, url: Option<&str>, code_url: Option<&str>, summary: Option<&str>) -> Result<LiteratureEntry>;
    fn update_literature_status(&self, id: i64, status: &str) -> Result<()>;
    fn list_literature(&self, project_id: i64) -> Result<Vec<LiteratureEntry>>;

    // Feedback
    fn create_feedback(&self, project_id: i64, text: &str, category: FeedbackCategory) -> Result<FeedbackEntry>;
    fn list_feedback(&self, project_id: i64) -> Result<Vec<FeedbackEntry>>;

    // Node existence check
    fn node_exists(&self, node_type: &str, node_id: i64) -> Result<bool>;

    // Get-by-id methods
    fn get_decision(&self, id: i64) -> Result<Decision>;
    fn get_principle(&self, id: i64) -> Result<Principle>;
    fn get_hypothesis(&self, id: i64) -> Result<Hypothesis>;
    fn get_constraint(&self, id: i64) -> Result<Constraint>;
    fn get_literature(&self, id: i64) -> Result<LiteratureEntry>;
    fn get_feedback_entry(&self, id: i64) -> Result<FeedbackEntry>;

    // Phase field updates (#8)
    fn update_phase_fields(&self, id: i64, description: Option<&str>, goals: Option<&str>, success_criteria: Option<&str>) -> Result<()>;
    fn set_phase_started(&self, id: i64) -> Result<()>;
    fn set_phase_completed(&self, id: i64) -> Result<()>;

    // Hypothesis field updates (#9)
    fn update_hypothesis_fields(&self, id: i64, prediction: Option<&str>, criteria: Option<&str>, confidence: Option<f64>) -> Result<()>;

    // Orphan detection (#14)
    fn get_orphaned_nodes(&self, node_type: &str, project_id: i64) -> Result<Vec<i64>>;

    // Per-project ordinal resolution
    fn get_project_by_name(&self, name: &str) -> Result<Project>;
    fn resolve_node_id(&self, table: &str, seq: i64, project_name: Option<&str>) -> Result<i64>;

    // --- Temporal Awareness (Feature 5) ---
    fn create_session(&self, project_id: Option<i64>) -> Result<Session>;
    fn end_session(&self, id: i64, summary: Option<&str>) -> Result<()>;
    fn list_sessions(&self, project_id: Option<i64>) -> Result<Vec<Session>>;
    fn get_current_session(&self) -> Result<Option<Session>>;
    fn nodes_since(&self, timestamp: &str) -> Result<TemporalDelta>;
    fn staleness_report(&self, project_id: i64) -> Result<StalenessReport>;
    fn get_velocity(&self, project_id: i64) -> Result<VelocityMetrics>;
}

#[cfg(test)]
mod tests;
pub mod sqlite;
pub mod migrations;
