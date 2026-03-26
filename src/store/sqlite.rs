use super::*;
use rusqlite::{Connection, params};
use chrono::NaiveDateTime;

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        Self::new(":memory:")
    }

    fn now() -> String {
        chrono::Local::now().naive_local().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    fn parse_dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .unwrap_or_else(|_| chrono::Local::now().naive_local())
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                alias TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS phases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id),
                name TEXT NOT NULL,
                status TEXT NOT NULL,
                impact INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS phase_deps (
                phase_id INTEGER NOT NULL REFERENCES phases(id),
                depends_on_id INTEGER NOT NULL REFERENCES phases(id),
                PRIMARY KEY (phase_id, depends_on_id)
            );
            CREATE TABLE IF NOT EXISTS experiments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                phase_id INTEGER REFERENCES phases(id),
                name TEXT NOT NULL,
                status TEXT NOT NULL,
                hypothesis TEXT,
                result TEXT,
                notes TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS findings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                experiment_id INTEGER REFERENCES experiments(id),
                text TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_type TEXT NOT NULL,
                source_id INTEGER NOT NULL,
                target_type TEXT NOT NULL,
                target_id INTEGER NOT NULL,
                relation TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                experiment_id INTEGER REFERENCES experiments(id),
                what TEXT NOT NULL,
                why TEXT,
                created_at TEXT NOT NULL
            );"
        )?;
        Ok(())
    }

    fn parse_project_status(s: &str) -> ProjectStatus {
        match s {
            "active" => ProjectStatus::Active,
            "paused" => ProjectStatus::Paused,
            "archived" => ProjectStatus::Archived,
            _ => ProjectStatus::Active,
        }
    }

    fn project_status_str(s: &ProjectStatus) -> &'static str {
        match s {
            ProjectStatus::Active => "active",
            ProjectStatus::Paused => "paused",
            ProjectStatus::Archived => "archived",
        }
    }

    fn parse_phase_status(s: &str) -> PhaseStatus {
        match s {
            "pending" => PhaseStatus::Pending,
            "in_progress" => PhaseStatus::InProgress,
            "complete" => PhaseStatus::Complete,
            "deprioritized" => PhaseStatus::Deprioritized,
            "paused" => PhaseStatus::Paused,
            _ => PhaseStatus::Pending,
        }
    }

    fn phase_status_str(s: &PhaseStatus) -> &'static str {
        match s {
            PhaseStatus::Pending => "pending",
            PhaseStatus::InProgress => "in_progress",
            PhaseStatus::Complete => "complete",
            PhaseStatus::Deprioritized => "deprioritized",
            PhaseStatus::Paused => "paused",
        }
    }

    fn parse_exp_status(s: &str) -> ExperimentStatus {
        match s {
            "pending" => ExperimentStatus::Pending,
            "pass" => ExperimentStatus::Pass,
            "fail" => ExperimentStatus::Fail,
            "inconclusive" => ExperimentStatus::Inconclusive,
            _ => ExperimentStatus::Pending,
        }
    }

    fn exp_status_str(s: &ExperimentStatus) -> &'static str {
        match s {
            ExperimentStatus::Pending => "pending",
            ExperimentStatus::Pass => "pass",
            ExperimentStatus::Fail => "fail",
            ExperimentStatus::Inconclusive => "inconclusive",
        }
    }

    fn parse_node_type(s: &str) -> NodeType {
        match s {
            "Finding" => NodeType::Finding,
            "Experiment" => NodeType::Experiment,
            "Decision" => NodeType::Decision,
            "Literature" => NodeType::Literature,
            "Phase" => NodeType::Phase,
            _ => NodeType::Finding,
        }
    }

    fn parse_edge_type(s: &str) -> EdgeType {
        match s {
            "ProducedBy" => EdgeType::ProducedBy,
            "Informed" => EdgeType::Informed,
            "Supports" => EdgeType::Supports,
            "Contradicts" => EdgeType::Contradicts,
            "Supersedes" => EdgeType::Supersedes,
            "DependsOn" => EdgeType::DependsOn,
            "RelatedTo" => EdgeType::RelatedTo,
            "CitedIn" => EdgeType::CitedIn,
            _ => EdgeType::RelatedTo,
        }
    }
}

impl Store for SqliteStore {
    fn create_project(&self, name: &str, alias: Option<&str>) -> Result<Project> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO projects (name, alias, status, created_at) VALUES (?1, ?2, 'active', ?3)",
            params![name, alias, now],
        )?;
        self.get_project(self.conn.last_insert_rowid())
    }

    fn get_project(&self, id: i64) -> Result<Project> {
        self.conn.query_row(
            "SELECT id, name, alias, status, created_at FROM projects WHERE id = ?1",
            params![id],
            |row| Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                alias: row.get(2)?,
                status: Self::parse_project_status(&row.get::<_, String>(3)?),
                created_at: Self::parse_dt(&row.get::<_, String>(4)?),
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "project".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare("SELECT id, name, alias, status, created_at FROM projects ORDER BY id")?;
        let rows = stmt.query_map([], |row| Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            alias: row.get(2)?,
            status: SqliteStore::parse_project_status(&row.get::<_, String>(3)?),
            created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?),
        }))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn update_project_status(&self, id: i64, status: ProjectStatus) -> Result<()> {
        self.conn.execute("UPDATE projects SET status = ?1 WHERE id = ?2",
            params![Self::project_status_str(&status), id])?;
        Ok(())
    }

    fn create_phase(&self, project_id: i64, name: &str, impact: i32, depends_on: &[i64]) -> Result<Phase> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO phases (project_id, name, impact, status, created_at) VALUES (?1, ?2, ?3, 'pending', ?4)",
            params![project_id, name, impact, now],
        )?;
        let id = self.conn.last_insert_rowid();
        for dep in depends_on {
            self.conn.execute("INSERT INTO phase_deps (phase_id, depends_on_id) VALUES (?1, ?2)", params![id, dep])?;
        }
        self.get_phase(id)
    }

    fn get_phase(&self, id: i64) -> Result<Phase> {
        let phase = self.conn.query_row(
            "SELECT id, project_id, name, status, impact, created_at FROM phases WHERE id = ?1",
            params![id],
            |row| Ok(Phase {
                id: row.get(0)?,
                project_id: row.get(1)?,
                name: row.get(2)?,
                status: SqliteStore::parse_phase_status(&row.get::<_, String>(3)?),
                impact: row.get(4)?,
                depends_on: vec![],
                created_at: SqliteStore::parse_dt(&row.get::<_, String>(5)?),
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "phase".into(), id },
            o => StoreError::Db(o),
        })?;
        let mut stmt = self.conn.prepare("SELECT depends_on_id FROM phase_deps WHERE phase_id = ?1")?;
        let deps: Vec<i64> = stmt.query_map(params![id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Phase { depends_on: deps, ..phase })
    }

    fn list_phases(&self, project_id: i64) -> Result<Vec<Phase>> {
        let mut stmt = self.conn.prepare("SELECT id FROM phases WHERE project_id = ?1 ORDER BY id")?;
        let ids: Vec<i64> = stmt.query_map(params![project_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter().map(|id| self.get_phase(id)).collect()
    }

    fn update_phase_status(&self, id: i64, status: PhaseStatus) -> Result<()> {
        self.conn.execute("UPDATE phases SET status = ?1 WHERE id = ?2",
            params![Self::phase_status_str(&status), id])?;
        Ok(())
    }

    fn create_experiment(&self, phase_id: Option<i64>, name: &str) -> Result<Experiment> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO experiments (phase_id, name, status, created_at) VALUES (?1, ?2, 'pending', ?3)",
            params![phase_id, name, now],
        )?;
        self.get_experiment(self.conn.last_insert_rowid())
    }

    fn get_experiment(&self, id: i64) -> Result<Experiment> {
        self.conn.query_row(
            "SELECT id, phase_id, name, status, hypothesis, result, notes, created_at FROM experiments WHERE id = ?1",
            params![id],
            |row| Ok(Experiment {
                id: row.get(0)?,
                phase_id: row.get(1)?,
                name: row.get(2)?,
                status: SqliteStore::parse_exp_status(&row.get::<_, String>(3)?),
                hypothesis: row.get(4)?,
                result: row.get(5)?,
                notes: row.get(6)?,
                created_at: SqliteStore::parse_dt(&row.get::<_, String>(7)?),
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "experiment".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn list_experiments(&self, phase_id: Option<i64>) -> Result<Vec<Experiment>> {
        let ids: Vec<i64> = if let Some(pid) = phase_id {
            let mut s = self.conn.prepare("SELECT id FROM experiments WHERE phase_id = ?1 ORDER BY id")?;
            s.query_map(params![pid], |r| r.get(0))?.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut s = self.conn.prepare("SELECT id FROM experiments ORDER BY id")?;
            s.query_map([], |r| r.get(0))?.collect::<std::result::Result<Vec<_>, _>>()?
        };
        ids.into_iter().map(|id| self.get_experiment(id)).collect()
    }

    fn update_experiment_status(&self, id: i64, status: ExperimentStatus, result: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE experiments SET status = ?1, result = ?2 WHERE id = ?3",
            params![Self::exp_status_str(&status), result, id])?;
        Ok(())
    }

    fn create_finding(&self, experiment_id: Option<i64>, text: &str) -> Result<Finding> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO findings (experiment_id, text, created_at) VALUES (?1, ?2, ?3)",
            params![experiment_id, text, now],
        )?;
        self.get_finding(self.conn.last_insert_rowid())
    }

    fn get_finding(&self, id: i64) -> Result<Finding> {
        self.conn.query_row(
            "SELECT id, experiment_id, text, created_at FROM findings WHERE id = ?1",
            params![id],
            |row| Ok(Finding {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                text: row.get(2)?,
                created_at: SqliteStore::parse_dt(&row.get::<_, String>(3)?),
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "finding".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn list_findings(&self, experiment_id: Option<i64>) -> Result<Vec<Finding>> {
        let ids: Vec<i64> = if let Some(eid) = experiment_id {
            let mut s = self.conn.prepare("SELECT id FROM findings WHERE experiment_id = ?1 ORDER BY id")?;
            s.query_map(params![eid], |r| r.get(0))?.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut s = self.conn.prepare("SELECT id FROM findings ORDER BY id")?;
            s.query_map([], |r| r.get(0))?.collect::<std::result::Result<Vec<_>, _>>()?
        };
        ids.into_iter().map(|id| self.get_finding(id)).collect()
    }

    fn create_edge(&self, source_type: NodeType, source_id: i64, target_type: NodeType, target_id: i64, relation: EdgeType) -> Result<Edge> {
        let st = format!("{:?}", source_type);
        let tt = format!("{:?}", target_type);
        let rel = format!("{:?}", relation);
        self.conn.execute(
            "INSERT INTO edges (source_type, source_id, target_type, target_id, relation) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![st, source_id, tt, target_id, rel],
        )?;
        Ok(Edge { id: self.conn.last_insert_rowid(), source_type, source_id, target_type, target_id, relation })
    }

    fn get_edges_from(&self, source_type: NodeType, source_id: i64) -> Result<Vec<Edge>> {
        let st = format!("{:?}", source_type);
        let mut stmt = self.conn.prepare(
            "SELECT id, source_type, source_id, target_type, target_id, relation FROM edges WHERE source_type = ?1 AND source_id = ?2"
        )?;
        let rows = stmt.query_map(params![st, source_id], |row| Ok(Edge {
            id: row.get(0)?,
            source_type: SqliteStore::parse_node_type(&row.get::<_, String>(1)?),
            source_id: row.get(2)?,
            target_type: SqliteStore::parse_node_type(&row.get::<_, String>(3)?),
            target_id: row.get(4)?,
            relation: SqliteStore::parse_edge_type(&row.get::<_, String>(5)?),
        }))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn get_edges_to(&self, target_type: NodeType, target_id: i64) -> Result<Vec<Edge>> {
        let tt = format!("{:?}", target_type);
        let mut stmt = self.conn.prepare(
            "SELECT id, source_type, source_id, target_type, target_id, relation FROM edges WHERE target_type = ?1 AND target_id = ?2"
        )?;
        let rows = stmt.query_map(params![tt, target_id], |row| Ok(Edge {
            id: row.get(0)?,
            source_type: SqliteStore::parse_node_type(&row.get::<_, String>(1)?),
            source_id: row.get(2)?,
            target_type: SqliteStore::parse_node_type(&row.get::<_, String>(3)?),
            target_id: row.get(4)?,
            relation: SqliteStore::parse_edge_type(&row.get::<_, String>(5)?),
        }))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn create_decision(&self, experiment_id: Option<i64>, what: &str, why: Option<&str>) -> Result<Decision> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO decisions (experiment_id, what, why, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![experiment_id, what, why, now],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Decision {
            id,
            experiment_id,
            what: what.to_string(),
            why: why.map(|s| s.to_string()),
            created_at: Self::parse_dt(&now),
        })
    }

    fn list_decisions(&self, _project_id: i64) -> Result<Vec<Decision>> {
        let mut stmt = self.conn.prepare("SELECT id, experiment_id, what, why, created_at FROM decisions ORDER BY id")?;
        let rows = stmt.query_map([], |row| Ok(Decision {
            id: row.get(0)?,
            experiment_id: row.get(1)?,
            what: row.get(2)?,
            why: row.get(3)?,
            created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?),
        }))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }
}
