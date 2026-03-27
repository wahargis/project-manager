//! Versioned schema migration system for project-manager.
//!
//! Each migration is a numbered function that applies DDL changes.
//! Migrations are idempotent — they handle pre-existing columns gracefully.
//! The schema_version table tracks which migrations have been applied.

use rusqlite::Connection;

/// Current highest migration version.
const LATEST_VERSION: i64 = 8;

/// Run all pending migrations on the database connection.
/// Creates the schema_version table if it doesn't exist, checks the current
/// version, and applies each migration sequentially in a transaction.
pub fn migrate(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    // Create schema_version table if it doesn't exist
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL,
            applied_at TEXT NOT NULL
        );"
    )?;

    let current_version = get_current_version(conn)?;

    if current_version >= LATEST_VERSION {
        return Ok(());
    }

    // Apply each migration sequentially
    for v in (current_version + 1)..=LATEST_VERSION {
        apply_migration(conn, v)?;
    }

    Ok(())
}

/// Get the current schema version (0 if no migrations have been applied).
fn get_current_version(conn: &Connection) -> Result<i64, Box<dyn std::error::Error>> {
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    Ok(version)
}

/// Apply a single migration inside a transaction and record the version.
fn apply_migration(conn: &Connection, version: i64) -> Result<(), Box<dyn std::error::Error>> {
    let tx = conn.unchecked_transaction()?;

    match version {
        1 => migrate_v1_phases(&tx)?,
        2 => migrate_v2_decisions(&tx)?,
        3 => migrate_v3_literature(&tx)?,
        4 => migrate_v4_hypotheses(&tx)?,
        5 => migrate_v5_constraints(&tx)?,
        6 => migrate_v6_principles(&tx)?,
        7 => migrate_v7_edges_uniqueness(&tx)?,
        8 => migrate_v8_subprojects(&tx)?,
        _ => return Err(format!("Unknown migration version: {}", version).into()),
    }

    tx.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        rusqlite::params![version],
    )?;

    tx.commit()?;
    Ok(())
}

/// Helper: add a column if it doesn't already exist.
/// Ignores "duplicate column name" errors from SQLite.
fn add_column_if_not_exists(
    conn: &Connection,
    table: &str,
    column: &str,
    col_type: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let sql = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, col_type);
    match conn.execute_batch(&sql) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("duplicate column") => Ok(()),
        Err(e) => Err(e.into()),
    }
}

// --- Migration v1: Phases ---
// Add description, goals, success_criteria, started_at, completed_at to phases
fn migrate_v1_phases(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    add_column_if_not_exists(conn, "phases", "description", "TEXT")?;
    add_column_if_not_exists(conn, "phases", "goals", "TEXT")?;
    add_column_if_not_exists(conn, "phases", "success_criteria", "TEXT")?;
    add_column_if_not_exists(conn, "phases", "started_at", "TEXT")?;
    add_column_if_not_exists(conn, "phases", "completed_at", "TEXT")?;
    Ok(())
}

// --- Migration v2: Decisions ---
// Add project_id to decisions, backfill from experiment→phase→project chain
fn migrate_v2_decisions(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    add_column_if_not_exists(conn, "decisions", "project_id", "INTEGER REFERENCES projects(id)")?;

    // Backfill project_id from experiment→phase→project chain
    conn.execute_batch(
        "UPDATE decisions SET project_id = (
            SELECT ph.project_id FROM phases ph
            JOIN experiments e ON e.phase_id = ph.id
            WHERE e.id = decisions.experiment_id
        ) WHERE decisions.experiment_id IS NOT NULL AND decisions.project_id IS NULL;"
    )?;
    Ok(())
}

// --- Migration v3: Literature ---
// Add venue, year, code_url, file_path, status, summary to literature
fn migrate_v3_literature(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    add_column_if_not_exists(conn, "literature", "venue", "TEXT")?;
    add_column_if_not_exists(conn, "literature", "year", "INTEGER")?;
    add_column_if_not_exists(conn, "literature", "code_url", "TEXT")?;
    add_column_if_not_exists(conn, "literature", "file_path", "TEXT")?;
    add_column_if_not_exists(conn, "literature", "status", "TEXT DEFAULT 'unread'")?;
    add_column_if_not_exists(conn, "literature", "summary", "TEXT")?;
    Ok(())
}

// --- Migration v4: Hypotheses ---
// Add prediction, criteria, confidence to hypotheses
fn migrate_v4_hypotheses(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    add_column_if_not_exists(conn, "hypotheses", "prediction", "TEXT")?;
    add_column_if_not_exists(conn, "hypotheses", "criteria", "TEXT")?;
    add_column_if_not_exists(conn, "hypotheses", "confidence", "REAL")?;
    Ok(())
}

// --- Migration v5: Constraints ---
// Add severity, resource, measured_value, expires_at to constraints_tbl
fn migrate_v5_constraints(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    add_column_if_not_exists(conn, "constraints_tbl", "severity", "TEXT DEFAULT 'hard'")?;
    add_column_if_not_exists(conn, "constraints_tbl", "resource", "TEXT")?;
    add_column_if_not_exists(conn, "constraints_tbl", "measured_value", "TEXT")?;
    add_column_if_not_exists(conn, "constraints_tbl", "expires_at", "TEXT")?;
    Ok(())
}

// --- Migration v6: Principles ---
// Add rationale, enforcement_level to principles
fn migrate_v6_principles(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    add_column_if_not_exists(conn, "principles", "rationale", "TEXT")?;
    add_column_if_not_exists(conn, "principles", "enforcement_level", "TEXT DEFAULT 'advisory'")?;
    Ok(())
}

// --- Migration v7: Edges uniqueness ---
// Create unique index on edges to prevent duplicate relationships
fn migrate_v7_edges_uniqueness(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    // Deduplicate existing edges before creating unique index
    conn.execute_batch(
        "DELETE FROM edges WHERE id NOT IN (
            SELECT MIN(id) FROM edges 
            GROUP BY source_type, source_id, target_type, target_id, relation
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_edges_unique
         ON edges (source_type, source_id, target_type, target_id, relation);"
    )?;
    Ok(())
}

// --- Migration v8: Subprojects ---
// Add parent_id to projects for hierarchical project structure
fn migrate_v8_subprojects(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    add_column_if_not_exists(conn, "projects", "parent_id", "INTEGER REFERENCES projects(id)")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Create a minimal in-memory DB with the base schema (no migrations).
    fn setup_base_schema() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(
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
        ).unwrap();
        conn
    }

    #[test]
    fn test_migration_from_v0_to_latest() {
        let conn = setup_base_schema();
        migrate(&conn).unwrap();

        // Verify v1: phases has new columns
        conn.execute(
            "INSERT INTO projects (name, status, created_at) VALUES ('test', 'active', '2025-01-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO phases (project_id, name, status, impact, created_at, description, goals, success_criteria, started_at, completed_at)
             VALUES (1, 'P1', 'pending', 10, '2025-01-01 00:00:00', 'A description', 'Some goals', 'Criteria here', '2025-01-01 00:00:00', NULL)",
            [],
        ).unwrap();

        // Verify v2: decisions has project_id
        conn.execute(
            "INSERT INTO decisions (what, why, created_at, project_id) VALUES ('Use Rust', 'Type safety', '2025-01-01 00:00:00', 1)",
            [],
        ).unwrap();

        // Verify v3: literature has new columns
        conn.execute(
            "INSERT INTO literature (project_id, title, created_at, venue, year, code_url, file_path, status, summary)
             VALUES (1, 'A Paper', '2025-01-01 00:00:00', 'NeurIPS', 2024, 'https://github.com/foo', '/papers/a.pdf', 'read', 'A summary')",
            [],
        ).unwrap();

        // Verify v4: hypotheses has new columns
        conn.execute(
            "INSERT INTO hypotheses (text, status, created_at, prediction, criteria, confidence)
             VALUES ('H1', 'proposed', '2025-01-01 00:00:00', 'It will work', 'p < 0.05', 0.8)",
            [],
        ).unwrap();

        // Verify v5: constraints_tbl has new columns
        conn.execute(
            "INSERT INTO constraints_tbl (project_id, scope, text, created_at, severity, resource, measured_value, expires_at)
             VALUES (1, 'hardware', 'Max 32GB VRAM', '2025-01-01 00:00:00', 'hard', 'GPU VRAM', '32768', NULL)",
            [],
        ).unwrap();

        // Verify v6: principles has new columns
        conn.execute(
            "INSERT INTO principles (project_id, scope, text, status, created_at, rationale, enforcement_level)
             VALUES (1, 'project', 'No force kills', 'active', '2025-01-01 00:00:00', 'Prevents driver wedge', 'mandatory')",
            [],
        ).unwrap();

        // Verify v7: edges unique index exists (inserting duplicate should fail)
        conn.execute(
            "INSERT INTO edges (source_type, source_id, target_type, target_id, relation) VALUES ('Finding', 1, 'Finding', 2, 'Supports')",
            [],
        ).unwrap();
        let dup_result = conn.execute(
            "INSERT INTO edges (source_type, source_id, target_type, target_id, relation) VALUES ('Finding', 1, 'Finding', 2, 'Supports')",
            [],
        );
        assert!(dup_result.is_err(), "Duplicate edge should fail with unique index");

        // Verify v8: projects has parent_id
        conn.execute(
            "INSERT INTO projects (name, status, created_at, parent_id) VALUES ('sub', 'active', '2025-01-01 00:00:00', 1)",
            [],
        ).unwrap();

        // Verify schema_version recorded all 8 migrations
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 8);

        let max_version: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(max_version, 8);
    }

    #[test]
    fn test_migration_idempotent() {
        let conn = setup_base_schema();

        // Run migrate twice — should succeed both times
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 8, "Should still have exactly 8 version records after idempotent run");
    }

    #[test]
    fn test_migration_preserves_data() {
        let conn = setup_base_schema();

        // Insert data before migration
        conn.execute("INSERT INTO projects (name, status, created_at) VALUES ('existing', 'active', '2025-01-01 00:00:00')", []).unwrap();
        conn.execute("INSERT INTO phases (project_id, name, status, impact, created_at) VALUES (1, 'Phase1', 'in_progress', 40, '2025-01-01 00:00:00')", []).unwrap();
        conn.execute("INSERT INTO experiments (phase_id, name, status, created_at) VALUES (1, 'Exp1', 'pass', '2025-01-01 00:00:00')", []).unwrap();
        conn.execute("INSERT INTO decisions (experiment_id, what, why, created_at) VALUES (1, 'Use Q4_K', 'Best quality/speed', '2025-01-01 00:00:00')", []).unwrap();
        conn.execute("INSERT INTO literature (project_id, title, created_at) VALUES (1, 'Attention Is All You Need', '2025-01-01 00:00:00')", []).unwrap();
        conn.execute("INSERT INTO hypotheses (text, status, created_at) VALUES ('H1', 'proposed', '2025-01-01 00:00:00')", []).unwrap();
        conn.execute("INSERT INTO constraints_tbl (project_id, scope, text, created_at) VALUES (1, 'hardware', 'Max 32GB', '2025-01-01 00:00:00')", []).unwrap();
        conn.execute("INSERT INTO principles (project_id, scope, text, status, created_at) VALUES (1, 'project', 'No force kills', 'active', '2025-01-01 00:00:00')", []).unwrap();

        // Run migration
        migrate(&conn).unwrap();

        // Verify existing data is preserved
        let proj_name: String = conn.query_row("SELECT name FROM projects WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(proj_name, "existing");

        let phase_name: String = conn.query_row("SELECT name FROM phases WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(phase_name, "Phase1");

        // New columns should be NULL for pre-existing data
        let desc: Option<String> = conn.query_row("SELECT description FROM phases WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert!(desc.is_none(), "description should be NULL for pre-existing phase");

        // v2 backfill: decision should have project_id from experiment→phase→project
        let dec_project_id: Option<i64> = conn.query_row("SELECT project_id FROM decisions WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(dec_project_id, Some(1), "decision project_id should be backfilled from experiment chain");

        // Literature new columns should be NULL/default
        let lit_status: Option<String> = conn.query_row("SELECT status FROM literature WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(lit_status, Some("unread".to_string()), "literature status should default to 'unread'");

        let lit_venue: Option<String> = conn.query_row("SELECT venue FROM literature WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert!(lit_venue.is_none(), "venue should be NULL for pre-existing literature");

        // Hypothesis new columns should be NULL
        let confidence: Option<f64> = conn.query_row("SELECT confidence FROM hypotheses WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert!(confidence.is_none(), "confidence should be NULL for pre-existing hypothesis");

        // Constraints new columns should have defaults
        let severity: Option<String> = conn.query_row("SELECT severity FROM constraints_tbl WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(severity, Some("hard".to_string()), "severity should default to 'hard'");

        // Principles new columns
        let enforcement: Option<String> = conn.query_row("SELECT enforcement_level FROM principles WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(enforcement, Some("advisory".to_string()), "enforcement_level should default to 'advisory'");
    }

    #[test]
    fn test_migration_partial_application() {
        let conn = setup_base_schema();

        // Simulate: v1-v3 already applied
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL,
                applied_at TEXT NOT NULL
            );
            INSERT INTO schema_version (version, applied_at) VALUES (1, '2025-01-01 00:00:00');
            INSERT INTO schema_version (version, applied_at) VALUES (2, '2025-01-01 00:00:00');
            INSERT INTO schema_version (version, applied_at) VALUES (3, '2025-01-01 00:00:00');"
        ).unwrap();

        // Manually add the columns that v1-v3 would have added
        conn.execute_batch(
            "ALTER TABLE phases ADD COLUMN description TEXT;
             ALTER TABLE phases ADD COLUMN goals TEXT;
             ALTER TABLE phases ADD COLUMN success_criteria TEXT;
             ALTER TABLE phases ADD COLUMN started_at TEXT;
             ALTER TABLE phases ADD COLUMN completed_at TEXT;
             ALTER TABLE decisions ADD COLUMN project_id INTEGER REFERENCES projects(id);
             ALTER TABLE literature ADD COLUMN venue TEXT;
             ALTER TABLE literature ADD COLUMN year INTEGER;
             ALTER TABLE literature ADD COLUMN code_url TEXT;
             ALTER TABLE literature ADD COLUMN file_path TEXT;
             ALTER TABLE literature ADD COLUMN status TEXT DEFAULT 'unread';
             ALTER TABLE literature ADD COLUMN summary TEXT;"
        ).unwrap();

        // Now migrate — should only apply v4-v8
        migrate(&conn).unwrap();

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 8, "Should have 8 total version records (3 pre-existing + 5 new)");

        // Verify v4 columns exist
        conn.execute(
            "INSERT INTO projects (name, status, created_at) VALUES ('test', 'active', '2025-01-01 00:00:00')", [],
        ).unwrap();
        conn.execute(
            "INSERT INTO hypotheses (text, status, created_at, prediction, criteria, confidence) VALUES ('H', 'proposed', '2025-01-01 00:00:00', 'P', 'C', 0.5)",
            [],
        ).unwrap();
    }
}
