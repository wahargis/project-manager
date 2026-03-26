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
        Commands::Import { path, name } => {
            import_v2(&store, &path, &name)?;
        }
        _ => {
            println!("Command not yet implemented");
        }
    }
    Ok(())
}

fn import_v2(store: &SqliteStore, path: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let data: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    
    // Create project
    let proj = store.create_project(name, None)?;
    println!("Created project: {} (id: {})", name, proj.id);
    
    // Import phases
    let mut phase_id_map = std::collections::HashMap::new();
    if let Some(phases) = data["phases"].as_array() {
        for p in phases {
            let old_id = p["id"].as_i64().unwrap_or(0);
            let pname = p["name"].as_str().unwrap_or("unnamed");
            let impact = p["impact"].as_i64().unwrap_or(0) as i32;
            let deps: Vec<i64> = p["depends_on"].as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default();
            // Map old dep IDs to new IDs
            let mapped_deps: Vec<i64> = deps.iter()
                .filter_map(|old| phase_id_map.get(old).copied())
                .collect();
            
            let phase = store.create_phase(proj.id, pname, impact, &mapped_deps)?;
            
            // Set status
            let status_str = p["status"].as_str().unwrap_or("pending");
            let status = match status_str {
                "complete" => pm::store::PhaseStatus::Complete,
                "in_progress" => pm::store::PhaseStatus::InProgress,
                "deprioritized" => pm::store::PhaseStatus::Deprioritized,
                "paused" | "paused-at-ceiling" => pm::store::PhaseStatus::Paused,
                _ => pm::store::PhaseStatus::Pending,
            };
            store.update_phase_status(phase.id, status)?;
            phase_id_map.insert(old_id, phase.id);
        }
        println!("  Imported {} phases", phases.len());
    }
    
    // Import experiments
    let mut exp_id_map = std::collections::HashMap::new();
    if let Some(exps) = data["experiments"].as_array() {
        for e in exps {
            let old_id = e["id"].as_i64().unwrap_or(0);
            let ename = e["name"].as_str().unwrap_or("unnamed");
            let old_phase = e["phase"].as_i64();
            let new_phase = old_phase.and_then(|op| phase_id_map.get(&op).copied());
            
            let exp = store.create_experiment(new_phase, ename)?;
            
            let status_str = e["status"].as_str().unwrap_or(e["result"].as_str().unwrap_or("pending"));
            let status = match status_str {
                "pass" => pm::store::ExperimentStatus::Pass,
                "fail" => pm::store::ExperimentStatus::Fail,
                "pending" => pm::store::ExperimentStatus::Pending,
                _ => pm::store::ExperimentStatus::Inconclusive,
            };
            let result = e["result"].as_str();
            store.update_experiment_status(exp.id, status, result)?;
            exp_id_map.insert(old_id, exp.id);
        }
        println!("  Imported {} experiments", exps.len());
    }
    
    // Import findings
    if let Some(findings) = data["findings"].as_array() {
        for f in findings {
            let text = f["text"].as_str().unwrap_or("");
            let old_exp = f["experiment"].as_i64();
            let new_exp = old_exp.and_then(|oe| exp_id_map.get(&oe).copied());
            
            let finding = store.create_finding(new_exp, text)?;
            
            // Create edges for supports/contradicts
            if let Some(sup) = f["supports"].as_i64() {
                // Edge will reference old finding IDs — need mapping
                // For now skip cross-references (would need a second pass)
            }
            if let Some(con) = f["contradicts"].as_i64() {
                // Same — skip for now
            }
        }
        println!("  Imported {} findings", findings.len());
    }
    
    // Import decisions
    if let Some(decs) = data["decisions"].as_array() {
        for d in decs {
            let what = d["what"].as_str().unwrap_or("");
            let why = d["why"].as_str();
            let old_exp = d["experiment"].as_i64();
            let new_exp = old_exp.and_then(|oe| exp_id_map.get(&oe).copied());
            store.create_decision(new_exp, what, why)?;
        }
        println!("  Imported {} decisions", decs.len());
    }
    
    println!("\nImport complete. Run: pm next {}", name);
    Ok(())
}
