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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    Finding,
    Experiment,
    Decision,
    Literature,
    Phase,
    Research,
}

// --- Entity Structs ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub alias: Option<String>,
    pub status: ProjectStatus,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub status: PhaseStatus,
    pub impact: i32,
    pub depends_on: Vec<i64>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: i64,
    pub phase_id: Option<i64>,
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
    pub what: String,
    pub why: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Research {
    pub id: i64,
    pub phase_id: Option<i64>,
    pub name: String,
    pub report: Option<String>,
    pub status: ResearchStatus,
    pub created_at: NaiveDateTime,
}

// --- Store Trait ---

pub trait Store {
    // Projects
    fn create_project(&self, name: &str, alias: Option<&str>) -> Result<Project>;
    fn get_project(&self, id: i64) -> Result<Project>;
    fn list_projects(&self) -> Result<Vec<Project>>;
    fn update_project_status(&self, id: i64, status: ProjectStatus) -> Result<()>;

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
    fn create_decision(&self, experiment_id: Option<i64>, what: &str, why: Option<&str>) -> Result<Decision>;
    fn list_decisions(&self, project_id: i64) -> Result<Vec<Decision>>;

    // Research
    fn create_research(&self, phase_id: Option<i64>, name: &str) -> Result<Research>;
    fn get_research(&self, id: i64) -> Result<Research>;
    fn list_research(&self, phase_id: Option<i64>) -> Result<Vec<Research>>;
    fn update_research(&self, id: i64, status: ResearchStatus, report: Option<&str>) -> Result<()>;
}

#[cfg(test)]
mod tests;
pub mod sqlite;
