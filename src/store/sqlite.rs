use super::*;
use rusqlite::Connection;

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        Self::new(":memory:")
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                alias TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
        ")?;
        Ok(())
    }
}

// Stub implementation — all methods todo!() for now
// Tests will drive the implementation
impl Store for SqliteStore {
    fn create_project(&self, _name: &str, _alias: Option<&str>) -> Result<Project> { todo!() }
    fn get_project(&self, _id: i64) -> Result<Project> { todo!() }
    fn list_projects(&self) -> Result<Vec<Project>> { todo!() }
    fn update_project_status(&self, _id: i64, _status: ProjectStatus) -> Result<()> { todo!() }
    fn create_phase(&self, _project_id: i64, _name: &str, _impact: i32, _depends_on: &[i64]) -> Result<Phase> { todo!() }
    fn get_phase(&self, _id: i64) -> Result<Phase> { todo!() }
    fn list_phases(&self, _project_id: i64) -> Result<Vec<Phase>> { todo!() }
    fn update_phase_status(&self, _id: i64, _status: PhaseStatus) -> Result<()> { todo!() }
    fn create_experiment(&self, _phase_id: Option<i64>, _name: &str) -> Result<Experiment> { todo!() }
    fn get_experiment(&self, _id: i64) -> Result<Experiment> { todo!() }
    fn list_experiments(&self, _phase_id: Option<i64>) -> Result<Vec<Experiment>> { todo!() }
    fn update_experiment_status(&self, _id: i64, _status: ExperimentStatus, _result: Option<&str>) -> Result<()> { todo!() }
    fn create_finding(&self, _experiment_id: Option<i64>, _text: &str) -> Result<Finding> { todo!() }
    fn get_finding(&self, _id: i64) -> Result<Finding> { todo!() }
    fn list_findings(&self, _experiment_id: Option<i64>) -> Result<Vec<Finding>> { todo!() }
    fn create_edge(&self, _st: NodeType, _si: i64, _tt: NodeType, _ti: i64, _rel: EdgeType) -> Result<Edge> { todo!() }
    fn get_edges_from(&self, _st: NodeType, _si: i64) -> Result<Vec<Edge>> { todo!() }
    fn get_edges_to(&self, _tt: NodeType, _ti: i64) -> Result<Vec<Edge>> { todo!() }
    fn create_decision(&self, _exp_id: Option<i64>, _what: &str, _why: Option<&str>) -> Result<Decision> { todo!() }
    fn list_decisions(&self, _project_id: i64) -> Result<Vec<Decision>> { todo!() }
}
