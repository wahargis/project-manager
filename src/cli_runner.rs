use pm::util::truncate_safe;
use pm::cli::*;
use pm::store::sqlite::SqliteStore;
use pm::store::{Store, PhaseStatus, ExperimentStatus, ResearchStatus, NodeType, EdgeType, PrincipleScope, PrincipleStatus, HypothesisStatus, ConstraintScope, FeedbackCategory};
use pm::dag::DagEngine;
use pm::kg::KgEngine;
use pm::analysis::confidence;

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


fn print_edges(store: &SqliteStore, node_type: NodeType, node_id: i64) {
    let from_edges = store.get_edges_from(node_type.clone(), node_id).unwrap_or_default();
    let to_edges = store.get_edges_to(node_type.clone(), node_id).unwrap_or_default();
    if !from_edges.is_empty() || !to_edges.is_empty() {
        println!("  Edges:");
        for e in &from_edges {
            println!("    --{:?}--> {:?} #{}", e.relation, e.target_type, e.target_id);
        }
        for e in &to_edges {
            println!("    <--{:?}-- {:?} #{}", e.relation, e.source_type, e.source_id);
        }
        println!("    ({} outgoing, {} incoming)", from_edges.len(), to_edges.len());
    }
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
            let mut research = 0;
            let mut hypotheses = 0;
            for p in &phases {
                research += store.list_research(Some(p.id)).map(|r| r.len()).unwrap_or(0);
                hypotheses += store.list_hypotheses(Some(p.id)).map(|h| h.len()).unwrap_or(0);
            }
            let principles = store.list_principles(proj.id).map(|p| p.len()).unwrap_or(0);
            let constraints = store.list_constraints(proj.id).map(|c| c.len()).unwrap_or(0);
            let literature = store.list_literature(proj.id).map(|l| l.len()).unwrap_or(0);
            let feedback_count = store.list_feedback(proj.id).map(|f| f.len()).unwrap_or(0);
            // Build set of all node IDs in this project for edge filtering
            let mut project_node_ids: std::collections::HashSet<(NodeType, i64)> = std::collections::HashSet::new();
            for p in &phases {
                project_node_ids.insert((NodeType::Phase, p.id));
                for e in store.list_experiments(Some(p.id)).unwrap_or_default() {
                    project_node_ids.insert((NodeType::Experiment, e.id));
                    for f in store.list_findings(Some(e.id)).unwrap_or_default() {
                        project_node_ids.insert((NodeType::Finding, f.id));
                    }
                }
                for r in store.list_research(Some(p.id)).unwrap_or_default() {
                    project_node_ids.insert((NodeType::Research, r.id));
                }
                for h in store.list_hypotheses(Some(p.id)).unwrap_or_default() {
                    project_node_ids.insert((NodeType::Hypothesis, h.id));
                }
            }
            for d in store.list_decisions(proj.id).unwrap_or_default() {
                project_node_ids.insert((NodeType::Decision, d.id));
            }
            for p in store.list_principles(proj.id).unwrap_or_default() {
                project_node_ids.insert((NodeType::Principle, p.id));
            }
            for c in store.list_constraints(proj.id).unwrap_or_default() {
                project_node_ids.insert((NodeType::Constraint, c.id));
            }
            for l in store.list_literature(proj.id).unwrap_or_default() {
                project_node_ids.insert((NodeType::Literature, l.id));
            }
            let edges = store.list_all_edges().map(|edges| {
                edges.iter().filter(|e| {
                    project_node_ids.contains(&(e.source_type.clone(), e.source_id))
                        || project_node_ids.contains(&(e.target_type.clone(), e.target_id))
                }).count()
            }).unwrap_or(0);

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

            // Group projects by parent for hierarchical display
            let active: Vec<_> = projects.iter().filter(|p| p.status == pm::store::ProjectStatus::Active).collect();
            let parents: Vec<_> = active.iter().filter(|p| p.parent_id.is_none()).collect();
            let children: Vec<_> = active.iter().filter(|p| p.parent_id.is_some()).collect();

            for parent in &parents {
                let subs: Vec<_> = children.iter().filter(|c| c.parent_id == Some(parent.id)).collect();
                if subs.is_empty() {
                    // Standalone project (no children) -- show inline
                    let dag = DagEngine::new(&store, parent.id);
                    if let Ok(next) = dag.next_phases() {
                        let top = next.iter().find(|p| p.status == PhaseStatus::InProgress)
                            .or_else(|| next.iter().find(|p| p.status == PhaseStatus::Pending));
                        if let Some(top) = top {
                            let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                            let pref = top.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", top.id));
                            let phase_ctx = dashboard_phase_context(&store, top.id);
                            println!("  [{}] {} {} [impact:{}] {}{}", parent.name, s, pref, top.impact, top.name, phase_ctx);
                        }
                        if let Some(p) = next.iter().find(|p| p.status == PhaseStatus::Paused) {
                            let pref = p.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", p.id));
                            println!("  [{}] PAUSED {} [impact:{}] {}", parent.name, pref, p.impact, p.name);
                        }
                    }
                } else {
                    // Parent with subprojects -- group header
                    println!("## {}", parent.name);
                    // Show parent's own phases first
                    let dag = DagEngine::new(&store, parent.id);
                    if let Ok(next) = dag.next_phases() {
                        let top = next.iter().find(|p| p.status == PhaseStatus::InProgress)
                            .or_else(|| next.iter().find(|p| p.status == PhaseStatus::Pending));
                        if let Some(top) = top {
                            let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                            let pref = top.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", top.id));
                            println!("  [{}] {} {} [impact:{}] {}", parent.name, s, pref, top.impact, top.name);
                        }
                    }
                    // Show subprojects indented
                    for sub in &subs {
                        let dag = DagEngine::new(&store, sub.id);
                        if let Ok(next) = dag.next_phases() {
                            let top = next.iter().find(|p| p.status == PhaseStatus::InProgress)
                                .or_else(|| next.iter().find(|p| p.status == PhaseStatus::Pending));
                            if let Some(top) = top {
                                let s = if top.status == PhaseStatus::InProgress { "IN-PROGRESS" } else { "NEXT" };
                                let pref = top.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", top.id));
                                println!("  [{}/{}] {} {} [impact:{}] {}", parent.name, sub.name, s, pref, top.impact, top.name);
                            }
                            if let Some(p) = next.iter().find(|p| p.status == PhaseStatus::Paused) {
                                let pref = p.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", p.id));
                                println!("  [{}/{}] PAUSED {} [impact:{}] {}", parent.name, sub.name, pref, p.impact, p.name);
                            }
                        }
                    }
                    println!();
                }
            }
            println!("\n## ACTION: Execute the highest-impact item above.");
        }

        Commands::Project { action } => match action {
            ProjectAction::List => {
                let all = store.list_projects()?;
                for p in &all {
                    let parent_info = if let Some(pid) = p.parent_id {
                        all.iter().find(|pp| pp.id == pid)
                            .map(|pp| format!(" [parent: {}]", pp.name))
                            .unwrap_or_else(|| format!(" [parent: #{}]", pid))
                    } else {
                        String::new()
                    };
                    println!("  {} ({:?}) [alias: {}]{}", p.name, p.status, p.alias.as_deref().unwrap_or("-"), parent_info);
                }
            }
            ProjectAction::Activate { name, alias, parent } => {
                let parent_id = if let Some(parent_name) = &parent {
                    let parent_proj = resolve_project(&store, parent_name)
                        .ok_or(format!("Parent project not found: {}", parent_name))?;
                    Some(parent_proj.id)
                } else {
                    None
                };
                let p = store.create_project(&name, alias.as_deref(), parent_id)?;
                if let Some(pid) = parent_id {
                    println!("Activated: {} (id: {}, parent: #{})", p.name, p.id, pid);
                } else {
                    println!("Activated: {} (id: {})", p.name, p.id);
                }
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
            ProjectAction::Subs { name } => {
                let proj = resolve_project(&store, &name).ok_or("Project not found")?;
                let subs = store.list_subprojects(proj.id)?;
                if subs.is_empty() {
                    println!("No subprojects for {}", proj.name);
                } else {
                    println!("=== Subprojects of {} ===
", proj.name);
                    for s in &subs {
                        println!("  {} ({:?}) [alias: {}]", s.name, s.status, s.alias.as_deref().unwrap_or("-"));
                    }
                }
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
                        let pref = p.project_seq.map(|s| format!("#{}", s)).unwrap_or_else(|| format!("#{}", p.id));
                    println!("  {} [{:?}] [impact:{}] {}{}", pref, p.status, p.impact, p.name, deps);
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
                ExpAction::List { phase, verbose } => {
                    let exps = if phase.is_some() {
                        store.list_experiments(phase)?
                    } else {
                        // Scope by project: collect experiments from all project phases
                        let mut all = Vec::new();
                        for p in store.list_phases(_proj.id)? {
                            all.extend(store.list_experiments(Some(p.id))?);
                        }
                        all
                    };
                    for e in exps {
                        println!("  #{} [{:?}] {} (phase: {:?})", e.id, e.status, e.name, e.phase_id);
                        if verbose {
                            if let Some(r) = &e.result { println!("    Result: {}", r); }
                            if let Some(n) = &e.notes { println!("    Notes: {}", n); }
                            if let Some(h) = &e.hypothesis { println!("    Hypothesis: {}", h); }
                        }
                    }
                }
                ExpAction::Get { id } => {
                    let exp = store.get_experiment(id)?;
                    println!("Experiment #{}: {} [{:?}]", exp.id, exp.name, exp.status);
                    if let Some(r) = &exp.result { println!("  Result: {}", r); }
                    if let Some(n) = &exp.notes { println!("  Notes: {}", n); }
                    if let Some(pid) = exp.phase_id { println!("  Phase: #{}", pid); }
                    println!("  Created: {}", exp.created_at);
                    // Show findings from this experiment
                    let exp_findings = store.list_findings(Some(exp.id)).unwrap_or_default();
                    if !exp_findings.is_empty() {
                        println!("  Findings ({}): ", exp_findings.len());
                        for f in &exp_findings {
                            let trunc = truncate_safe(&f.text, 80);
                            println!("    F#{}: {}", f.id, trunc);
                        }
                    }
                    // Statistical confidence scoring (MAD-based)
                    if let Some(conf) = confidence::compute_experiment_confidence(&exp_findings) {
                        print!("{}", conf.display());
                    }
                    print_edges(&store, NodeType::Experiment, exp.id);
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
                                let trunc = truncate_safe(&s.text, 70);
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
                FindingAction::List { experiment, verbose } => {
                    let findings = if experiment.is_some() {
                        store.list_findings(experiment)?
                    } else {
                        let mut all = Vec::new();
                        for phase in store.list_phases(_proj.id)? {
                            for exp in store.list_experiments(Some(phase.id))? {
                                all.extend(store.list_findings(Some(exp.id))?);
                            }
                        }
                        all
                    };
                    for f in &findings {
                        if verbose {
                            println!("  #{}: {} (exp: {:?})", f.id, &f.text, f.experiment_id);
                        } else {
                            println!("  #{}: {} (exp: {:?})", f.id, truncate_safe(&f.text, 80), f.experiment_id);
                        }
                    }
                }
                FindingAction::Get { id } => {
                    let f = store.get_finding(id)?;
                    println!("Finding #{}:", f.id);
                    println!("  Text: {}", f.text);
                    if let Some(eid) = f.experiment_id { println!("  Experiment: #{}", eid); }
                    if let Some(c) = f.confidence { println!("  Confidence: {:.2}", c); }
                    if let Some(b) = &f.belief_status { println!("  Belief status: {}", b); }
                    println!("  Created: {}", f.created_at);
                    print_edges(&store, NodeType::Finding, f.id);
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
                        println!("  {} #{}: {}", format!("{:?}", r.root.node_type), r.root.id, truncate_safe(&r.root.label, 60));
                        for (edge, target, _incoming) in &r.edges {
                            println!("    --{:?}--> {:?} #{}: {}", edge.relation, target.node_type, target.id, truncate_safe(&target.label, 50));
                        }
                    }
                }
            }
        }

        Commands::Dec { project, action } => {
            let proj = resolve_project(&store, &project).ok_or("Project not found")?;
            match action {
                DecAction::Add { what, why, experiment } => {
                    use pm::validation;
                    let v = validation::validate_decision(&what, why.as_deref());
                    if !v.is_ok() {
                        eprintln!("Validation error:\n{}", v.to_mcp_error());
                        return Ok(());
                    }
                    // #34: Anti-cleanup guardrail
                    if let Some(matched) = pm::mcp::nodes::cleanup_guard_check(&what) {
                        println!("\n\u{26a0}\u{fe0f} CLEANUP GUARD: This decision contains closure/pruning language ({}).", matched);
                        println!("Research phases and experiments with negative results are valuable \u{2014} they narrow the search space.");
                        println!("If this is an explicit user request to close/deprioritize, proceed. Otherwise, consider reframing");
                        println!("as a redirect (what NEW direction does this suggest?) rather than a closure.\n");
                    }
                    let d = store.create_decision(experiment, &what, why.as_deref(), None)?;
                    println!("Decision #{} added: {}", d.id, d.what);
                }
                DecAction::List => {
                    for d in store.list_decisions(proj.id)? {
                        println!("  #{}: {} (exp: {:?})", d.id, d.what, d.experiment_id);
                    }
                }
                DecAction::Get { id } => {
                    let d = store.get_decision(id)?;
                    println!("Decision #{}:", d.id);
                    println!("  What: {}", d.what);
                    if let Some(w) = &d.why { println!("  Why: {}", w); }
                    if let Some(eid) = d.experiment_id { println!("  Experiment: #{}", eid); }
                    if let Some(pid) = d.project_id { println!("  Project: #{}", pid); }
                    if let Some(c) = d.confidence { println!("  Confidence: {:.2}", c); }
                    if let Some(b) = &d.belief_status { println!("  Belief status: {}", b); }
                    println!("  Created: {}", d.created_at);
                    print_edges(&store, NodeType::Decision, d.id);
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
                    println!("Principle #{} added [{:?}]: {}", p.id, p.scope, truncate_safe(&p.text, 80));
                }
                PrincipleAction::List => {
                    for p in store.list_principles(proj.id)? {
                        let sup = if let Some(by) = p.superseded_by { format!(" (superseded by #{})", by) } else { String::new() };
                        println!("  #{} [{:?}/{:?}] {}{}", p.id, p.scope, p.status, truncate_safe(&p.text, 80), sup);
                    }
                }
                PrincipleAction::Get { id } => {
                    let p = store.get_principle(id)?;
                    println!("Principle #{}:", p.id);
                    println!("  Text: {}", p.text);
                    println!("  Scope: {:?}", p.scope);
                    println!("  Status: {:?}", p.status);
                    if let Some(r) = &p.rationale { println!("  Rationale: {}", r); }
                    if let Some(e) = &p.enforcement_level { println!("  Enforcement: {}", e); }
                    if let Some(c) = p.confidence { println!("  Confidence: {:.2}", c); }
                    if let Some(b) = &p.belief_status { println!("  Belief status: {}", b); }
                    if let Some(by) = p.superseded_by { println!("  Superseded by: #{}", by); }
                    println!("  Created: {}", p.created_at);
                    print_edges(&store, NodeType::Principle, p.id);
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
                HypAction::Add { text, phase, finding } => {
                    let h = store.create_hypothesis(phase, &text)?;
                    let mut out = format!("Hypothesis #{} added: {}", h.id, truncate_safe(&h.text, 80));

                    // Causal grounding: auto-create Finding --Supports--> Hypothesis edge
                    if let Some(fid) = finding {
                        match store.create_edge(NodeType::Finding, fid, NodeType::Hypothesis, h.id, EdgeType::Supports) {
                            Ok(_) => out += &format!("\n  Auto-edge: Finding#{} --Supports--> Hypothesis#{}", fid, h.id),
                            Err(e) => out += &format!("\n  Edge note: {}", e),
                        }
                    } else {
                        // Soft warning: hypothesis without informing finding
                        out += "\n  \u{26a0}\u{fe0f} WARNING: Hypothesis has no informing finding. Consider linking:";
                        out += &format!("\n    pm kg <project> edge Finding ? Hypothesis {} Supports", h.id);
                    }

                    println!("{}", out);
                }
                HypAction::List { phase } => {
                    for h in store.list_hypotheses(phase)? {
                        let exp = if let Some(eid) = h.experiment_id { format!(" (exp #{})", eid) } else { String::new() };
                        println!("  #{} [{:?}] {}{}", h.id, h.status, truncate_safe(&h.text, 80), exp);
                    }
                }
                HypAction::Get { id } => {
                    let h = store.get_hypothesis(id)?;
                    println!("Hypothesis #{}:", h.id);
                    println!("  Text: {}", h.text);
                    println!("  Status: {:?}", h.status);
                    if let Some(p) = &h.prediction { println!("  Prediction: {}", p); }
                    if let Some(c) = &h.criteria { println!("  Criteria: {}", c); }
                    if let Some(c) = h.confidence { println!("  Confidence: {:.2}", c); }
                    if let Some(eid) = h.experiment_id { println!("  Experiment: #{}", eid); }
                    if let Some(fid) = h.finding_id { println!("  Finding: #{}", fid); }
                    if let Some(b) = &h.belief_status { println!("  Belief status: {}", b); }
                    println!("  Created: {}", h.created_at);
                    print_edges(&store, NodeType::Hypothesis, h.id);
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
                    println!("Constraint #{} added [{:?}]: {}", c.id, c.scope, truncate_safe(&c.text, 80));
                }
                ConAction::List => {
                    for c in store.list_constraints(proj.id)? {
                        let src = c.source.as_deref().unwrap_or("-");
                        println!("  #{} [{:?}] {} (source: {})", c.id, c.scope, truncate_safe(&c.text, 80), src);
                    }
                }
                ConAction::Get { id } => {
                    let c = store.get_constraint(id)?;
                    println!("Constraint #{}:", c.id);
                    println!("  Text: {}", c.text);
                    println!("  Scope: {:?}", c.scope);
                    if let Some(s) = &c.severity { println!("  Severity: {}", s); }
                    if let Some(s) = &c.source { println!("  Source: {}", s); }
                    if let Some(r) = &c.resource { println!("  Resource: {}", r); }
                    if let Some(m) = &c.measured_value { println!("  Measured value: {}", m); }
                    if let Some(cf) = c.confidence { println!("  Confidence: {:.2}", cf); }
                    if let Some(b) = &c.belief_status { println!("  Belief status: {}", b); }
                    println!("  Created: {}", c.created_at);
                    print_edges(&store, NodeType::Constraint, c.id);
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
                        let status = l.status.as_deref().unwrap_or("unread");
                        println!("  #{} [{}] [{}] {}", l.id, aid, status, l.title);
                    }
                }
                LitAction::Get { id } => {
                    let l = store.get_literature(id)?;
                    println!("Literature #{}:", l.id);
                    println!("  Title: {}", l.title);
                    if let Some(a) = &l.arxiv_id { println!("  ArXiv: {}", a); }
                    if let Some(a) = &l.authors { println!("  Authors: {}", a); }
                    if let Some(v) = &l.venue { println!("  Venue: {}", v); }
                    if let Some(y) = l.year { println!("  Year: {}", y); }
                    if let Some(s) = &l.status { println!("  Status: {}", s); }
                    if let Some(r) = &l.relevance { println!("  Relevance: {}", r); }
                    if let Some(f) = &l.key_findings { println!("  Key findings: {}", f); }
                    if let Some(s) = &l.summary { println!("  Summary: {}", s); }
                    if let Some(u) = &l.url { println!("  URL: {}", u); }
                    if let Some(u) = &l.code_url { println!("  Code URL: {}", u); }
                    if let Some(f) = &l.file_path { println!("  File: {}", f); }
                    println!("  Created: {}", l.created_at);
                    print_edges(&store, NodeType::Literature, l.id);
                }
                LitAction::Status { id, status } => {
                    let result = pm::mcp::nodes::tool_lit_status(&store, id, &status);
                    println!("{}", result);
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
                    println!("Feedback #{} added [{:?}]: {}", f.id, f.category, truncate_safe(&f.text, 80));
                }
                FbAction::List => {
                    for f in store.list_feedback(proj.id)? {
                        println!("  #{} [{:?}] {}", f.id, f.category, truncate_safe(&f.text, 80));
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
                        println!("ROOT: {:?} #{}: {}", result.root.node_type, result.root.id, truncate_safe(&result.root.label, 80));
                        for (edge, target, incoming) in &result.edges {
                            if *incoming {
                                println!("  <--{:?}-- {:?} #{}: {}", edge.relation, target.node_type, target.id, truncate_safe(&target.label, 60));
                            } else {
                                println!("  --{:?}--> {:?} #{}: {}", edge.relation, target.node_type, target.id, truncate_safe(&target.label, 60));
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
                        "Hypothesis" | "hypothesis" | "h" => NodeType::Hypothesis,
                        "Principle" | "principle" | "pr" => NodeType::Principle,
                        "Constraint" | "constraint" | "c" => NodeType::Constraint,
                        "Feedback" | "feedback" | "fb" => NodeType::Feedback,
                        _ => return Err(format!("Unknown node type: {}", source_type).into()),
                    };
                    let tt = match target_type.as_str() {
                        "Finding" | "finding" | "f" => NodeType::Finding,
                        "Experiment" | "experiment" | "e" => NodeType::Experiment,
                        "Decision" | "decision" | "d" => NodeType::Decision,
                        "Phase" | "phase" | "p" => NodeType::Phase,
                        "Research" | "research" | "r" => NodeType::Research,
                        "Literature" | "literature" | "l" => NodeType::Literature,
                        "Hypothesis" | "hypothesis" | "h" => NodeType::Hypothesis,
                        "Principle" | "principle" | "pr" => NodeType::Principle,
                        "Constraint" | "constraint" | "c" => NodeType::Constraint,
                        "Feedback" | "feedback" | "fb" => NodeType::Feedback,
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
                        "Contains" | "contains" => EdgeType::Contains,
                        "DerivedFrom" | "derived" => EdgeType::DerivedFrom,
                        "TestedBy" | "tested_by" => EdgeType::TestedBy,
                        "ViolatedBy" | "violated_by" => EdgeType::ViolatedBy,
                        "BranchesFrom" | "branches" => EdgeType::BranchesFrom,
                        "ConvergesInto" | "converges" => EdgeType::ConvergesInto,
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
            // Delegate to MCP review implementation for orphan detection + constraint expiry
            let output = pm::mcp::review::tool_review(&store, &project);
            println!("{}", output);
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

        Commands::OrphanRepair { project } => {
            let output = pm::mcp::review::tool_orphan_repair(&store, &project);
            println!("{}", output);
        }

        Commands::KgAudit { project } => {
            println!("{}", pm::mcp::review::tool_kg_audit(&store, &project));
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
        Commands::Search { project, query, all } => {
            // v6 fix: thread project_filter through tool_search so --all actually
            // produces per-project (not duplicated global) results, AND so the
            // single-project case respects the project boundary.
            if all {
                let projects = store.list_projects()?;
                let active: Vec<_> = projects.iter()
                    .filter(|p| p.status == pm::store::ProjectStatus::Active)
                    .collect();
                if active.is_empty() {
                    println!("No active projects found.");
                } else {
                    for proj in &active {
                        println!("=== {} ===", proj.name);
                        let output = pm::mcp::review::tool_search_with_filter(&store, &query, Some(proj.id));
                        println!("{}", output);
                    }
                }
            } else {
                let proj = resolve_project(&store, &project).ok_or("Project not found")?;
                let output = pm::mcp::review::tool_search_with_filter(&store, &query, Some(proj.id));
                println!("{}", output);
            }
        }
        Commands::Context { topic, limit, json } => {
            let output = pm::mcp::review::tool_context(&store, &topic, limit);
            if json {
                let v = serde_json::json!({ "topic": topic, "limit": limit, "output": output });
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| output.clone()));
            } else {
                println!("{}", output);
            }
        }
        Commands::Query { text, json } => {
            let output = pm::mcp::review::tool_query(&store, &text);
            if json {
                let v = serde_json::json!({ "query": text, "output": output });
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| output.clone()));
            } else {
                println!("{}", output);
            }
        }
        Commands::SessionInit { json } => {
            let output = pm::mcp::dashboard::tool_session_init(&store);
            if json {
                let v = serde_json::json!({ "briefing": output });
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| output.clone()));
            } else {
                println!("{}", output);
            }
        }
    }
    Ok(())
}

fn import_v2(store: &SqliteStore, path: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let data: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let proj = store.create_project(name, None, None)?;
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
            let trunc = truncate_safe(&f.text, 100);
            out += &format!("  #{}: {}\n", f.id, trunc);
        }
    }
    Ok(out)
}fn dashboard_phase_context(store: &SqliteStore, phase_id: i64) -> String {
    let mut ctx = String::new();
    let mut latest_finding: Option<(i64, String)> = None;
    let mut pending_count = 0usize;
    let mut pass_count = 0usize;
    let mut fail_count = 0usize;
    let mut pending_names: Vec<String> = Vec::new();

    if let Ok(exps) = store.list_experiments(Some(phase_id)) {
        for exp in &exps {
            match exp.status {
                ExperimentStatus::Pending => pending_count += 1,
                ExperimentStatus::Pass => pass_count += 1,
                ExperimentStatus::Fail => fail_count += 1,
                _ => {}
            }

            let mut max_finding_in_exp: i64 = 0;
            if let Ok(findings) = store.list_findings(Some(exp.id)) {
                for f in &findings {
                    if f.id as i64 > max_finding_in_exp {
                        max_finding_in_exp = f.id as i64;
                    }
                    match &latest_finding {
                        None => latest_finding = Some((f.id, truncate_safe(&f.text, 80).to_string())),
                        Some((old_id, _)) if f.id > *old_id => {
                            latest_finding = Some((f.id, truncate_safe(&f.text, 80).to_string()));
                        }
                        _ => {}
                    }
                }
            }

            if exp.status == ExperimentStatus::Pending {
                let fc = store.list_findings(Some(exp.id)).map(|f| f.len()).unwrap_or(0);
                let label = if fc > 0 {
                    format!("E#{} ({} findings)", exp.id, fc)
                } else {
                    format!("E#{}", exp.id)
                };
                pending_names.push(label);
            }
        }

        // Phase lifecycle signals
        let total = pending_count + pass_count + fail_count;
        if total > 0 {
            ctx += &format!("\n      Experiments: {} pending, {} pass, {} fail", pending_count, pass_count, fail_count);
            if pending_count == 0 {
                ctx += " -- REVIEW NEEDED (all resolved)";
            }
        }

        // Show ALL pending experiments with finding counts — inform, dont decide
        if !pending_names.is_empty() {
            if pending_names.len() == 1 {
                ctx += &format!("\n      Pending: {}", pending_names[0]);
            } else {
                ctx += &format!("\n      Pending ({}): {}", pending_names.len(),
                    pending_names.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
                if pending_names.len() > 4 {
                    ctx += &format!(", +{} more", pending_names.len() - 4);
                }
            }
        }
    }

    // Untested hypotheses count
    if let Ok(hyps) = store.list_hypotheses(Some(phase_id)) {
        let untested = hyps.iter().filter(|h| h.status == HypothesisStatus::Proposed).count();
        if untested > 0 {
            ctx += &format!("\n      {} untested hypothesis(es)", untested);
        }
    }
    if let Some((fid, text)) = latest_finding {
        ctx += &format!("\n      Latest: F#{} {}", fid, text);
    }
    ctx
}


