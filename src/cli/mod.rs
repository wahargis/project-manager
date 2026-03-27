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
    /// Research / reflection management
    Research {
        project: String,
        #[command(subcommand)]
        action: ResearchAction,
    },
    /// Principle management (project-level guidance)
    Principle {
        project: String,
        #[command(subcommand)]
        action: PrincipleAction,
    },
    /// Hypothesis management (testable predictions)
    Hyp {
        project: String,
        #[command(subcommand)]
        action: HypAction,
    },
    /// Constraint management (hard boundaries)
    Con {
        project: String,
        #[command(subcommand)]
        action: ConAction,
    },
    /// Literature management (citations)
    Lit {
        project: String,
        #[command(subcommand)]
        action: LitAction,
    },
    /// Feedback management (corrections/confirmations)
    Fb {
        project: String,
        #[command(subcommand)]
        action: FbAction,
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
    /// Start web dashboard server
    Serve {
        #[arg(long, default_value_t = 9090)]
        port: u16,
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
    /// Add a KG edge
    Edge {
        /// Source node type (Finding, Experiment, Decision, Phase, Research)
        source_type: String,
        /// Source node ID
        source_id: i64,
        /// Target node type
        target_type: String,
        /// Target node ID
        target_id: i64,
        /// Relation (Supports, Contradicts, DependsOn, Informed, Supersedes, RelatedTo, ProducedBy, CitedIn)
        relation: String,
    },
    /// List all KG edges
    Edges,
    /// List edges FROM a specific node
    From {
        /// Source node type (f/e/d/p/r/l)
        node_type: String,
        /// Source node ID
        node_id: i64,
    },
    /// List edges TO a specific node
    To {
        /// Target node type (f/e/d/p/r/l)
        node_type: String,
        /// Target node ID
        node_id: i64,
    },
    /// Delete a KG edge by ID
    Rm {
        /// Edge ID to delete
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum ResearchAction {
    Add { name: String, #[arg(long)] phase: Option<i64>, #[arg(long)] report: Option<String> },
    List { #[arg(long)] phase: Option<i64> },
    Update { id: i64, #[arg(long)] status: Option<String>, #[arg(long)] report: Option<String> },
}

#[derive(Subcommand)]
pub enum PrincipleAction {
    Add { text: String, #[arg(long, default_value = "project")] scope: String },
    List,
    Supersede { id: i64, #[arg(long)] by: Option<i64> },
}

#[derive(Subcommand)]
pub enum HypAction {
    Add { text: String, #[arg(long)] phase: Option<i64> },
    List { #[arg(long)] phase: Option<i64> },
    Test { id: i64, #[arg(long)] experiment: i64 },
    Resolve { id: i64, #[arg(long)] status: String, #[arg(long)] finding: Option<i64> },
}

#[derive(Subcommand)]
pub enum ConAction {
    Add { text: String, #[arg(long, default_value = "hardware")] scope: String, #[arg(long)] source: Option<String> },
    List,
}

#[derive(Subcommand)]
pub enum LitAction {
    Add { title: String, #[arg(long)] arxiv: Option<String>, #[arg(long)] relevance: Option<String>, #[arg(long)] findings: Option<String> },
    List,
}

#[derive(Subcommand)]
pub enum FbAction {
    Add { text: String, #[arg(long, default_value = "correction")] category: String },
    List,
}

