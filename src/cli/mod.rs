use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pm", about = "Research project management for long-horizon agentic work")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Database path (default: ~/.local/share/pm/pm.db)
    #[arg(short, long, global = true)]
    pub db: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Project management
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Phase management
    Phase {
        /// Project name or alias
        project: String,
        #[command(subcommand)]
        action: PhaseAction,
    },
    /// Experiment management
    Exp {
        project: String,
        #[command(subcommand)]
        action: ExpAction,
    },
    /// Finding management
    Finding {
        project: String,
        #[command(subcommand)]
        action: FindingAction,
    },
    /// Decision management
    Dec {
        project: String,
        #[command(subcommand)]
        action: DecAction,
    },
    /// Knowledge graph
    Kg {
        project: String,
        #[command(subcommand)]
        action: KgAction,
    },
    /// Cross-project dashboard
    Dashboard,
    /// Next actions for a project
    Next {
        project: String,
    },
    /// Research review
    Review {
        project: String,
    },
    /// Scaffold phase into task items
    Scaffold {
        project: String,
        #[arg(long)]
        phase: i64,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import v2 JSON project data
    /// Generate session handoff document
    Handoff {
        project: String,
    },
    Import {
        /// Path to v2 project.json
        path: String,
        /// Project name
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ProjectAction {
    List,
    Activate { name: String, #[arg(short, long)] alias: Option<String> },
    Pause { name: String },
    Archive { name: String },
}

#[derive(Subcommand)]
pub enum PhaseAction {
    Add { name: String, #[arg(long, default_value_t = 0)] impact: i32, #[arg(long, value_delimiter = ',')] depends: Option<Vec<i64>> },
    List,
    Update { id: i64, #[arg(long)] status: String },
    Get { id: i64 },
}

#[derive(Subcommand)]
pub enum ExpAction {
    Add { name: String, #[arg(long)] phase: Option<i64>, #[arg(long)] status: Option<String>, #[arg(long)] result: Option<String> },
    List { #[arg(long)] phase: Option<i64> },
    Update { id: i64, #[arg(long)] status: String, #[arg(long)] result: Option<String> },
}

#[derive(Subcommand)]
pub enum FindingAction {
    Add { text: String, #[arg(long)] experiment: Option<i64> },
    List { #[arg(long)] experiment: Option<i64> },
    Traverse { id: i64, #[arg(long, default_value_t = 1)] depth: usize },
}

#[derive(Subcommand)]
pub enum DecAction {
    Add { what: String, #[arg(long)] why: Option<String>, #[arg(long)] experiment: Option<i64> },
    List,
}

#[derive(Subcommand)]
pub enum KgAction {
    Map,
    Traverse { #[arg(long)] from: String },
    Cluster,
}
