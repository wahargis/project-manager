use pm::cli::*;
use pm::store::sqlite::SqliteStore;
use pm::store::{Store, PhaseStatus, ExperimentStatus, ResearchStatus, NodeType, EdgeType, PrincipleScope, PrincipleStatus, HypothesisStatus, ConstraintScope, FeedbackCategory};
use pm::dag::DagEngine;
use pm::kg::KgEngine;

fn default_db_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{}/.local/share/pm", home);
    std::fs::create_dir_all(&dir).ok();
    format!("{}/pm.db", dir)
}

fn resolve_project(store: &SqliteStore, name: &str) -> Option<pm::store::Project> {
    store.list_projects().ok()?.into_iter()
        .find(|p| p.name == name || p.alias.as_deref() == Some(name))
}

pub fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = cli.db.unwrap_or_else(default_db_path);
    let store = SqliteStore::new(&db_path)?;

    match cli.command {
        Commands::Stats { project } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            let phases = store.list_phases(proj.id)?;
            let mut exp_count = 0;
            let mut finding_count = 0;
            for p in &phases {
                exp_count += store.list_experiments(Some(p.id)).map(|e| e.len()).unwrap_or(0);
                for e in store.list_experiments(Some(p.id)).unwrap_or_default() {
                    finding_count += store.list_findings(Some(e.id)).map(|f| f.len()).unwrap_or(0);
                }
            }
            let decisions = store.list_decisions(proj.id).map(|d| d.len()).unwrap_or(0);
            let research = store.list_research(None).map(|r| r.len()).unwrap_or(0);
            let principles = store.list_principles(proj.id).map(|p| p.len()).unwrap_or(0);
            let hypotheses = store.list_hypotheses(None).map(|h| h.len()).unwrap_or(0);
            let constraints = store.list_constraints(proj.id).map(|c| c.len()).unwrap_or(0);
            let literature = store.list_literature(proj.id).map(|l| l.len()).unwrap_or(0);
            let feedback_count = store.list_feedback(proj.id).map(|f| f.len()).unwrap_or(0);
            let edges = store.list_all_edges().map(|e| e.len()).unwrap_or(0);

            println!("=== {} KG Stats ===", proj.name);
            println!("  Phases:      {}", phases.len());
            println!("  Experiments: {}", exp_count);
            println!("  Findings:    {}", finding_count);
            println!("  Decisions:   {}", decisions);
            println!("  Research:    {}", research);
            println!("  Principles:  {}", principles);
            println!("  Hypotheses:  {}", hypotheses);
            println!("  Constraints: {}", constraints);
            println!("  Literature:  {}", literature);
            println!("  Feedback:    {}", feedback_count);
            println!("  Edges:       {}", edges);
            println!("  Total nodes: {}", phases.len() + exp_count + finding_count + decisions + research + principles + hypotheses + constraints + literature + feedback_count);
        }

        Commands::Dashboard => {
            let projects = store.list_projects()?;
            println!("=== Cross-Project Dashboard ===\n");
            for proj in &projects {
                if proj.status != pm::store::ProjectStatus::Active { continue; }
                let dag = DagEngine::new(&store, proj.id);
                if let Ok(next) = dag.next_phases() {
                    // Prefer: InProgress > Pending > Paused
                    let top = next.iter().find(|p| p.status == PhaseStatus::InProgress)
                        .or_else(|| next.iter().find(|p| p.status == PhaseStatus::Pending));
                    if let Some(top) = top {
                        let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                        println!("  [{}] {} #{} [impact:{}] {}", proj.name, s, top.id, top.impact, top.name);
                    }
                    // Show paused as secondary
                    if let Some(p) = next.iter().find(|p| p.status == PhaseStatus::Paused) {
                        println!("  [{}] PAUSED #{} [impact:{}] {}", proj.name, p.id, p.impact, p.name);
                    }
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
                if let Some(p) = resolve_project(&store, &name) {
                    store.update_project_status(p.id, pm::store::ProjectStatus::Paused)?;
                    println!("Paused: {}", name);
                } else { eprintln!("Not found: {}", name); }
            }
            ProjectAction::Archive { name } => {
                if let Some(p) = resolve_project(&store, &name) {
                    store.update_project_status(p.id, pm::store::ProjectStatus::Archived)?;
                    println!("Archived: {}", name);
                } else { eprintln!("Not found: {}", name); }
            }
        },

        Commands::Phase { project, action } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                PhaseAction::Add { name, impact, depends } => {
                    let deps = depends.unwrap_or_default();
                    let phase = store.create_phase(proj.id, &name, impact, &deps)?;
                    println!("Phase #{} added: {} [impact:{}]", phase.id, phase.name, phase.impact);
                }
                PhaseAction::List => {
                    for p in store.list_phases(proj.id)? {
                        let deps = if p.depends_on.is_empty() { String::new() }
                            else { format!(" (depends: {:?})", p.depends_on) };
                        println!("  #{} [{:?}] [impact:{}] {}{}", p.id, p.status, p.impact, p.name, deps);
                    }
                }
                PhaseAction::Update { id, status } => {
                    let s = match status.as_str() {
                        "pending" => PhaseStatus::Pending,
                        "in_progress" => PhaseStatus::InProgress,
                        "complete" => PhaseStatus::Complete,
                        "deprioritized" => PhaseStatus::Deprioritized,
                        "paused" => PhaseStatus::Paused,
                        _ => return Err(format!("Invalid status: {}", status).into()),
                    };
                    store.update_phase_status(id, s)?;
                    println!("Phase #{} updated: status={}", id, status);
                }
                PhaseAction::Get { id } => {
                    let p = store.get_phase(id)?;
                    println!("Phase #{}: {} [{:?}] impact:{} depends:{:?}", p.id, p.name, p.status, p.impact, p.depends_on);
                }
            }
        }

        Commands::Exp { project, action } => {
            let _proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                ExpAction::Add { name, phase, status, result } => {
                    let exp = store.create_experiment(phase, &name)?;
                    if let Some(s) = status {
                        let es = match s.as_str() {
                            "pass" => ExperimentStatus::Pass,
                            "fail" => ExperimentStatus::Fail,
                            "inconclusive" => ExperimentStatus::Inconclusive,
                            _ => ExperimentStatus::Pending,
                        };
                        store.update_experiment_status(exp.id, es, result.as_deref())?;
                    }
                    println!("Experiment #{} added: {}", exp.id, exp.name);
                }
                ExpAction::List { phase } => {
                    let exps = store.list_experiments(phase)?;
                    for e in exps {
                        println!("  #{} [{:?}] {} (phase: {:?})", e.id, e.status, e.name, e.phase_id);
                    }
                }
                ExpAction::Get { id } => {
                    let exp = store.get_experiment(id)?;
                    println!("Experiment #{}: {} [{:?}]", exp.id, exp.name, exp.status);
                    if let Some(r) = &exp.result { println!("  Result: {}", r); }
                    if let Some(n) = &exp.notes { println!("  Notes: {}", n); }
                    if let Some(pid) = exp.phase_id { println!("  Phase: #{}", pid); }
                    // Show findings from this experiment
                    if let Ok(findings) = store.list_findings(Some(exp.id)) {
                        if !findings.is_empty() {
                            println!("  Findings ({}): ", findings.len());
                            for f in &findings {
                                let trunc = if f.text.len() > 80 { &f.text[..80] } else { &f.text };
                                println!("    F#{}: {}", f.id, trunc);
                            }
                        }
                    }
                }
                ExpAction::Update { id, status, result } => {
                    let es = match status.as_str() {
                        "pass" => ExperimentStatus::Pass,
                        "fail" => ExperimentStatus::Fail,
                        "inconclusive" => ExperimentStatus::Inconclusive,
                        _ => ExperimentStatus::Pending,
                    };
                    store.update_experiment_status(id, es, result.as_deref())?;
                    println!("Experiment #{} updated: status={}", id, status);

                    
                }
            }
        }

        Commands::Finding { project, action } => {
            let _proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                FindingAction::Add { text, experiment } => {
                    let f = store.create_finding(experiment, &text)?;
                    println!("Finding #{} added", f.id);

                    // KG quality hints: suggest related findings for edge creation
                    if let Some(eid) = experiment {
                        let siblings = store.list_findings(Some(eid)).unwrap_or_default();
                        let others: Vec<_> = siblings.iter().filter(|s| s.id != f.id).collect();
                        if !others.is_empty() {
                            println!("\n  Nearby findings (same experiment #{}):", eid);
                            for s in others.iter().take(5) {
                                let trunc = if s.text.len() > 70 { &s.text[..70] } else { &s.text };
                                println!("    F#{}: {}", s.id, trunc);
                            }
                            println!("\n  Suggest edges? Examples:");
                            println!("    pm kg {} edge f {} f {} supports", _proj.name, f.id, others[0].id);
                            if others.len() > 1 {
                                println!("    pm kg {} edge f {} f {} contradicts", _proj.name, f.id, others[1].id);
                            }
                        }
                    }

                    // Check text length for lab report quality
                    if f.text.len() < 200 {
                        println!("\n  WARNING: Finding text is {} chars (< 200). Consider expanding to lab report format:", f.text.len());
                        println!("    Methodology | Data | Analysis | Conclusions | Edges");
                    }
                }
                FindingAction::List { experiment } => {
                    let findings = store.list_findings(experiment)?;
                    for f in findings {
                        println!("  #{}: {} (exp: {:?})", f.id, &f.text[..f.text.len().min(80)], f.experiment_id);
                    }
                }
                FindingAction::Update { id, text, experiment } => {
                    // Update finding text and/or experiment
                    let sqlite = "/home/atari2036/gen-ai/RAG/kotaemon-app/kotaemon-app/install_dir/conda/bin/sqlite3";
                    let db = format!("{}/.local/share/pm/pm.db", std::env::var("HOME").unwrap_or_default());
                    if let Some(t) = &text {
                        let escaped = t.replace("'", "''");
                        let sql = format!("UPDATE findings SET text='{}' WHERE id={}", escaped, id);
                        std::process::Command::new(sqlite).args([&db, &sql]).status().ok();
                    }
                    if let Some(eid) = experiment {
                        let sql = format!("UPDATE findings SET experiment_id={} WHERE id={}", eid, id);
                        std::process::Command::new(sqlite).args([&db, &sql]).status().ok();
                    }
                    println!("Finding #{} updated", id);
                }
                FindingAction::Traverse { id, depth } => {
                    let kg = KgEngine::new(&store);
                    let results = kg.traverse_deep(NodeType::Finding, id, depth)?;
                    for r in results {
                        println!("  {} #{}: {}", format!("{:?}", r.root.node_type), r.root.id, &r.root.label[..r.root.label.len().min(60)]);
                        for (edge, target, _incoming) in &r.edges {
                            println!("    --{:?}--> {:?} #{}: {}", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(50)]);
                        }
                    }
                }
            }
        }

        Commands::Dec { project, action } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                DecAction::Add { what, why, experiment } => {
                    let d = store.create_decision(experiment, &what, why.as_deref(), None)?;
                    println!("Decision #{} added: {}", d.id, d.what);
                }
                DecAction::List => {
                    for d in store.list_decisions(proj.id)? {
                        println!("  #{}: {} (exp: {:?})", d.id, d.what, d.experiment_id);
                    }
                }
                DecAction::Get { id } => {
                    let decs = store.list_decisions(proj.id)?;
                    if let Some(d) = decs.iter().find(|d| d.id == id) {
                        println!("Decision #{}: {}", d.id, d.what);
                        if let Some(w) = &d.why { println!("  Why: {}", w); }
                        if let Some(eid) = d.experiment_id { println!("  Experiment: #{}", eid); }
                        // Show edges
                        let kg = KgEngine::new(&store);
                        if let Ok(result) = kg.traverse(NodeType::Decision, d.id) {
                            for (edge, target, incoming) in &result.edges {
                                if *incoming {
                                    println!("  <--{:?}-- {:?} #{}: {}", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(60)]);
                                } else {
                                    println!("  --{:?}--> {:?} #{}: {}", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(60)]);
                                }
                            }
                        }
                    } else {
                        eprintln!("Decision #{} not found", id);
                    }
                }
            }
        }

        Commands::Research { project, action } => {
            let _proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                ResearchAction::Add { name, phase, report } => {
                    let r = store.create_research(phase, &name)?;
                    if let Some(rep) = report {
                        store.update_research(r.id, ResearchStatus::Pending, Some(&rep))?;
                    }
                    println!("Research #{} added: {}", r.id, r.name);
                }
                ResearchAction::List { phase } => {
                    let items = store.list_research(phase)?;
                    for r in items {
                        println!("  #{} [{:?}] {} (phase: {:?})", r.id, r.status, r.name, r.phase_id);
                    }
                }
                ResearchAction::Update { id, status, report } => {
                    let current = store.get_research(id)?;
                    let rs = if let Some(s) = &status {
                        match s.as_str() {
                            "pending" => ResearchStatus::Pending,
                            "in_progress" => ResearchStatus::InProgress,
                            "complete" => ResearchStatus::Complete,
                            _ => return Err(format!("Invalid status: {}", s).into()),
                        }
                    } else {
                        current.status
                    };
                    let rep = report.as_deref().or(current.report.as_deref());
                    store.update_research(id, rs, rep)?;
                    println!("Research #{} updated", id);
                }
            }
        }


        Commands::Principle { project, action } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                PrincipleAction::Add { text, scope } => {
                    let s = match scope.as_str() {
                        "universal" => PrincipleScope::Universal,
                        "phase" => PrincipleScope::Phase,
                        _ => PrincipleScope::Project,
                    };
                    let p = store.create_principle(proj.id, s, &text, None, None)?;
                    println!("Principle #{} added [{:?}]: {}", p.id, p.scope, &p.text[..p.text.len().min(80)]);
                }
                PrincipleAction::List => {
                    for p in store.list_principles(proj.id)? {
                        let sup = if let Some(by) = p.superseded_by { format!(" (superseded by #{})", by) } else { String::new() };
                        println!("  #{} [{:?}/{:?}] {}{}", p.id, p.scope, p.status, &p.text[..p.text.len().min(80)], sup);
                    }
                }
                PrincipleAction::Supersede { id, by } => {
                    store.update_principle_status(id, PrincipleStatus::Superseded, by)?;
                    println!("Principle #{} superseded", id);
                }
            }
        }

        Commands::Hyp { project, action } => {
            let _proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                HypAction::Add { text, phase } => {
                    let h = store.create_hypothesis(phase, &text)?;
                    println!("Hypothesis #{} added: {}", h.id, &h.text[..h.text.len().min(80)]);
                }
                HypAction::List { phase } => {
                    for h in store.list_hypotheses(phase)? {
                        let exp = if let Some(eid) = h.experiment_id { format!(" (exp #{})", eid) } else { String::new() };
                        println!("  #{} [{:?}] {}{}", h.id, h.status, &h.text[..h.text.len().min(80)], exp);
                    }
                }
                HypAction::Test { id, experiment } => {
                    store.update_hypothesis(id, HypothesisStatus::Testing, Some(experiment), None)?;
                    println!("Hypothesis #{} now testing via experiment #{}", id, experiment);
                }
                HypAction::Resolve { id, status, finding } => {
                    let s = match status.as_str() {
                        "confirmed" => HypothesisStatus::Confirmed,
                        "refuted" => HypothesisStatus::Refuted,
                        "proposed" => HypothesisStatus::Proposed,
                        "testing" => HypothesisStatus::Testing,
                        _ => return Err(format!("Invalid status: {} (use confirmed/refuted/proposed/testing)", status).into()),
                    };
                    store.update_hypothesis(id, s, None, finding)?;
                    println!("Hypothesis #{} resolved: {}", id, status);
                }
            }
        }

        Commands::Con { project, action } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                ConAction::Add { text, scope, source } => {
                    let s = match scope.as_str() {
                        "software" => ConstraintScope::Software,
                        "process" => ConstraintScope::Process,
                        _ => ConstraintScope::Hardware,
                    };
                    let c = store.create_constraint(proj.id, s, &text, source.as_deref(), None, None, None, None)?;
                    println!("Constraint #{} added [{:?}]: {}", c.id, c.scope, &c.text[..c.text.len().min(80)]);
                }
                ConAction::List => {
                    for c in store.list_constraints(proj.id)? {
                        let src = c.source.as_deref().unwrap_or("-");
                        println!("  #{} [{:?}] {} (source: {})", c.id, c.scope, &c.text[..c.text.len().min(80)], src);
                    }
                }
            }
        }

        Commands::Lit { project, action } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                LitAction::Add { title, arxiv, relevance, findings } => {
                    let l = store.create_literature(proj.id, &title, arxiv.as_deref(), relevance.as_deref(), findings.as_deref(), None, None, None, None, None, None)?;
                    let aid = l.arxiv_id.as_deref().unwrap_or("-");
                    println!("Literature #{} added: {} [{}]", l.id, l.title, aid);
                }
                LitAction::List => {
                    for l in store.list_literature(proj.id)? {
                        let aid = l.arxiv_id.as_deref().unwrap_or("-");
                        println!("  #{} [{}] {}", l.id, aid, l.title);
                    }
                }
            }
        }

        Commands::Fb { project, action } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                FbAction::Add { text, category } => {
                    let cat = match category.as_str() {
                        "confirmation" => FeedbackCategory::Confirmation,
                        _ => FeedbackCategory::Correction,
                    };
                    let f = store.create_feedback(proj.id, &text, cat)?;
                    println!("Feedback #{} added [{:?}]: {}", f.id, f.category, &f.text[..f.text.len().min(80)]);
                }
                FbAction::List => {
                    for f in store.list_feedback(proj.id)? {
                        println!("  #{} [{:?}] {}", f.id, f.category, &f.text[..f.text.len().min(80)]);
                    }
                }
            }
        }

        Commands::Kg { project, action } => {
            let _proj = resolve_project(&store, &project).ok_or("Project not found")?;
            let kg = KgEngine::new(&store);
            match action {
                KgAction::Map => {
                    let findings = store.list_findings(None)?;
                    println!("=== Knowledge Graph ===\n");
                    println!("Findings: {}", findings.len());
                    let contradictions = kg.find_contradictions(&findings)?;
                    if !contradictions.is_empty() {
                        println!("\nContradictions:");
                        for (a, b) in &contradictions {
                            println!("  #{} vs #{}", a.id, b.id);
                        }
                    }
                }
                KgAction::Traverse { from } => {
                    // Parse "finding:12" format
                    let parts: Vec<&str> = from.split(':').collect();
                    if parts.len() == 2 {
                        let nt = match parts[0] {
                            "finding" => NodeType::Finding,
                            "experiment" => NodeType::Experiment,
                            "decision" => NodeType::Decision,
                            _ => return Err("Unknown node type".into()),
                        };
                        let id: i64 = parts[1].parse()?;
                        let result = kg.traverse(nt, id)?;
                        println!("ROOT: {:?} #{}: {}", result.root.node_type, result.root.id, &result.root.label[..result.root.label.len().min(80)]);
                        for (edge, target, incoming) in &result.edges {
                            if *incoming {
                                println!("  <--{:?}-- {:?} #{}: {}", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(60)]);
                            } else {
                                println!("  --{:?}--> {:?} #{}: {}", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(60)]);
                            }
                        }
                    }
                }
                KgAction::Cluster => {
                    println!("Cluster not yet implemented in v3");
                }
                KgAction::Edge { source_type, source_id, target_type, target_id, relation } => {
                    let st = match source_type.as_str() {
                        "Finding" | "finding" | "f" => NodeType::Finding,
                        "Experiment" | "experiment" | "e" => NodeType::Experiment,
                        "Decision" | "decision" | "d" => NodeType::Decision,
                        "Phase" | "phase" | "p" => NodeType::Phase,
                        "Research" | "research" | "r" => NodeType::Research,
                        "Literature" | "literature" | "l" => NodeType::Literature,
                        _ => return Err(format!("Unknown node type: {}", source_type).into()),
                    };
                    let tt = match target_type.as_str() {
                        "Finding" | "finding" | "f" => NodeType::Finding,
                        "Experiment" | "experiment" | "e" => NodeType::Experiment,
                        "Decision" | "decision" | "d" => NodeType::Decision,
                        "Phase" | "phase" | "p" => NodeType::Phase,
                        "Research" | "research" | "r" => NodeType::Research,
                        "Literature" | "literature" | "l" => NodeType::Literature,
                        _ => return Err(format!("Unknown node type: {}", target_type).into()),
                    };
                    let rel = match relation.as_str() {
                        "Supports" | "supports" => EdgeType::Supports,
                        "Contradicts" | "contradicts" => EdgeType::Contradicts,
                        "DependsOn" | "depends" => EdgeType::DependsOn,
                        "Informed" | "informed" => EdgeType::Informed,
                        "Supersedes" | "supersedes" => EdgeType::Supersedes,
                        "RelatedTo" | "related" => EdgeType::RelatedTo,
                        "ProducedBy" | "produced" => EdgeType::ProducedBy,
                        "CitedIn" | "cited" => EdgeType::CitedIn,
                        _ => return Err(format!("Unknown relation: {}", relation).into()),
                    };
                    let edge = store.create_edge(st, source_id, tt, target_id, rel)?;
                    println!("Edge #{} added: {:?} #{} --{:?}--> {:?} #{}", edge.id, edge.source_type, edge.source_id, edge.relation, edge.target_type, edge.target_id);
                }
                KgAction::Edges => {
                    let edges = store.list_all_edges()?;
                    for e in &edges {
                        println!("  #{}: {:?} #{} --{:?}--> {:?} #{}", e.id, e.source_type, e.source_id, e.relation, e.target_type, e.target_id);
                    }
                    println!("\n{} edges total", edges.len());
                }
                KgAction::Rm { id } => {
                    store.delete_edge(id)?;
                    println!("Edge #{} deleted", id);
                }
                KgAction::From { node_type, node_id } => {
                    let nt = match node_type.as_str() {
                        "Finding" | "finding" | "f" => NodeType::Finding,
                        "Experiment" | "experiment" | "e" => NodeType::Experiment,
                        "Decision" | "decision" | "d" => NodeType::Decision,
                        "Phase" | "phase" | "p" => NodeType::Phase,
                        "Research" | "research" | "r" => NodeType::Research,
                        "Literature" | "literature" | "l" => NodeType::Literature,
                        _ => return Err(format!("Unknown node type: {}", node_type).into()),
                    };
                    let edges = store.get_edges_from(nt.clone(), node_id)?;
                    for e in &edges {
                        println!("  #{}: {:?} #{} --{:?}--> {:?} #{}", e.id, e.source_type, e.source_id, e.relation, e.target_type, e.target_id);
                    }
                    println!("\n{} edges from {:?} #{}", edges.len(), nt, node_id);
                }
                KgAction::To { node_type, node_id } => {
                    let nt = match node_type.as_str() {
                        "Finding" | "finding" | "f" => NodeType::Finding,
                        "Experiment" | "experiment" | "e" => NodeType::Experiment,
                        "Decision" | "decision" | "d" => NodeType::Decision,
                        "Phase" | "phase" | "p" => NodeType::Phase,
                        "Research" | "research" | "r" => NodeType::Research,
                        "Literature" | "literature" | "l" => NodeType::Literature,
                        _ => return Err(format!("Unknown node type: {}", node_type).into()),
                    };
                    let edges = store.get_edges_to(nt.clone(), node_id)?;
                    for e in &edges {
                        println!("  #{}: {:?} #{} --{:?}--> {:?} #{}", e.id, e.source_type, e.source_id, e.relation, e.target_type, e.target_id);
                    }
                    println!("\n{} edges to {:?} #{}", edges.len(), nt, node_id);
                }
            }
        }

        Commands::Next { project } => {
            if let Some(proj) = resolve_project(&store, &project) {
                let dag = DagEngine::new(&store, proj.id);
                let next = dag.next_phases()?;
                println!("=== Next Phases (by impact) ===\n");
                for phase in next.iter().take(3) {
                    let s = if phase.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                    println!("  {} #{} [impact:{}] {}", s, phase.id, phase.impact, phase.name);
                }
                if let Some(n) = dag.stagnation_check(3)? {
                    println!("\n  WARNING: STAGNATION — {} consecutive failed experiments", n);
                }
                println!("\n## ACTION: Execute the top phase.");
            } else { eprintln!("Project not found: {}", project); }
        }

        Commands::Review { project } => {
            if let Some(proj) = resolve_project(&store, &project) {
                let dag = DagEngine::new(&store, proj.id);
                let kg = KgEngine::new(&store);
                let phases = store.list_phases(proj.id)?;
                
                println!("=== Research Review: {} ===\n", proj.name);
                
                // Experiment velocity
                let mut total = 0; let mut pass = 0; let mut fail = 0; let mut pending = 0;
                for phase in &phases {
                    for exp in store.list_experiments(Some(phase.id))? {
                        total += 1;
                        match exp.status {
                            ExperimentStatus::Pass => pass += 1,
                            ExperimentStatus::Fail => fail += 1,
                            ExperimentStatus::Pending => pending += 1,
                            _ => {}
                        }
                    }
                }
                println!("## Experiments: {} total, {} pass, {} fail, {} pending", total, pass, fail, pending);
                
                // Stagnation
                if let Some(n) = dag.stagnation_check(3)? {
                    println!("\n## STAGNATION: {} consecutive fails — REDIRECT needed", n);
                } else {
                    println!("\n## Stagnation: OK");
                }
                
                // Impact
                let next = dag.next_phases()?;
                println!("\n## Top phases by impact:");
                for p in next.iter().take(3) {
                    println!("  #{} [impact:{}] {:?} {}", p.id, p.impact, p.status, p.name);
                }
                
                // Contradictions
                let findings = store.list_findings(None)?;
                let contradictions = kg.find_contradictions(&findings)?;
                if !contradictions.is_empty() {
                    println!("\n## Contradictions: {}", contradictions.len());
                }
                

                // Amdahl check
                let in_progress: Vec<_> = next.iter().filter(|p| p.status == PhaseStatus::InProgress).collect();
                let top_pending: Vec<_> = next.iter().filter(|p| p.status == PhaseStatus::Pending).collect();
                if let Some(active) = in_progress.first() {
                    if let Some(higher) = top_pending.iter().find(|pp| pp.impact > active.impact) {
                        println!("\n## AMDAHL WARNING: Active phase #{} [impact:{}] but phase #{} [impact:{}] has higher impact!", active.id, active.impact, higher.id, higher.impact);
                    }
                }

                // Literature status
                let lit_count = store.list_literature(proj.id).map(|l| l.len()).unwrap_or(0);
                println!("\n## Literature: {} entries tracked", lit_count);
                if lit_count > 0 { println!("  Check for new relevant papers periodically"); }

                // KG connectivity
                let all_edges = store.list_all_edges()?;
                let fids_in_edges: std::collections::HashSet<i64> = all_edges.iter()
                    .filter_map(|e| if format!("{:?}", e.source_type) == "Finding" { Some(e.source_id) } else if format!("{:?}", e.target_type) == "Finding" { Some(e.target_id) } else { None })
                    .collect();
                let disconnected: Vec<_> = findings.iter().filter(|f| !fids_in_edges.contains(&f.id)).collect();
                if !disconnected.is_empty() {
                    println!("\n## KG: {} disconnected findings (no edges)", disconnected.len());
                }

                // Open hypotheses
                if let Ok(hyps) = store.list_hypotheses(None) {
                    let proposed: Vec<_> = hyps.iter().filter(|h| h.status == HypothesisStatus::Proposed).collect();
                    if !proposed.is_empty() {
                        println!("\n## HYPOTHESES: {} untested", proposed.len());
                        for h in proposed.iter().take(3) { let t = if h.text.len() > 60 { &h.text[..60] } else { &h.text }; println!("  H#{}: {}", h.id, t); }
                    }
                }
                println!("\n## ACTION: Address any warnings above.");
            } else { eprintln!("Project not found: {}", project); }
        }

        Commands::Scaffold { project, phase, format } => {
            if let Some(_proj) = resolve_project(&store, &project) {
                let p = store.get_phase(phase)?;
                println!("=== Scaffold: Phase #{} ({}) ===\n", p.id, p.name);
                let exps = store.list_experiments(Some(phase))?;
                let pending: Vec<_> = exps.iter().filter(|e| e.status == ExperimentStatus::Pending).collect();
                if format == "json" {
                    let items: Vec<serde_json::Value> = pending.iter().map(|e| {
                        serde_json::json!({"subject": format!("Exp #{}: {}", e.id, e.name), "description": format!("Phase {} ({})", p.id, p.name)})
                    }).collect();
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else {
                    for e in &pending {
                        println!("  TASK: {} | phase={} | exp={}", e.name, phase, e.id);
                    }
                }
            } else { eprintln!("Project not found: {}", project); }
        }

        Commands::Serve { port } => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(pm::web::serve(&db_path, port));
        }
        Commands::Handoff { project } => {
            println!("{}", handoff_text(&store, &project)?);
        }
        Commands::Import { path, name } => {
            import_v2(&store, &path, &name)?;
        }
    }
    Ok(())
}

fn import_v2(store: &SqliteStore, path: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let data: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let proj = store.create_project(name, None)?;
    println!("Created project: {} (id: {})", name, proj.id);

    let mut phase_map = std::collections::HashMap::new();
    if let Some(phases) = data["phases"].as_array() {
        for p in phases {
            let old_id = p["id"].as_i64().unwrap_or(0);
            let pname = p["name"].as_str().unwrap_or("unnamed");
            let impact = p["impact"].as_i64().unwrap_or(0) as i32;
            let deps: Vec<i64> = p["depends_on"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_i64()).filter_map(|d| phase_map.get(&d).copied()).collect())
                .unwrap_or_default();
            let phase = store.create_phase(proj.id, pname, impact, &deps)?;
            let status = match p["status"].as_str().unwrap_or("pending") {
                "complete" => PhaseStatus::Complete,
                "in_progress" => PhaseStatus::InProgress,
                "deprioritized" => PhaseStatus::Deprioritized,
                "paused" | "paused-at-ceiling" => PhaseStatus::Paused,
                _ => PhaseStatus::Pending,
            };
            store.update_phase_status(phase.id, status)?;
            phase_map.insert(old_id, phase.id);
        }
        println!("  {} phases", phases.len());
    }

    let mut exp_map = std::collections::HashMap::new();
    if let Some(exps) = data["experiments"].as_array() {
        for e in exps {
            let old_id = e["id"].as_i64().unwrap_or(0);
            let ename = e["name"].as_str().unwrap_or("unnamed");
            let new_phase = e["phase"].as_i64().and_then(|op| phase_map.get(&op).copied());
            let exp = store.create_experiment(new_phase, ename)?;
            let status = match e["status"].as_str().unwrap_or("pending") {
                "pass" => ExperimentStatus::Pass,
                "fail" => ExperimentStatus::Fail,
                "pending" => ExperimentStatus::Pending,
                _ => ExperimentStatus::Inconclusive,
            };
            store.update_experiment_status(exp.id, status, e["result"].as_str())?;
            exp_map.insert(old_id, exp.id);
        }
        println!("  {} experiments", exps.len());
    }

    if let Some(findings) = data["findings"].as_array() {
        for f in findings {
            let text = f["text"].as_str().unwrap_or("");
            let new_exp = f["experiment"].as_i64().and_then(|oe| exp_map.get(&oe).copied());
            store.create_finding(new_exp, text)?;
        }
        println!("  {} findings", findings.len());
    }

    if let Some(decs) = data["decisions"].as_array() {
        for d in decs {
            let what = d["what"].as_str().unwrap_or("");
            let why = d["why"].as_str();
            let new_exp = d["experiment"].as_i64().and_then(|oe| exp_map.get(&oe).copied());
            store.create_decision(new_exp, what, why, None)?;
        }
        println!("  {} decisions", decs.len());
    }

    println!("\nImport complete. Run: pm next {}", name);
    Ok(())
}

fn handoff_text(store: &SqliteStore, project: &str) -> Result<String, Box<dyn std::error::Error>> {
    let proj = resolve_project(store, project).ok_or("Project not found")?;
    let dag = pm::dag::DagEngine::new(store, proj.id);
    let _kg = pm::kg::KgEngine::new(store);
    let phases = store.list_phases(proj.id)?;
    
    let mut out = format!("=== Session Handoff: {} ===\n\n", proj.name);
    let in_progress: Vec<_> = phases.iter().filter(|p| p.status == PhaseStatus::InProgress).collect();
    let complete: Vec<_> = phases.iter().filter(|p| p.status == PhaseStatus::Complete).collect();
    
    out += &format!("## Progress: {}/{} phases complete\n", complete.len(), phases.len());
    if !in_progress.is_empty() {
        out += "\n## Active:\n";
        for p in &in_progress { out += &format!("  Phase #{} [impact:{}]: {}\n", p.id, p.impact, p.name); }
    }
    if let Ok(next) = dag.next_phases() {
        if let Some(top) = next.first() {
            out += &format!("\n## Next Action: Phase #{} [impact:{}] {}\n", top.id, top.impact, top.name);
        }
    }
    if let Ok(Some(n)) = dag.stagnation_check(3) {
        out += &format!("\n## WARNING: {} consecutive fails — redirect needed\n", n);
    }
    let all_findings = store.list_findings(None)?;
    if !all_findings.is_empty() {
        out += "\n## Recent Findings:\n";
        for f in all_findings.iter().rev().take(3) {
            let trunc = if f.text.len() > 100 { &f.text[..100] } else { &f.text };
            out += &format!("  #{}: {}\n", f.id, trunc);
        }
    }
    Ok(out)
}
