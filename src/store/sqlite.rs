use super::*;
use super::migrations;
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
        if let Err(e) = migrations::migrate(&store.conn) {
            return Err(StoreError::Db(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                Some(format!("migration failed: {}", e)),
            )));
        }
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
            CREATE TABLE IF NOT EXISTS principles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id),
                scope TEXT NOT NULL,
                text TEXT NOT NULL,
                status TEXT NOT NULL,
                superseded_by INTEGER,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS hypotheses (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                phase_id INTEGER REFERENCES phases(id),
                text TEXT NOT NULL,
                status TEXT NOT NULL,
                experiment_id INTEGER,
                finding_id INTEGER,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS constraints_tbl (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id),
                scope TEXT NOT NULL,
                text TEXT NOT NULL,
                source TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS literature (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id),
                arxiv_id TEXT,
                title TEXT NOT NULL,
                authors TEXT,
                relevance TEXT,
                key_findings TEXT,
                url TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS feedback (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id),
                text TEXT NOT NULL,
                category TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                experiment_id INTEGER REFERENCES experiments(id),
                what TEXT NOT NULL,
                why TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS research (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                phase_id INTEGER REFERENCES phases(id),
                name TEXT NOT NULL,
                report TEXT,
                status TEXT NOT NULL,
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

    fn parse_research_status(s: &str) -> ResearchStatus {
        match s {
            "pending" => ResearchStatus::Pending,
            "in_progress" => ResearchStatus::InProgress,
            "complete" => ResearchStatus::Complete,
            _ => ResearchStatus::Pending,
        }
    }

    fn research_status_str(s: &ResearchStatus) -> &'static str {
        match s {
            ResearchStatus::Pending => "pending",
            ResearchStatus::InProgress => "in_progress",
            ResearchStatus::Complete => "complete",
        }
    }

    fn parse_node_type(s: &str) -> NodeType {
        match s {
            "Finding" => NodeType::Finding,
            "Experiment" => NodeType::Experiment,
            "Decision" => NodeType::Decision,
            "Literature" => NodeType::Literature,
            "Phase" => NodeType::Phase,
            "Research" => NodeType::Research,
            "Principle" => NodeType::Principle,
            "Hypothesis" => NodeType::Hypothesis,
            "Constraint" => NodeType::Constraint,
            "Feedback" => NodeType::Feedback,
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
            "Contains" => EdgeType::Contains,
            "DerivedFrom" => EdgeType::DerivedFrom,
            "TestedBy" => EdgeType::TestedBy,
            "ViolatedBy" => EdgeType::ViolatedBy,
            "BranchesFrom" => EdgeType::BranchesFrom,
            "ConvergesInto" => EdgeType::ConvergesInto,
            _ => EdgeType::RelatedTo,
        }
    }


    /// Compute the next project_seq for a table with direct project_id.
    pub fn next_project_seq(&self, table: &str, project_id: i64) -> Result<i64> {
        let sql = format!(
            "SELECT COALESCE(MAX(project_seq), 0) + 1 FROM {} WHERE project_id = ?1",
            table
        );
        self.conn.query_row(&sql, params![project_id], |r| r.get(0))
            .map_err(StoreError::Db)
    }

    /// Compute the next project_seq for a table linked through phases (experiment, hypothesis, research).
    pub fn next_project_seq_via_phase(&self, table: &str, phase_id: i64) -> Result<i64> {
        let project_id: i64 = self.conn.query_row(
            "SELECT project_id FROM phases WHERE id = ?1",
            params![phase_id], |r| r.get(0)
        ).map_err(StoreError::Db)?;
        // Count existing nodes for this project in this table, joined through phases
        let sql = format!(
            "SELECT COALESCE(MAX(t.project_seq), 0) + 1 FROM {} t JOIN phases p ON t.phase_id = p.id WHERE p.project_id = ?1",
            table
        );
        self.conn.query_row(&sql, params![project_id], |r| r.get(0))
            .map_err(StoreError::Db)
    }

    /// Compute the next project_seq for findings (linked through experiment -> phase -> project).
    pub fn next_finding_project_seq(&self, experiment_id: i64) -> Result<i64> {
        let project_id: i64 = self.conn.query_row(
            "SELECT p.project_id FROM experiments e JOIN phases p ON e.phase_id = p.id WHERE e.id = ?1",
            params![experiment_id], |r| r.get(0)
        ).map_err(StoreError::Db)?;
        let sql = "SELECT COALESCE(MAX(f.project_seq), 0) + 1 FROM findings f                    JOIN experiments e ON f.experiment_id = e.id                    JOIN phases p ON e.phase_id = p.id                    WHERE p.project_id = ?1";
        self.conn.query_row(sql, params![project_id], |r| r.get(0))
            .map_err(StoreError::Db)
    }

    /// Format a node reference with project alias and project_seq.
    /// Returns e.g. "[VR] Phase #3" or "Phase #17" if no project context.
    pub fn format_node_ref(&self, node_type: &str, global_id: i64, project_seq: Option<i64>) -> String {
        if let Some(seq) = project_seq {
            // Try to get project alias for context
            let alias = self.get_project_alias_for_node(node_type, global_id);
            if let Some(a) = alias {
                return format!("[{}] {} #{}", a, node_type, seq);
            }
            return format!("{} #{}", node_type, seq);
        }
        format!("{} #{}", node_type, global_id)
    }

    /// Get project alias for a node by looking up the FK chain.
    fn get_project_alias_for_node(&self, node_type: &str, node_id: i64) -> Option<String> {
        let project_id: Option<i64> = match node_type {
            "Phase" | "Principle" | "Constraint" | "Literature" | "Feedback" | "Decision" => {
                let table = match node_type {
                    "Phase" => "phases",
                    "Principle" => "principles",
                    "Constraint" => "constraints_tbl",
                    "Literature" => "literature",
                    "Feedback" => "feedback",
                    "Decision" => "decisions",
                    _ => return None,
                };
                self.conn.query_row(
                    &format!("SELECT project_id FROM {} WHERE id = ?1", table),
                    params![node_id], |r| r.get(0)
                ).ok()
            }
            "Experiment" | "Hypothesis" | "Research" => {
                let table = match node_type {
                    "Experiment" => "experiments",
                    "Hypothesis" => "hypotheses",
                    "Research" => "research",
                    _ => return None,
                };
                self.conn.query_row(
                    &format!("SELECT p.project_id FROM {} t JOIN phases p ON t.phase_id = p.id WHERE t.id = ?1", table),
                    params![node_id], |r| r.get(0)
                ).ok()
            }
            "Finding" => {
                self.conn.query_row(
                    "SELECT p.project_id FROM findings f JOIN experiments e ON f.experiment_id = e.id JOIN phases p ON e.phase_id = p.id WHERE f.id = ?1",
                    params![node_id], |r| r.get(0)
                ).ok()
            }
            _ => None,
        };
        if let Some(pid) = project_id {
            self.conn.query_row(
                "SELECT COALESCE(alias, name) FROM projects WHERE id = ?1",
                params![pid], |r| r.get::<_, String>(0)
            ).ok()
        } else {
            None
        }
    }
    pub fn node_exists_check(&self, node_type: &str, node_id: i64) -> Result<bool> {
        let table = match node_type {
            "Finding" => "findings",
            "Experiment" => "experiments",
            "Decision" => "decisions",
            "Phase" => "phases",
            "Research" => "research",
            "Principle" => "principles",
            "Hypothesis" => "hypotheses",
            "Constraint" => "constraints_tbl",
            "Literature" => "literature",
            "Feedback" => "feedback",
            _ => return Err(StoreError::Constraint(format!("Unknown node type: {}", node_type))),
        };
        let sql = format!("SELECT EXISTS(SELECT 1 FROM {} WHERE id = ?1)", table);
        self.conn.query_row(&sql, params![node_id], |row| row.get(0))
            .map_err(StoreError::Db)
    }

    /// Get modified_at for a row in a table (test helper / MCP use).
    pub fn get_modified_at(&self, table: &str, id: i64) -> Result<Option<String>> {
        let sql = format!("SELECT modified_at FROM {} WHERE id = ?1", table);
        self.conn.query_row(&sql, params![id], |row| row.get(0))
            .map_err(StoreError::Db)
    }

    /// Backdate created_at by `days` for testing staleness. Also backdates modified_at.
    pub fn backdate_created_at(&self, table: &str, id: i64, days: i64) -> Result<()> {
        let sql = format!(
            "UPDATE {} SET created_at = datetime(created_at, '-{} days'), modified_at = datetime(modified_at, '-{} days') WHERE id = ?1",
            table, days, days
        );
        self.conn.execute(&sql, params![id])?;
        Ok(())
    }
    /// Count total edges (both directions) for a node. Used by search ranking.
    pub fn count_edges_for_node(&self, node_type: &str, node_id: i64) -> Result<i64> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FROM edges WHERE (source_type = ?1 AND source_id = ?2) OR (target_type = ?1 AND target_id = ?2)"
        )?;
        let count: i64 = stmt.query_row(params![node_type, node_id, node_type, node_id], |row| row.get(0))?;
        Ok(count)
    }

    /// Evidence weight: count Supports edges minus Contradicts edges pointing TO this node.
    pub fn evidence_weight_for_node(&self, node_type: &str, node_id: i64) -> Result<i64> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(SUM(CASE WHEN relation = 'Supports' THEN 1 WHEN relation = 'Contradicts' THEN -1 ELSE 0 END), 0) FROM edges WHERE target_type = ?1 AND target_id = ?2"
        )?;
        let weight: i64 = stmt.query_row(params![node_type, node_id], |row| row.get(0))?;
        Ok(weight)
    }



    // ====== TMS (Truth-Maintenance System) Methods ======

    /// Map a NodeType string to its table name.
    fn node_type_to_table(node_type: &str) -> Option<&'static str> {
        match node_type {
            "Finding" => Some("findings"),
            "Decision" => Some("decisions"),
            "Hypothesis" => Some("hypotheses"),
            "Principle" => Some("principles"),
            "Constraint" => Some("constraints_tbl"),
            _ => None,
        }
    }

    /// Update confidence for a node.
    pub fn update_confidence(&self, node_type: &str, node_id: i64, confidence: f64) -> Result<()> {
        let table = Self::node_type_to_table(node_type)
            .ok_or_else(|| StoreError::Constraint(format!("Node type {} does not support confidence", node_type)))?;
        let sql = format!("UPDATE {} SET confidence = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", table);
        self.conn.execute(&sql, params![confidence, node_id])?;
        Ok(())
    }

    /// Update belief status for a node.
    pub fn update_belief_status(&self, node_type: &str, node_id: i64, status: &str) -> Result<()> {
        if !["believed", "suspended", "retracted"].contains(&status) {
            return Err(StoreError::Constraint(format!("Invalid belief status: {}. Must be believed, suspended, or retracted.", status)));
        }
        let table = Self::node_type_to_table(node_type)
            .ok_or_else(|| StoreError::Constraint(format!("Node type {} does not support belief_status", node_type)))?;
        let sql = format!("UPDATE {} SET belief_status = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", table);
        self.conn.execute(&sql, params![status, node_id])?;
        Ok(())
    }

    /// Get current confidence of a node.
    pub fn get_confidence(&self, node_type: &str, node_id: i64) -> Result<Option<f64>> {
        let table = Self::node_type_to_table(node_type)
            .ok_or_else(|| StoreError::Constraint(format!("Node type {} does not support confidence", node_type)))?;
        let sql = format!("SELECT confidence FROM {} WHERE id = ?1", table);
        self.conn.query_row(&sql, params![node_id], |row| row.get(0))
            .map_err(StoreError::Db)
    }

    /// Get downstream dependents of a node by traversing forward through
    /// Informed, Supports, DerivedFrom edges (BFS).
    pub fn get_downstream_dependents(&self, node_type: &NodeType, node_id: i64) -> Result<Vec<(String, i64)>> {
        let forward_relations = ["Informed", "Supports", "DerivedFrom"];
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let mut dependents = Vec::new();

        let start_key = (format!("{:?}", node_type), node_id);
        visited.insert(start_key.clone());
        queue.push_back((format!("{:?}", node_type), node_id));

        while let Some((nt_str, nid)) = queue.pop_front() {
            // Find edges where this node is the SOURCE and relation is a dependency type
            let mut stmt = self.conn.prepare(
                "SELECT target_type, target_id, relation FROM edges WHERE source_type = ?1 AND source_id = ?2"
            )?;
            let edges: Vec<(String, i64, String)> = stmt.query_map(params![nt_str, nid], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
            })?.collect::<std::result::Result<Vec<_>, _>>()?;

            for (target_type, target_id, relation) in edges {
                if !forward_relations.contains(&relation.as_str()) {
                    continue;
                }
                let key = (target_type.clone(), target_id);
                if !visited.contains(&key) {
                    visited.insert(key);
                    dependents.push((target_type.clone(), target_id));
                    queue.push_back((target_type, target_id));
                }
            }
        }

        Ok(dependents)
    }

    /// Count Supports edges pointing TO a node.
    pub fn count_supports_edges(&self, node_type: &str, node_id: i64) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE target_type = ?1 AND target_id = ?2 AND relation = 'Supports'",
            params![node_type, node_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Create an edge with TMS side effects (confidence propagation + belief revision).
    pub fn create_edge_with_tms(
        &self,
        source_type: NodeType,
        source_id: i64,
        target_type: NodeType,
        target_id: i64,
        relation: EdgeType,
    ) -> Result<TmsEdgeResult> {
        // Create the edge first
        let edge = self.create_edge(source_type.clone(), source_id, target_type.clone(), target_id, relation.clone())?;

        let mut suspended_nodes = Vec::new();
        let mut confidence_updates = Vec::new();
        let target_type_str = format!("{:?}", target_type);

        match relation {
            EdgeType::Supports => {
                // Confidence propagation: count supports, update confidence
                // Formula: min(0.95, 0.3 + 0.1 * support_count)
                // Never decrease confidence from supports -- take max of current and formula
                if Self::node_type_to_table(&target_type_str).is_some() {
                    let support_count = self.count_supports_edges(&target_type_str, target_id)?;
                    let formula_conf = (0.3 + 0.1 * support_count as f64).min(0.95);
                    let current_conf = self.get_confidence(&target_type_str, target_id)?.unwrap_or(0.5);
                    let new_confidence = formula_conf.max(current_conf);
                    self.update_confidence(&target_type_str, target_id, new_confidence)?;
                    confidence_updates.push((target_type_str.clone(), target_id, new_confidence));
                }
            }
            EdgeType::Contradicts => {
                // Reduce target confidence by 0.2
                if Self::node_type_to_table(&target_type_str).is_some() {
                    let current_conf = self.get_confidence(&target_type_str, target_id)?.unwrap_or(0.5);
                    let new_confidence = (current_conf - 0.2).max(0.0);
                    self.update_confidence(&target_type_str, target_id, new_confidence)?;
                    confidence_updates.push((target_type_str.clone(), target_id, new_confidence));

                    // If confidence drops below 0.3, suspend the target
                    if new_confidence < 0.3 {
                        self.update_belief_status(&target_type_str, target_id, "suspended")?;
                        suspended_nodes.push((target_type_str.clone(), target_id));
                    }

                    // JTMS: suspend downstream dependents
                    let dependents = self.get_downstream_dependents(&target_type, target_id)?;
                    for (dep_type, dep_id) in &dependents {
                        if Self::node_type_to_table(dep_type).is_some() {
                            self.update_belief_status(dep_type, *dep_id, "suspended")?;
                            suspended_nodes.push((dep_type.clone(), *dep_id));
                        }
                    }
                }
            }
            _ => {
                // No TMS side effects for other relation types
            }
        }

        Ok(TmsEdgeResult {
            edge,
            suspended_nodes,
            confidence_updates,
        })
    }

}

impl Store for SqliteStore {
    fn create_project(&self, name: &str, alias: Option<&str>, parent_id: Option<i64>) -> Result<Project> {
        // Validate parent exists if specified
        if let Some(pid) = parent_id {
            self.get_project(pid).map_err(|_| StoreError::Constraint(
                format!("parent project #{} does not exist", pid)
            ))?;
        }
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO projects (name, alias, status, created_at, parent_id) VALUES (?1, ?2, 'active', ?3, ?4)",
            params![name, alias, now, parent_id],
        )?;
        self.get_project(self.conn.last_insert_rowid())
    }

    fn get_project(&self, id: i64) -> Result<Project> {
        self.conn.query_row(
            "SELECT id, name, alias, status, created_at, parent_id FROM projects WHERE id = ?1",
            params![id],
            |row| Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                alias: row.get(2)?,
                status: Self::parse_project_status(&row.get::<_, String>(3)?),
                created_at: Self::parse_dt(&row.get::<_, String>(4)?),
                parent_id: row.get(5)?,
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "project".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare("SELECT id, name, alias, status, created_at, parent_id FROM projects ORDER BY id")?;
        let rows = stmt.query_map([], |row| Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            alias: row.get(2)?,
            status: SqliteStore::parse_project_status(&row.get::<_, String>(3)?),
            created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?),
            parent_id: row.get(5)?,
        }))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn list_subprojects(&self, parent_id: i64) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare("SELECT id, name, alias, status, created_at, parent_id FROM projects WHERE parent_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![parent_id], |row| Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            alias: row.get(2)?,
            status: SqliteStore::parse_project_status(&row.get::<_, String>(3)?),
            created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?),
            parent_id: row.get(5)?,
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
        let seq = self.next_project_seq("phases", project_id)?;
        self.conn.execute(
            "INSERT INTO phases (project_id, name, impact, status, created_at, project_seq, modified_at) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?4)",
            params![project_id, name, impact, now, seq],
        )?;
        let id = self.conn.last_insert_rowid();
        for dep in depends_on {
            self.conn.execute("INSERT INTO phase_deps (phase_id, depends_on_id) VALUES (?1, ?2)", params![id, dep])?;
        }
        self.get_phase(id)
    }

    fn get_phase(&self, id: i64) -> Result<Phase> {
        let phase = self.conn.query_row(
            "SELECT id, project_id, name, status, impact, created_at, description, goals, success_criteria, started_at, completed_at, project_seq FROM phases WHERE id = ?1",
            params![id],
            |row| {
                let started_at: Option<String> = row.get(9)?;
                let completed_at: Option<String> = row.get(10)?;
                Ok(Phase {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    project_seq: row.get(11)?,
                    name: row.get(2)?,
                    status: SqliteStore::parse_phase_status(&row.get::<_, String>(3)?),
                    impact: row.get(4)?,
                    depends_on: vec![],
                    description: row.get(6)?,
                    goals: row.get(7)?,
                    success_criteria: row.get(8)?,
                    started_at: started_at.map(|s| SqliteStore::parse_dt(&s)),
                    completed_at: completed_at.map(|s| SqliteStore::parse_dt(&s)),
                    created_at: SqliteStore::parse_dt(&row.get::<_, String>(5)?),
                })
            },
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
        self.conn.execute("UPDATE phases SET status = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2",
            params![Self::phase_status_str(&status), id])?;
        Ok(())
    }

    fn create_experiment(&self, phase_id: Option<i64>, name: &str) -> Result<Experiment> {
        let now = Self::now();
        let seq = if let Some(pid) = phase_id {
            self.next_project_seq_via_phase("experiments", pid).ok()
        } else { None };
        self.conn.execute(
            "INSERT INTO experiments (phase_id, name, status, created_at, project_seq, modified_at) VALUES (?1, ?2, 'pending', ?3, ?4, ?3)",
            params![phase_id, name, now, seq],
        )?;
        self.get_experiment(self.conn.last_insert_rowid())
    }

    fn get_experiment(&self, id: i64) -> Result<Experiment> {
        self.conn.query_row(
            "SELECT id, phase_id, name, status, hypothesis, result, notes, created_at, project_seq FROM experiments WHERE id = ?1",
            params![id],
            |row| Ok(Experiment {
                id: row.get(0)?,
                phase_id: row.get(1)?,
                project_seq: row.get(8)?,
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
        self.conn.execute("UPDATE experiments SET status = ?1, result = ?2, modified_at = datetime('now', 'localtime') WHERE id = ?3",
            params![Self::exp_status_str(&status), result, id])?;
        Ok(())
    }

    fn create_finding(&self, experiment_id: Option<i64>, text: &str) -> Result<Finding> {
        let now = Self::now();
        let seq = if let Some(eid) = experiment_id {
            self.next_finding_project_seq(eid).ok()
        } else { None };
        self.conn.execute(
            "INSERT INTO findings (experiment_id, text, created_at, project_seq, modified_at, confidence, belief_status) VALUES (?1, ?2, ?3, ?4, ?3, 0.5, 'believed')",
            params![experiment_id, text, now, seq],
        )?;
        self.get_finding(self.conn.last_insert_rowid())
    }

    fn get_finding(&self, id: i64) -> Result<Finding> {
        self.conn.query_row(
            "SELECT id, experiment_id, text, created_at, project_seq, confidence, belief_status FROM findings WHERE id = ?1",
            params![id],
            |row| Ok(Finding {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                project_seq: row.get(4)?,
                text: row.get(2)?,
                created_at: SqliteStore::parse_dt(&row.get::<_, String>(3)?),
                confidence: row.get(5)?,
                belief_status: row.get(6)?,
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

        // Verify source node exists
        if !self.node_exists_check(&st, source_id)? {
            return Err(StoreError::Constraint(format!(
                "{} #{} does not exist", st, source_id
            )));
        }
        // Verify target node exists
        if !self.node_exists_check(&tt, target_id)? {
            return Err(StoreError::Constraint(format!(
                "{} #{} does not exist", tt, target_id
            )));
        }

        // Check for duplicate edge
        let existing = self.conn.query_row(
            "SELECT id FROM edges WHERE source_type = ?1 AND source_id = ?2 AND target_type = ?3 AND target_id = ?4 AND relation = ?5",
            params![st, source_id, tt, target_id, rel],
            |row| row.get::<_, i64>(0),
        );
        if let Ok(existing_id) = existing {
            // Return existing edge instead of creating duplicate
            return Ok(Edge { id: existing_id, source_type, source_id, target_type, target_id, relation });
        }

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
    fn list_all_edges(&self) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_type, source_id, target_type, target_id, relation FROM edges ORDER BY id"
        )?;
        let rows = stmt.query_map([], |row| Ok(Edge {
            id: row.get(0)?,
            source_type: SqliteStore::parse_node_type(&row.get::<_, String>(1)?),
            source_id: row.get(2)?,
            target_type: SqliteStore::parse_node_type(&row.get::<_, String>(3)?),
            target_id: row.get(4)?,
            relation: SqliteStore::parse_edge_type(&row.get::<_, String>(5)?),
        }))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn delete_edge(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM edges WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn create_decision(&self, experiment_id: Option<i64>, what: &str, why: Option<&str>, project_id: Option<i64>) -> Result<Decision> {
        let now = Self::now();
        let seq = if let Some(pid) = project_id {
            self.next_project_seq("decisions", pid).ok()
        } else { None };
        self.conn.execute(
            "INSERT INTO decisions (experiment_id, what, why, created_at, project_id, project_seq, modified_at, confidence, belief_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?4, 0.5, 'believed')",
            params![experiment_id, what, why, now, project_id, seq],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Decision {
            id,
            experiment_id,
            project_id,
            project_seq: seq,
            what: what.to_string(),
            why: why.map(|s| s.to_string()),
            created_at: Self::parse_dt(&now),
            confidence: Some(0.5),
            belief_status: Some("believed".to_string()),
        })
    }

    fn create_principle(&self, project_id: i64, scope: PrincipleScope, text: &str, rationale: Option<&str>, enforcement_level: Option<&str>) -> Result<Principle> {
        let now = Self::now();
        let s = match scope { PrincipleScope::Universal => "universal", PrincipleScope::Project => "project", PrincipleScope::Phase => "phase" };
        let el = enforcement_level.unwrap_or("advisory");
        let seq = self.next_project_seq("principles", project_id)?;
        self.conn.execute(
            "INSERT INTO principles (project_id, scope, text, status, rationale, enforcement_level, created_at, project_seq, modified_at, confidence, belief_status) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?6, 0.8, 'believed')",
            params![project_id, s, text, rationale, el, now, seq],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Principle { id, project_id, project_seq: Some(seq), scope, text: text.to_string(), status: PrincipleStatus::Active, superseded_by: None, rationale: rationale.map(|s| s.to_string()), enforcement_level: Some(el.to_string()), created_at: Self::parse_dt(&now), confidence: Some(0.8), belief_status: Some("believed".to_string()) })
    }
    fn list_principles(&self, project_id: i64) -> Result<Vec<Principle>> {
        let mut stmt = self.conn.prepare("SELECT id, project_id, scope, text, status, superseded_by, created_at, rationale, enforcement_level, project_seq, confidence, belief_status FROM principles WHERE project_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![project_id], |row| {
            let scope_str: String = row.get(2)?;
            let scope = match scope_str.as_str() { "universal" => PrincipleScope::Universal, "phase" => PrincipleScope::Phase, _ => PrincipleScope::Project };
            let status_str: String = row.get(4)?;
            let status = match status_str.as_str() { "superseded" => PrincipleStatus::Superseded, "refined" => PrincipleStatus::Refined, _ => PrincipleStatus::Active };
            Ok(Principle { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(9)?, scope, text: row.get(3)?, status, superseded_by: row.get(5)?, rationale: row.get(7)?, enforcement_level: row.get(8)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(6)?), confidence: row.get(10)?, belief_status: row.get(11)? })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }
    fn update_principle_status(&self, id: i64, status: PrincipleStatus, superseded_by: Option<i64>) -> Result<()> {
        let s = match status { PrincipleStatus::Active => "active", PrincipleStatus::Superseded => "superseded", PrincipleStatus::Refined => "refined" };
        self.conn.execute("UPDATE principles SET status = ?1, superseded_by = ?2, modified_at = datetime('now', 'localtime') WHERE id = ?3", params![s, superseded_by, id])?;
        Ok(())
    }

    fn create_hypothesis(&self, phase_id: Option<i64>, text: &str) -> Result<Hypothesis> {
        let now = Self::now();
        let seq = if let Some(pid) = phase_id {
            self.next_project_seq_via_phase("hypotheses", pid).ok()
        } else { None };
        self.conn.execute("INSERT INTO hypotheses (phase_id, text, status, created_at, project_seq, modified_at, confidence, belief_status) VALUES (?1, ?2, 'proposed', ?3, ?4, ?3, 0.3, 'believed')", params![phase_id, text, now, seq])?;
        let id = self.conn.last_insert_rowid();
        Ok(Hypothesis { id, phase_id, project_seq: seq, text: text.to_string(), status: HypothesisStatus::Proposed, experiment_id: None, finding_id: None, prediction: None, criteria: None, confidence: Some(0.3), belief_status: Some("believed".to_string()), created_at: Self::parse_dt(&now) })
    }
    fn list_hypotheses(&self, phase_id: Option<i64>) -> Result<Vec<Hypothesis>> {
        let sql = if phase_id.is_some() { "SELECT id, phase_id, text, status, experiment_id, finding_id, created_at, prediction, criteria, confidence, project_seq, belief_status FROM hypotheses WHERE phase_id = ?1 ORDER BY id" }
                  else { "SELECT id, phase_id, text, status, experiment_id, finding_id, created_at, prediction, criteria, confidence, project_seq, belief_status FROM hypotheses ORDER BY id" };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(pid) = phase_id {
            stmt.query_map(params![pid], |row| {
                let status_str: String = row.get(3)?;
                let status = match status_str.as_str() { "testing" => HypothesisStatus::Testing, "confirmed" => HypothesisStatus::Confirmed, "refuted" => HypothesisStatus::Refuted, _ => HypothesisStatus::Proposed };
                Ok(Hypothesis { id: row.get(0)?, phase_id: row.get(1)?, project_seq: row.get(10)?, text: row.get(2)?, status, experiment_id: row.get(4)?, finding_id: row.get(5)?, prediction: row.get(7)?, criteria: row.get(8)?, confidence: row.get(9)?, belief_status: row.get(11)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(6)?) })
            })?.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)?
        } else {
            stmt.query_map([], |row| {
                let status_str: String = row.get(3)?;
                let status = match status_str.as_str() { "testing" => HypothesisStatus::Testing, "confirmed" => HypothesisStatus::Confirmed, "refuted" => HypothesisStatus::Refuted, _ => HypothesisStatus::Proposed };
                Ok(Hypothesis { id: row.get(0)?, phase_id: row.get(1)?, project_seq: row.get(10)?, text: row.get(2)?, status, experiment_id: row.get(4)?, finding_id: row.get(5)?, prediction: row.get(7)?, criteria: row.get(8)?, confidence: row.get(9)?, belief_status: row.get(11)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(6)?) })
            })?.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)?
        };
        Ok(rows)
    }
    fn update_hypothesis(&self, id: i64, status: HypothesisStatus, experiment_id: Option<i64>, finding_id: Option<i64>) -> Result<()> {
        let s = match status { HypothesisStatus::Proposed => "proposed", HypothesisStatus::Testing => "testing", HypothesisStatus::Confirmed => "confirmed", HypothesisStatus::Refuted => "refuted" };
        self.conn.execute("UPDATE hypotheses SET status = ?1, experiment_id = ?2, finding_id = ?3, modified_at = datetime('now', 'localtime') WHERE id = ?4", params![s, experiment_id, finding_id, id])?;
        Ok(())
    }

    fn create_constraint(&self, project_id: i64, scope: ConstraintScope, text: &str, source: Option<&str>, severity: Option<&str>, resource: Option<&str>, measured_value: Option<&str>, expires_at: Option<&str>) -> Result<Constraint> {
        let now = Self::now();
        let s = match scope { ConstraintScope::Hardware => "hardware", ConstraintScope::Software => "software", ConstraintScope::Process => "process" };
        let sev = severity.unwrap_or("hard");
        let seq = self.next_project_seq("constraints_tbl", project_id)?;
        self.conn.execute(
            "INSERT INTO constraints_tbl (project_id, scope, text, source, severity, resource, measured_value, expires_at, created_at, project_seq, modified_at, confidence, belief_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?9, 0.9, 'believed')",
            params![project_id, s, text, source, sev, resource, measured_value, expires_at, now, seq],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Constraint { id, project_id, project_seq: Some(seq), scope, text: text.to_string(), source: source.map(|s| s.to_string()), severity: Some(sev.to_string()), resource: resource.map(|s| s.to_string()), measured_value: measured_value.map(|s| s.to_string()), expires_at: expires_at.map(|s| s.to_string()), created_at: Self::parse_dt(&now), confidence: Some(0.9), belief_status: Some("believed".to_string()) })
    }
    fn list_constraints(&self, project_id: i64) -> Result<Vec<Constraint>> {
        let mut stmt = self.conn.prepare("SELECT id, project_id, scope, text, source, created_at, severity, resource, measured_value, expires_at, project_seq, confidence, belief_status FROM constraints_tbl WHERE project_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![project_id], |row| {
            let scope_str: String = row.get(2)?;
            let scope = match scope_str.as_str() { "software" => ConstraintScope::Software, "process" => ConstraintScope::Process, _ => ConstraintScope::Hardware };
            Ok(Constraint { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(10)?, scope, text: row.get(3)?, source: row.get(4)?, severity: row.get(6)?, resource: row.get(7)?, measured_value: row.get(8)?, expires_at: row.get(9)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(5)?), confidence: row.get(11)?, belief_status: row.get(12)? })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn create_literature(&self, project_id: i64, title: &str, arxiv_id: Option<&str>, relevance: Option<&str>, key_findings: Option<&str>, authors: Option<&str>, venue: Option<&str>, year: Option<i32>, url: Option<&str>, code_url: Option<&str>, summary: Option<&str>) -> Result<LiteratureEntry> {
        let now = Self::now();
        let seq = self.next_project_seq("literature", project_id)?;
        self.conn.execute(
            "INSERT INTO literature (project_id, title, arxiv_id, relevance, key_findings, authors, venue, year, url, code_url, summary, status, created_at, project_seq, modified_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'unread', ?12, ?13, ?12)",
            params![project_id, title, arxiv_id, relevance, key_findings, authors, venue, year, url, code_url, summary, now, seq],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(LiteratureEntry {
            id, project_id, project_seq: Some(seq),
            arxiv_id: arxiv_id.map(|s| s.to_string()),
            title: title.to_string(),
            authors: authors.map(|s| s.to_string()),
            relevance: relevance.map(|s| s.to_string()),
            key_findings: key_findings.map(|s| s.to_string()),
            url: url.map(|s| s.to_string()),
            venue: venue.map(|s| s.to_string()),
            year,
            code_url: code_url.map(|s| s.to_string()),
            file_path: None,
            status: Some("unread".to_string()),
            summary: summary.map(|s| s.to_string()),
            created_at: Self::parse_dt(&now),
        })
    }
    fn update_literature_status(&self, id: i64, status: &str) -> Result<()> {
        let rows = self.conn.execute("UPDATE literature SET status = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", params![status, id])?;
        if rows == 0 {
            return Err(StoreError::NotFound { entity: "literature".into(), id });
        }
        Ok(())
    }
    fn list_literature(&self, project_id: i64) -> Result<Vec<LiteratureEntry>> {
        let mut stmt = self.conn.prepare("SELECT id, project_id, arxiv_id, title, authors, relevance, key_findings, url, created_at, venue, year, code_url, file_path, status, summary, project_seq FROM literature WHERE project_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(LiteratureEntry { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(15)?, arxiv_id: row.get(2)?, title: row.get(3)?, authors: row.get(4)?, relevance: row.get(5)?, key_findings: row.get(6)?, url: row.get(7)?, venue: row.get(9)?, year: row.get(10)?, code_url: row.get(11)?, file_path: row.get(12)?, status: row.get(13)?, summary: row.get(14)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(8)?) })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn create_feedback(&self, project_id: i64, text: &str, category: FeedbackCategory) -> Result<FeedbackEntry> {
        let now = Self::now();
        let c = match category { FeedbackCategory::Correction => "correction", FeedbackCategory::Confirmation => "confirmation" };
        let seq = self.next_project_seq("feedback", project_id)?;
        self.conn.execute("INSERT INTO feedback (project_id, text, category, created_at, project_seq, modified_at) VALUES (?1, ?2, ?3, ?4, ?5, ?4)", params![project_id, text, c, now, seq])?;
        let id = self.conn.last_insert_rowid();
        Ok(FeedbackEntry { id, project_id, project_seq: Some(seq), text: text.to_string(), category, created_at: Self::parse_dt(&now) })
    }
    fn list_feedback(&self, project_id: i64) -> Result<Vec<FeedbackEntry>> {
        let mut stmt = self.conn.prepare("SELECT id, project_id, text, category, created_at, project_seq FROM feedback WHERE project_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![project_id], |row| {
            let cat_str: String = row.get(3)?;
            let cat = match cat_str.as_str() { "confirmation" => FeedbackCategory::Confirmation, _ => FeedbackCategory::Correction };
            Ok(FeedbackEntry { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(5)?, text: row.get(2)?, category: cat, created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?) })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn list_decisions(&self, project_id: i64) -> Result<Vec<Decision>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id, d.experiment_id, d.what, d.why, d.created_at, d.project_id, d.project_seq, d.confidence, d.belief_status FROM decisions d WHERE d.project_id = ?1 ORDER BY d.id"
        )?;
        let rows = stmt.query_map([project_id], |row| Ok(Decision {
            id: row.get(0)?,
            experiment_id: row.get(1)?,
            project_seq: row.get(6)?,
            what: row.get(2)?,
            why: row.get(3)?,
            created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?),
            project_id: row.get(5)?,
            confidence: row.get(7)?,
            belief_status: row.get(8)?,
        }))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)
    }

    fn create_research(&self, phase_id: Option<i64>, name: &str) -> Result<Research> {
        let now = Self::now();
        let seq = if let Some(pid) = phase_id {
            self.next_project_seq_via_phase("research", pid).ok()
        } else { None };
        self.conn.execute(
            "INSERT INTO research (phase_id, name, status, created_at, project_seq, modified_at) VALUES (?1, ?2, 'pending', ?3, ?4, ?3)",
            params![phase_id, name, now, seq],
        )?;
        self.get_research(self.conn.last_insert_rowid())
    }

    fn get_research(&self, id: i64) -> Result<Research> {
        self.conn.query_row(
            "SELECT id, phase_id, name, report, status, created_at, project_seq FROM research WHERE id = ?1",
            params![id],
            |row| Ok(Research {
                id: row.get(0)?,
                phase_id: row.get(1)?,
                project_seq: row.get(6)?,
                name: row.get(2)?,
                report: row.get(3)?,
                status: SqliteStore::parse_research_status(&row.get::<_, String>(4)?),
                created_at: SqliteStore::parse_dt(&row.get::<_, String>(5)?),
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "research".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn list_research(&self, phase_id: Option<i64>) -> Result<Vec<Research>> {
        let ids: Vec<i64> = if let Some(pid) = phase_id {
            let mut s = self.conn.prepare("SELECT id FROM research WHERE phase_id = ?1 ORDER BY id")?;
            s.query_map(params![pid], |r| r.get(0))?.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut s = self.conn.prepare("SELECT id FROM research ORDER BY id")?;
            s.query_map([], |r| r.get(0))?.collect::<std::result::Result<Vec<_>, _>>()?
        };
        ids.into_iter().map(|id| self.get_research(id)).collect()
    }

    fn update_research(&self, id: i64, status: ResearchStatus, report: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE research SET status = ?1, report = ?2, modified_at = datetime('now', 'localtime') WHERE id = ?3",
            params![Self::research_status_str(&status), report, id])?;
        Ok(())
    }

    fn node_exists(&self, node_type: &str, node_id: i64) -> Result<bool> {
        self.node_exists_check(node_type, node_id)
    }

    fn get_decision(&self, id: i64) -> Result<Decision> {
        self.conn.query_row(
            "SELECT id, experiment_id, what, why, created_at, project_id, project_seq, confidence, belief_status FROM decisions WHERE id = ?1",
            params![id],
            |row| Ok(Decision {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                project_seq: row.get(6)?,
                what: row.get(2)?,
                why: row.get(3)?,
                created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?),
                project_id: row.get(5)?,
                confidence: row.get(7)?,
                belief_status: row.get(8)?,
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "decision".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn get_principle(&self, id: i64) -> Result<Principle> {
        self.conn.query_row(
            "SELECT id, project_id, scope, text, status, superseded_by, created_at, rationale, enforcement_level, project_seq, confidence, belief_status FROM principles WHERE id = ?1",
            params![id],
            |row| {
                let scope_str: String = row.get(2)?;
                let scope = match scope_str.as_str() { "universal" => PrincipleScope::Universal, "phase" => PrincipleScope::Phase, _ => PrincipleScope::Project };
                let status_str: String = row.get(4)?;
                let status = match status_str.as_str() { "superseded" => PrincipleStatus::Superseded, "refined" => PrincipleStatus::Refined, _ => PrincipleStatus::Active };
                Ok(Principle { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(9)?, scope, text: row.get(3)?, status, superseded_by: row.get(5)?, rationale: row.get(7)?, enforcement_level: row.get(8)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(6)?), confidence: row.get(10)?, belief_status: row.get(11)? })
            },
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "principle".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn get_hypothesis(&self, id: i64) -> Result<Hypothesis> {
        self.conn.query_row(
            "SELECT id, phase_id, text, status, experiment_id, finding_id, created_at, prediction, criteria, confidence, project_seq, belief_status FROM hypotheses WHERE id = ?1",
            params![id],
            |row| {
                let status_str: String = row.get(3)?;
                let status = match status_str.as_str() { "testing" => HypothesisStatus::Testing, "confirmed" => HypothesisStatus::Confirmed, "refuted" => HypothesisStatus::Refuted, _ => HypothesisStatus::Proposed };
                Ok(Hypothesis { id: row.get(0)?, phase_id: row.get(1)?, project_seq: row.get(10)?, text: row.get(2)?, status, experiment_id: row.get(4)?, finding_id: row.get(5)?, prediction: row.get(7)?, criteria: row.get(8)?, confidence: row.get(9)?, belief_status: row.get(11)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(6)?) })
            },
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "hypothesis".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn get_constraint(&self, id: i64) -> Result<Constraint> {
        self.conn.query_row(
            "SELECT id, project_id, scope, text, source, created_at, severity, resource, measured_value, expires_at, project_seq, confidence, belief_status FROM constraints_tbl WHERE id = ?1",
            params![id],
            |row| {
                let scope_str: String = row.get(2)?;
                let scope = match scope_str.as_str() { "software" => ConstraintScope::Software, "process" => ConstraintScope::Process, _ => ConstraintScope::Hardware };
                Ok(Constraint { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(10)?, scope, text: row.get(3)?, source: row.get(4)?, severity: row.get(6)?, resource: row.get(7)?, measured_value: row.get(8)?, expires_at: row.get(9)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(5)?), confidence: row.get(11)?, belief_status: row.get(12)? })
            },
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "constraint".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn get_literature(&self, id: i64) -> Result<LiteratureEntry> {
        self.conn.query_row(
            "SELECT id, project_id, arxiv_id, title, authors, relevance, key_findings, url, created_at, venue, year, code_url, file_path, status, summary, project_seq FROM literature WHERE id = ?1",
            params![id],
            |row| {
                Ok(LiteratureEntry { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(15)?, arxiv_id: row.get(2)?, title: row.get(3)?, authors: row.get(4)?, relevance: row.get(5)?, key_findings: row.get(6)?, url: row.get(7)?, venue: row.get(9)?, year: row.get(10)?, code_url: row.get(11)?, file_path: row.get(12)?, status: row.get(13)?, summary: row.get(14)?, created_at: SqliteStore::parse_dt(&row.get::<_, String>(8)?) })
            },
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "literature".into(), id },
            o => StoreError::Db(o),
        })
    }

    fn get_feedback_entry(&self, id: i64) -> Result<FeedbackEntry> {
        self.conn.query_row(
            "SELECT id, project_id, text, category, created_at, project_seq FROM feedback WHERE id = ?1",
            params![id],
            |row| {
                let cat_str: String = row.get(3)?;
                let cat = match cat_str.as_str() { "confirmation" => FeedbackCategory::Confirmation, _ => FeedbackCategory::Correction };
                Ok(FeedbackEntry { id: row.get(0)?, project_id: row.get(1)?, project_seq: row.get(5)?, text: row.get(2)?, category: cat, created_at: SqliteStore::parse_dt(&row.get::<_, String>(4)?) })
            },
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "feedback".into(), id },
            o => StoreError::Db(o),
        })
    }
    // --- Phase field updates (#8) ---

    fn update_phase_fields(&self, id: i64, description: Option<&str>, goals: Option<&str>, success_criteria: Option<&str>) -> Result<()> {
        if let Some(desc) = description {
            self.conn.execute("UPDATE phases SET description = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", params![desc, id])?;
        }
        if let Some(g) = goals {
            self.conn.execute("UPDATE phases SET goals = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", params![g, id])?;
        }
        if let Some(sc) = success_criteria {
            self.conn.execute("UPDATE phases SET success_criteria = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", params![sc, id])?;
        }
        Ok(())
    }

    fn set_phase_started(&self, id: i64) -> Result<()> {
        let now = Self::now();
        self.conn.execute("UPDATE phases SET started_at = ?1, modified_at = ?1 WHERE id = ?2", params![now, id])?;
        Ok(())
    }

    fn set_phase_completed(&self, id: i64) -> Result<()> {
        let now = Self::now();
        self.conn.execute("UPDATE phases SET completed_at = ?1, modified_at = ?1 WHERE id = ?2", params![now, id])?;
        Ok(())
    }

    // --- Hypothesis field updates (#9) ---

    fn update_hypothesis_fields(&self, id: i64, prediction: Option<&str>, criteria: Option<&str>, confidence: Option<f64>) -> Result<()> {
        if let Some(p) = prediction {
            self.conn.execute("UPDATE hypotheses SET prediction = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", params![p, id])?;
        }
        if let Some(c) = criteria {
            self.conn.execute("UPDATE hypotheses SET criteria = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", params![c, id])?;
        }
        if let Some(conf) = confidence {
            self.conn.execute("UPDATE hypotheses SET confidence = ?1, modified_at = datetime('now', 'localtime') WHERE id = ?2", params![conf, id])?;
        }
        Ok(())
    }

    fn get_project_by_name(&self, name: &str) -> Result<Project> {
        self.conn.query_row(
            "SELECT id, name, alias, status, created_at, parent_id FROM projects WHERE name = ?1 OR alias = ?1",
            params![name],
            |row| Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                alias: row.get(2)?,
                status: Self::parse_project_status(&row.get::<_, String>(3)?),
                created_at: Self::parse_dt(&row.get::<_, String>(4)?),
                parent_id: row.get(5)?,
            }),
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: "project".into(), id: 0 },
            o => StoreError::Db(o),
        })
    }

    fn resolve_node_id(&self, table: &str, seq: i64, project_name: Option<&str>) -> Result<i64> {
        if let Some(proj_name) = project_name {
            let project = self.get_project_by_name(proj_name)?;
            // Tables with direct project_id
            let direct_tables = ["phases", "literature", "principles", "constraints_tbl", "feedback", "decisions"];
            if direct_tables.contains(&table) {
                let sql = format!("SELECT id FROM {} WHERE project_id = ?1 AND project_seq = ?2", table);
                return self.conn.query_row(&sql, params![project.id, seq], |r| r.get(0))
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: table.into(), id: seq },
                        o => StoreError::Db(o),
                    });
            }
            // Tables linked through phases
            let phase_linked = ["experiments", "hypotheses", "research"];
            if phase_linked.contains(&table) {
                let sql = format!(
                    "SELECT t.id FROM {} t JOIN phases p ON t.phase_id = p.id WHERE p.project_id = ?1 AND t.project_seq = ?2",
                    table
                );
                return self.conn.query_row(&sql, params![project.id, seq], |r| r.get(0))
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: table.into(), id: seq },
                        o => StoreError::Db(o),
                    });
            }
            // Findings linked through experiments -> phases
            if table == "findings" {
                let sql = "SELECT f.id FROM findings f                            JOIN experiments e ON f.experiment_id = e.id                            JOIN phases p ON e.phase_id = p.id                            WHERE p.project_id = ?1 AND f.project_seq = ?2";
                return self.conn.query_row(sql, params![project.id, seq], |r| r.get(0))
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound { entity: table.into(), id: seq },
                        o => StoreError::Db(o),
                    });
            }
            Err(StoreError::Constraint(format!("Unknown table for resolve_node_id: {}", table)))
        } else {
            // Backward compatible: treat as global ID
            Ok(seq)
        }
    }

    fn get_orphaned_nodes(&self, node_type: &str, project_id: i64) -> Result<Vec<i64>> {
        let (table, id_col, project_filter) = match node_type {
            "finding" => {
                // Findings are scoped through experiments -> phases -> project
                let mut stmt = self.conn.prepare(
                    "SELECT f.id FROM findings f
                     LEFT JOIN experiments e ON f.experiment_id = e.id
                     LEFT JOIN phases p ON e.phase_id = p.id
                     WHERE (p.project_id = ?1 OR f.experiment_id IS NULL)
                     AND f.id NOT IN (
                         SELECT source_id FROM edges WHERE source_type = 'Finding'
                         UNION
                         SELECT target_id FROM edges WHERE target_type = 'Finding'
                     )"
                )?;
                let ids: Vec<i64> = stmt.query_map(params![project_id], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                return Ok(ids);
            }
            "decision" => ("decisions", "id", "project_id"),
            "hypothesis" => {
                // Hypotheses are scoped through phases
                let mut stmt = self.conn.prepare(
                    "SELECT h.id FROM hypotheses h
                     LEFT JOIN phases p ON h.phase_id = p.id
                     WHERE (p.project_id = ?1 OR h.phase_id IS NULL)
                     AND h.id NOT IN (
                         SELECT source_id FROM edges WHERE source_type = 'Hypothesis'
                         UNION
                         SELECT target_id FROM edges WHERE target_type = 'Hypothesis'
                     )"
                )?;
                let ids: Vec<i64> = stmt.query_map(params![project_id], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                return Ok(ids);
            }
            "literature" => ("literature", "id", "project_id"),
            "principle" => ("principles", "id", "project_id"),
            "constraint" => ("constraints_tbl", "id", "project_id"),
            _ => return Ok(vec![]),
        };
        let node_type_cap = match node_type {
            "finding" => "Finding",
            "decision" => "Decision",
            "literature" => "Literature",
            "principle" => "Principle",
            "constraint" => "Constraint",
            _ => return Ok(vec![]),
        };
        let sql = format!(
            "SELECT {id_col} FROM {table} WHERE {project_filter} = ?1 AND {id_col} NOT IN (
                SELECT source_id FROM edges WHERE source_type = ?2
                UNION
                SELECT target_id FROM edges WHERE target_type = ?2
            )"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let ids: Vec<i64> = stmt.query_map(params![project_id, node_type_cap], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    // --- Temporal Awareness (Feature 5) ---

    fn create_session(&self, project_id: Option<i64>) -> Result<Session> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO sessions (project_id, started_at, created_at) VALUES (?1, ?2, ?2)",
            params![project_id, now],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Session {
            id,
            project_id,
            started_at: Self::parse_dt(&now),
            ended_at: None,
            summary: None,
            active_experiment_id: None,
        })
    }

    fn end_session(&self, id: i64, summary: Option<&str>) -> Result<()> {
        let now = Self::now();
        let rows = self.conn.execute(
            "UPDATE sessions SET ended_at = ?1, summary = ?2 WHERE id = ?3",
            params![now, summary, id],
        )?;
        if rows == 0 {
            return Err(StoreError::NotFound { entity: "session".into(), id });
        }
        Ok(())
    }

    fn list_sessions(&self, project_id: Option<i64>) -> Result<Vec<Session>> {
        let sql = if project_id.is_some() {
            "SELECT id, project_id, started_at, ended_at, summary, active_experiment_id FROM sessions WHERE project_id = ?1 ORDER BY id"
        } else {
            "SELECT id, project_id, started_at, ended_at, summary, active_experiment_id FROM sessions ORDER BY id"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(pid) = project_id {
            stmt.query_map(params![pid], |row| {
                let ended_at: Option<String> = row.get(3)?;
                Ok(Session {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    started_at: SqliteStore::parse_dt(&row.get::<_, String>(2)?),
                    ended_at: ended_at.map(|s| SqliteStore::parse_dt(&s)),
                    summary: row.get(4)?,
                    active_experiment_id: row.get(5).ok().unwrap_or(None),
                })
            })?.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)?
        } else {
            stmt.query_map([], |row| {
                let ended_at: Option<String> = row.get(3)?;
                Ok(Session {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    started_at: SqliteStore::parse_dt(&row.get::<_, String>(2)?),
                    ended_at: ended_at.map(|s| SqliteStore::parse_dt(&s)),
                    summary: row.get(4)?,
                    active_experiment_id: row.get(5).ok().unwrap_or(None),
                })
            })?.collect::<std::result::Result<Vec<_>, _>>().map_err(StoreError::Db)?
        };
        Ok(rows)
    }

    fn get_current_session(&self) -> Result<Option<Session>> {
        let result = self.conn.query_row(
            "SELECT id, project_id, started_at, ended_at, summary, active_experiment_id FROM sessions WHERE ended_at IS NULL ORDER BY id DESC LIMIT 1",
            [],
            |row| {
                let ended_at: Option<String> = row.get(3)?;
                Ok(Session {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    started_at: SqliteStore::parse_dt(&row.get::<_, String>(2)?),
                    ended_at: ended_at.map(|s| SqliteStore::parse_dt(&s)),
                    summary: row.get(4)?,
                    active_experiment_id: row.get(5).ok().unwrap_or(None),
                })
            },
        );
        match result {
            Ok(session) => Ok(Some(session)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StoreError::Db(e)),
        }
    }

    fn nodes_since(&self, timestamp: &str) -> Result<TemporalDelta> {
        let mut delta = TemporalDelta {
            since: timestamp.to_string(),
            phases: vec![], experiments: vec![], findings: vec![],
            decisions: vec![], hypotheses: vec![], research: vec![],
            literature: vec![], principles: vec![], constraints: vec![],
            feedback: vec![],
        };

        // Phases created or modified since timestamp
        {
            let mut stmt = self.conn.prepare("SELECT id FROM phases WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.phases.push(self.get_phase(id)?); }
        }

        // Experiments
        {
            let mut stmt = self.conn.prepare("SELECT id FROM experiments WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.experiments.push(self.get_experiment(id)?); }
        }

        // Findings
        {
            let mut stmt = self.conn.prepare("SELECT id FROM findings WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.findings.push(self.get_finding(id)?); }
        }

        // Decisions
        {
            let mut stmt = self.conn.prepare("SELECT id FROM decisions WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.decisions.push(self.get_decision(id)?); }
        }

        // Hypotheses
        {
            let mut stmt = self.conn.prepare("SELECT id FROM hypotheses WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.hypotheses.push(self.get_hypothesis(id)?); }
        }

        // Research
        {
            let mut stmt = self.conn.prepare("SELECT id FROM research WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.research.push(self.get_research(id)?); }
        }

        // Literature
        {
            let mut stmt = self.conn.prepare("SELECT id FROM literature WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.literature.push(self.get_literature(id)?); }
        }

        // Principles
        {
            let mut stmt = self.conn.prepare("SELECT id FROM principles WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.principles.push(self.get_principle(id)?); }
        }

        // Constraints
        {
            let mut stmt = self.conn.prepare("SELECT id FROM constraints_tbl WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.constraints.push(self.get_constraint(id)?); }
        }

        // Feedback
        {
            let mut stmt = self.conn.prepare("SELECT id FROM feedback WHERE created_at > ?1 OR modified_at > ?1 ORDER BY id")?;
            let ids: Vec<i64> = stmt.query_map(params![timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids { delta.feedback.push(self.get_feedback_entry(id)?); }
        }

        Ok(delta)
    }

    fn staleness_report(&self, project_id: i64) -> Result<StalenessReport> {
        let now = chrono::Local::now().naive_local();
        let mut report = StalenessReport {
            stale_hypotheses: vec![],
            stale_experiments: vec![],
            unconnected_findings: vec![],
        };

        // Stale hypotheses: proposed > 7 days without testing
        if let Ok(phases) = self.list_phases(project_id) {
            for phase in &phases {
                if let Ok(hyps) = self.list_hypotheses(Some(phase.id)) {
                    for h in hyps {
                        if h.status == HypothesisStatus::Proposed {
                            let days = now.signed_duration_since(h.created_at).num_days();
                            if days >= 7 {
                                report.stale_hypotheses.push((h, days));
                            }
                        }
                    }
                }
            }
        }

        // Stale experiments: pending > 14 days
        if let Ok(phases) = self.list_phases(project_id) {
            for phase in &phases {
                if let Ok(exps) = self.list_experiments(Some(phase.id)) {
                    for e in exps {
                        if e.status == ExperimentStatus::Pending {
                            let days = now.signed_duration_since(e.created_at).num_days();
                            if days >= 14 {
                                report.stale_experiments.push((e, days));
                            }
                        }
                    }
                }
            }
        }

        // Unconnected findings: > 30 days old with no edges
        if let Ok(orphan_ids) = self.get_orphaned_nodes("finding", project_id) {
            for fid in orphan_ids {
                if let Ok(f) = self.get_finding(fid) {
                    let days = now.signed_duration_since(f.created_at).num_days();
                    if days >= 30 {
                        report.unconnected_findings.push(f);
                    }
                }
            }
        }

        Ok(report)
    }

    fn get_velocity(&self, project_id: i64) -> Result<VelocityMetrics> {
        let mut metrics = VelocityMetrics {
            findings_per_session: vec![],
            experiments_per_week: vec![],
            hypothesis_lifecycle_days: vec![],
        };

        // Findings per session: count findings created during each session window
        let sessions = self.list_sessions(Some(project_id))?;
        for session in &sessions {
            let started = session.started_at.format("%Y-%m-%d %H:%M:%S").to_string();
            let ended = session.ended_at.map(|e| e.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "9999-12-31 23:59:59".to_string());

            let count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM findings WHERE created_at >= ?1 AND created_at <= ?2",
                params![started, ended],
                |row| row.get(0),
            )?;
            metrics.findings_per_session.push((session.id, count as usize));
        }

        // Experiments per week: group by ISO week
        {
            let mut stmt = self.conn.prepare(
                "SELECT strftime('%Y-W%W', e.created_at) as week, e.status FROM experiments e
                 JOIN phases p ON e.phase_id = p.id
                 WHERE p.project_id = ?1 AND e.status IN ('pass', 'fail')
                 ORDER BY week"
            )?;
            let mut week_map: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();
            let rows = stmt.query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (week, status) = row?;
                let entry = week_map.entry(week).or_insert((0, 0));
                match status.as_str() {
                    "pass" => entry.0 += 1,
                    "fail" => entry.1 += 1,
                    _ => {}
                }
            }
            for (week, (pass, fail)) in week_map {
                metrics.experiments_per_week.push((week, pass, fail));
            }
        }

        // Hypothesis lifecycle: time from proposed to confirmed/refuted
        if let Ok(phases) = self.list_phases(project_id) {
            for phase in &phases {
                if let Ok(hyps) = self.list_hypotheses(Some(phase.id)) {
                    for h in hyps {
                        if h.status == HypothesisStatus::Confirmed || h.status == HypothesisStatus::Refuted {
                            // Get modified_at as resolution timestamp
                            if let Ok(Some(modified_str)) = self.get_modified_at("hypotheses", h.id) {
                                let modified = Self::parse_dt(&modified_str);
                                let days = modified.signed_duration_since(h.created_at).num_days() as f64;
                                metrics.hypothesis_lifecycle_days.push((h.id, days));
                            }
                        }
                    }
                }
            }
        }

        Ok(metrics)
    }



    fn text_search(&self, query: &str) -> Result<Vec<SearchResult>> {
        // Match any individual word for broader recall
        let query_words: Vec<&str> = query.split_whitespace().filter(|w| w.len() >= 2).collect();
        let pattern = if query_words.len() > 1 {
            // Use first significant word for SQL filtering (broad recall)
            format!("%{}%", query_words[0])
        } else {
            format!("%{}%", query)
        };
        let mut results: Vec<SearchResult> = Vec::new();

        // findings.text
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, text, modified_at, confidence, belief_status FROM findings WHERE text LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<String>>(5)?))
            })?;
            for r in rows {
                let (id, seq, text, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "finding".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // decisions.what, decisions.why
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, what, why, modified_at, confidence, belief_status FROM decisions WHERE what LIKE ?1 OR why LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<f64>>(5)?, row.get::<_, Option<String>>(6)?))
            })?;
            for r in rows {
                let (id, seq, what, _why, modified_at, confidence, belief_status) = r?;
                let excerpt: String = what.chars().take(150).collect();
                results.push(SearchResult { node_type: "decision".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // hypotheses.text, hypotheses.prediction
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, text, prediction, modified_at, confidence, belief_status FROM hypotheses WHERE text LIKE ?1 OR prediction LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<f64>>(5)?, row.get::<_, Option<String>>(6)?))
            })?;
            for r in rows {
                let (id, seq, text, _pred, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "hypothesis".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // literature.title, literature.key_findings
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, title, key_findings, modified_at FROM literature WHERE title LIKE ?1 OR key_findings LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?))
            })?;
            for r in rows {
                let (id, seq, title, _kf, modified_at) = r?;
                let excerpt: String = title.chars().take(150).collect();
                results.push(SearchResult { node_type: "literature".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // phases.name, phases.description
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, name, description, modified_at FROM phases WHERE name LIKE ?1 OR description LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?))
            })?;
            for r in rows {
                let (id, seq, name, _desc, modified_at) = r?;
                let excerpt: String = name.chars().take(150).collect();
                results.push(SearchResult { node_type: "phase".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // research.name, research.report
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, name, report, modified_at FROM research WHERE name LIKE ?1 OR report LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?))
            })?;
            for r in rows {
                let (id, seq, name, _report, modified_at) = r?;
                let excerpt: String = name.chars().take(150).collect();
                results.push(SearchResult { node_type: "research".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // experiments.name
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, name, modified_at FROM experiments WHERE name LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?))
            })?;
            for r in rows {
                let (id, seq, name, modified_at) = r?;
                let excerpt: String = name.chars().take(150).collect();
                results.push(SearchResult { node_type: "experiment".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // principles.text
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, text, modified_at, confidence, belief_status FROM principles WHERE text LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<String>>(5)?))
            })?;
            for r in rows {
                let (id, seq, text, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "principle".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // constraints_tbl.text
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, text, modified_at, confidence, belief_status FROM constraints_tbl WHERE text LIKE ?1 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<String>>(5)?))
            })?;
            for r in rows {
                let (id, seq, text, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "constraint".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // Truncate to 50 total results
        results.truncate(50);
        Ok(results)
    }

    fn text_search_in_project(&self, query: &str, project_id: Option<i64>) -> Result<Vec<SearchResult>> {
        // Project-scoped text search. When project_id is None, behaves identically
        // to text_search (cross-project). When Some(pid), each per-table query
        // adds a project_id filter (direct column or via phase/experiment join).
        let pid = match project_id {
            None => return self.text_search(query),
            Some(p) => p,
        };
        let query_words: Vec<&str> = query.split_whitespace().filter(|w| w.len() >= 2).collect();
        let pattern = if query_words.len() > 1 {
            format!("%{}%", query_words[0])
        } else {
            format!("%{}%", query)
        };
        let mut results: Vec<SearchResult> = Vec::new();

        // findings.text — project via experiment.phase_id -> phase.project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT f.id, f.project_seq, f.text, f.modified_at, f.confidence, f.belief_status                  FROM findings f                  LEFT JOIN experiments e ON f.experiment_id = e.id                  LEFT JOIN phases ph ON e.phase_id = ph.id                  WHERE f.text LIKE ?1 AND ph.project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<String>>(5)?))
            })?;
            for r in rows {
                let (id, seq, text, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "finding".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // decisions — has direct project_id (added in v2/v13)
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, what, why, modified_at, confidence, belief_status                  FROM decisions WHERE (what LIKE ?1 OR why LIKE ?1) AND project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<f64>>(5)?, row.get::<_, Option<String>>(6)?))
            })?;
            for r in rows {
                let (id, seq, what, _why, modified_at, confidence, belief_status) = r?;
                let excerpt: String = what.chars().take(150).collect();
                results.push(SearchResult { node_type: "decision".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // hypotheses — via phase.project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT h.id, h.project_seq, h.text, h.prediction, h.modified_at, h.confidence, h.belief_status                  FROM hypotheses h                  LEFT JOIN phases ph ON h.phase_id = ph.id                  WHERE (h.text LIKE ?1 OR h.prediction LIKE ?1) AND ph.project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<f64>>(5)?, row.get::<_, Option<String>>(6)?))
            })?;
            for r in rows {
                let (id, seq, text, _pred, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "hypothesis".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // literature — direct project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, title, key_findings, modified_at FROM literature                  WHERE (title LIKE ?1 OR key_findings LIKE ?1) AND project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?))
            })?;
            for r in rows {
                let (id, seq, title, _kf, modified_at) = r?;
                let excerpt: String = title.chars().take(150).collect();
                results.push(SearchResult { node_type: "literature".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // phases — direct project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, name, description, modified_at FROM phases                  WHERE (name LIKE ?1 OR description LIKE ?1) AND project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?))
            })?;
            for r in rows {
                let (id, seq, name, _desc, modified_at) = r?;
                let excerpt: String = name.chars().take(150).collect();
                results.push(SearchResult { node_type: "phase".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // research — via phase.project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT r.id, r.project_seq, r.name, r.report, r.modified_at FROM research r                  LEFT JOIN phases ph ON r.phase_id = ph.id                  WHERE (r.name LIKE ?1 OR r.report LIKE ?1) AND ph.project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?))
            })?;
            for r in rows {
                let (id, seq, name, _report, modified_at) = r?;
                let excerpt: String = name.chars().take(150).collect();
                results.push(SearchResult { node_type: "research".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // experiments — via phase.project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT e.id, e.project_seq, e.name, e.modified_at FROM experiments e                  LEFT JOIN phases ph ON e.phase_id = ph.id                  WHERE e.name LIKE ?1 AND ph.project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?))
            })?;
            for r in rows {
                let (id, seq, name, modified_at) = r?;
                let excerpt: String = name.chars().take(150).collect();
                results.push(SearchResult { node_type: "experiment".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence: None, belief_status: None });
            }
        }

        // principles — direct project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, text, modified_at, confidence, belief_status FROM principles                  WHERE text LIKE ?1 AND project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<String>>(5)?))
            })?;
            for r in rows {
                let (id, seq, text, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "principle".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        // constraints_tbl — direct project_id
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_seq, text, modified_at, confidence, belief_status FROM constraints_tbl                  WHERE text LIKE ?1 AND project_id = ?2 LIMIT 50"
            )?;
            let rows = stmt.query_map(params![pattern, pid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<String>>(5)?))
            })?;
            for r in rows {
                let (id, seq, text, modified_at, confidence, belief_status) = r?;
                let excerpt: String = text.chars().take(150).collect();
                results.push(SearchResult { node_type: "constraint".into(), node_id: id, project_seq: seq, text_excerpt: excerpt, modified_at, score: None, confidence, belief_status });
            }
        }

        results.truncate(50);
        Ok(results)
    }

    fn set_session_experiment(&self, experiment_id: i64) -> Result<()> {
        let rows = self.conn.execute(
            "UPDATE sessions SET active_experiment_id = ?1 WHERE ended_at IS NULL",
            params![experiment_id],
        )?;
        if rows == 0 {
            return Err(StoreError::NotFound { entity: "active session".into(), id: 0 });
        }
        Ok(())
    }

}

