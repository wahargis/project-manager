use pm::cli::*;
use pm::store::sqlite::SqliteStore;
use pm::store::{Store, PhaseStatus, ExperimentStatus, NodeType, EdgeType};
use pm::dag::DagEngine;
use pm::kg::KgEngine;

fn default_db_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{}/.local/share/pm", home);
    std::fs::create_dir_all(&dir).ok();
    format!("{}/pm.db", dir)
}

pub fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = cli.db.unwrap_or_else(default_db_path);
    let store = SqliteStore::new(&db_path)?;

    match cli.command {
        Commands::Dashboard => {
            let projects = store.list_projects()?;
            println!("=== Cross-Project Dashboard ===\n");
            for proj in &projects {
                if proj.status != pm::store::ProjectStatus::Active { continue; }
                let dag = DagEngine::new(&store, proj.id);
                let next = dag.next_phases()?;
                if let Some(top) = next.first() {
                    let status_str = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                    println!("  [{}] {} #{} [impact:{}] {}", proj.name, status_str, top.id, top.impact, top.name);
                }
            }
            println!("\n## ACTION: Execute the highest-impact item above.");
        }
        Commands::Project { action } => match action {
            ProjectAction::List => {
                for p in store.list_projects()? {
                    println!("  {} ({:?}) [alias: {}]", p.name, p.status, p.alias.as_deref().unwrap_or("-"));
                }
            }
            ProjectAction::Activate { name, alias } => {
                let p = store.create_project(&name, alias.as_deref())?;
                println!("Activated: {} (id: {})", p.name, p.id);
            }
            ProjectAction::Pause { name } => {
                let projects = store.list_projects()?;
                if let Some(p) = projects.iter().find(|p| p.name == name || p.alias.as_deref() == Some(&name)) {
                    store.update_project_status(p.id, pm::store::ProjectStatus::Paused)?;
                    println!("Paused: {}", name);
                }
            }
            ProjectAction::Archive { name } => {
                let projects = store.list_projects()?;
                if let Some(p) = projects.iter().find(|p| p.name == name || p.alias.as_deref() == Some(&name)) {
                    store.update_project_status(p.id, pm::store::ProjectStatus::Archived)?;
                    println!("Archived: {}", name);
                }
            }
        },
        Commands::Next { project } => {
            let projects = store.list_projects()?;
            if let Some(proj) = projects.iter().find(|p| p.name == project || p.alias.as_deref() == Some(&project)) {
                let dag = DagEngine::new(&store, proj.id);
                let next = dag.next_phases()?;
                println!("=== Next Phases (by impact) ===\n");
                for (i, phase) in next.iter().take(3).enumerate() {
                    let status = if phase.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                    println!("  {} #{} [impact:{}] {}", status, phase.id, phase.impact, phase.name);
                }
                if let Some(stag) = dag.stagnation_check(3)? {
                    println!("\n  WARNING: STAGNATION — {} consecutive failed experiments", stag);
                }
                println!("\n## ACTION: Execute the top phase.");
            } else {
                eprintln!("Project not found: {}", project);
            }
        }
        _ => {
            println!("Command not yet implemented");
        }
    }
    Ok(())
}
