//! MCP tool schema definitions.
//!
//! Contains the ToolDef struct and tool_definitions() function that
//! returns the JSON schema for every MCP tool exposed by project-manager.

use serde::Serialize;

#[derive(Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Define all MCP tool schemas.
pub fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "pm_dashboard".into(),
            description: "Cross-project priority dashboard. Shows highest-impact action across all active projects.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "pm_next".into(),
            description: "Next actions for a project with experiment summary, stagnation warning, and TaskCreate-ready top action.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "project": { "type": "string", "description": "Project name or alias" } },
                "required": ["project"]
            }),
        },
        ToolDef {
            name: "pm_review".into(),
            description: "Research health check: experiment velocity, stagnation, impact assessment, contradictions, orphaned nodes (all types), expired constraints.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "project": { "type": "string" } },
                "required": ["project"]
            }),
        },
        ToolDef {
            name: "pm_kg_traverse".into(),
            description: "Traverse KG from a node. Shows connected edges and nodes with direction.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"node_type": {"type": "string"}, "node_id": {"type": "integer"}}, "required": ["node_type", "node_id"]}),
        },
        ToolDef {
            name: "pm_scaffold".into(),
            description: "Phase detail with experiment roll-up, TaskCreate-ready pending experiments, active constraints, and active principles.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "phase_id": {"type": "integer"}}, "required": ["project", "phase_id"]}),
        },
        ToolDef {
            name: "pm_session_init".into(),
            description: "Returns TaskCreate-ready actionable tasks from DAG for all active projects. Detects stale hypotheses and orphaned findings. Call at session start.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "pm_session_context".into(),
            description: "Get focused context for the current session u{2014} extracts the active phase's knowledge subgraph with recent findings, decisions, hypotheses, and blocking issues.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["project"],
                "properties": {
                    "project": {"type": "string", "description": "Project name or alias"}
                }
            }),
        },
        ToolDef {
            name: "pm_experiment_create".into(),
            description: "BEFORE calling: use pm_search to check if a similar experiment already exists or has results. Create experiment with REQUIRED causal upstream — every experiment must link to what motivated it (finding, decision, or prior experiment). First experiment in a phase is exempt.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {
                "phase_id": {"type": "integer", "description": "Phase this experiment belongs to"},
                "name": {"type": "string", "description": "What investigation this experiment represents (min 10 chars)"},
                "informed_by_finding": {"type": "integer", "description": "Finding that motivated this experiment"},
                "informed_by_decision": {"type": "integer", "description": "Decision that directed this experiment"},
                "informed_by_experiment": {"type": "integer", "description": "Prior experiment this continues/branches from"}
            }, "required": ["phase_id", "name"]}),
        },
        ToolDef {
            name: "pm_exp_complete".into(),
            description: "Complete an experiment: set status + result + optionally create finding. Returns confirmation.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "status": {"type": "string", "description": "pass, fail, or inconclusive"}, "result": {"type": "string", "description": "Result summary"}, "finding": {"type": "string", "description": "Optional finding text to create"}}, "required": ["experiment_id", "status", "result"]}),
        },
        ToolDef {
            name: "pm_log_finding".into(),
            description: "Log an empirical finding from an experiment. After creation, auto-checks for related/contradicting findings. IMPORTANT: findings should be detailed lab reports (200+ chars) with methodology, data, and conclusions — not brief summaries.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}, "text": {"type": "string"}}, "required": ["experiment_id", "text"]}),
        },
        ToolDef {
            name: "pm_research_step".into(),
            description: "Log a finding with auto-routing. Finds the best active experiment in the project and creates the finding there. No experiment_id needed.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name"}, "text": {"type": "string", "description": "Finding text"}}, "required": ["project", "text"]}),
        },
        ToolDef {
            name: "pm_decision".into(),
            description: "Record a decision with rationale. 'why' is REQUIRED. Returns decision ID + recent findings for informed-by edges.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"what": {"type": "string", "description": "The decision made (min 50 chars)"}, "why": {"type": "string", "description": "Rationale, alternatives considered, evidence (REQUIRED, min 50 chars)"}, "experiment_id": {"type": "integer", "description": "Experiment that led to this decision (causal upstream)"}, "finding_ids": {"type": "string", "description": "Comma-separated finding IDs that informed this decision (causal upstream)"}, "project": {"type": "string", "description": "Project name to associate this decision with"}}, "required": ["what", "why"]}),
        },
        ToolDef {
            name: "pm_add_edge".into(),
            description: "Add a KG edge between two nodes.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"source_type": {"type": "string"}, "source_id": {"type": "integer"}, "target_type": {"type": "string"}, "target_id": {"type": "integer"}, "relation": {"type": "string", "description": "supports, contradicts, depends, informed, supersedes, related, produced, cited, contains, derived_from, tested_by, violated_by"}}, "required": ["source_type", "source_id", "target_type", "target_id", "relation"]}),
        },
        ToolDef {
            name: "pm_hyp_add".into(),
            description: "Create a hypothesis with optional causal grounding. Hypotheses should be informed by findings and testable by experiments.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "text": {"type": "string", "description": "Hypothesis text (min 20 chars)"}, "phase_id": {"type": "integer", "description": "Phase this hypothesis belongs to"}, "finding_id": {"type": "integer", "description": "Finding that informs this hypothesis (creates Supports edge)"}, "prediction": {"type": "string", "description": "Measurable predicted outcome"}, "criteria": {"type": "string", "description": "How to evaluate: what would confirm/refute this?"}}, "required": ["project", "text"]}),
        },
        ToolDef {
            name: "pm_hyp_update".into(),
            description: "Update hypothesis status with lifecycle enforcement. proposed->testing requires supporting evidence edge. testing->refuted requires finding_id. testing->confirmed suggests creating a principle.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"hypothesis_id": {"type": "integer"}, "status": {"type": "string", "description": "proposed, testing, confirmed, or refuted"}, "experiment_id": {"type": "integer", "description": "Experiment that tested this"}, "finding_id": {"type": "integer", "description": "Finding with evidence (REQUIRED for refuted)"}, "prediction": {"type": "string", "description": "Measurable predicted outcome"}, "criteria": {"type": "string", "description": "Evaluation criteria"}, "confidence": {"type": "number", "description": "Confidence level 0.0-1.0"}}, "required": ["hypothesis_id", "status"]}),
        },
        ToolDef {
            name: "pm_lit_add".into(),
            description: "Add a literature entry (paper, blog, reference). Requires authors + arxiv_id or url. Returns ID + phase edge suggestions.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "title": {"type": "string"}, "authors": {"type": "string", "description": "Author names (REQUIRED)"}, "arxiv_id": {"type": "string"}, "url": {"type": "string"}, "venue": {"type": "string", "description": "Publication venue (e.g., NeurIPS, ICML)"}, "year": {"type": "integer", "description": "Publication year"}, "code_url": {"type": "string", "description": "URL to code repository"}, "summary": {"type": "string", "description": "Brief summary of the paper"}, "relevance": {"type": "string", "description": "Relevance to project (min 100 chars)"}, "key_findings": {"type": "string", "description": "Key findings (min 200 chars)"}}, "required": ["project", "title"]}),
        },
        ToolDef {
            name: "pm_lit_status".into(),
            description: "Update literature status lifecycle: unread -> read -> cited -> tested -> dead_end/promising/integrated.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"literature_id": {"type": "integer"}, "status": {"type": "string", "description": "unread, read, cited, tested, dead_end, promising, or integrated"}}, "required": ["literature_id", "status"]}),
        },
        ToolDef {
            name: "pm_constraint_add".into(),
            description: "Add a hard constraint (hardware, budget, correctness requirement). Returns ID + phase/experiment edge suggestions.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "scope": {"type": "string", "description": "hardware, software, or process"}, "text": {"type": "string"}, "source": {"type": "string", "description": "Where this constraint comes from (REQUIRED)"}, "severity": {"type": "string", "description": "hard (default) or soft"}, "resource": {"type": "string", "description": "Resource being constrained (e.g., GPU VRAM, context window)"}, "measured_value": {"type": "string", "description": "Current measured value"}, "expires_at": {"type": "string", "description": "Expiry date (YYYY-MM-DD) -- pm_review flags expired constraints"}, "experiment_id": {"type": "integer", "description": "Experiment that tested/discovered this constraint (auto-creates TestedBy edge)"}}, "required": ["project", "scope", "text"]}),
        },
        ToolDef {
            name: "pm_research_complete".into(),
            description: "Complete a research/reflection action with a report.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"research_id": {"type": "integer"}, "status": {"type": "string", "description": "complete or abandoned"}, "report": {"type": "string", "description": "Research findings report"}, "phase_id": {"type": "integer", "description": "Phase this research belongs to (auto-creates Contains edge)"}, "finding_ids": {"type": "string", "description": "Comma-separated finding IDs that informed this research"}}, "required": ["research_id", "status"]}),
        },
        ToolDef {
            name: "pm_principle_add".into(),
            description: "Add a project-level principle or design guideline. Auto-creates DerivedFrom edges if finding_id or decision_id provided.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}, "scope": {"type": "string", "description": "universal, project, or phase"}, "text": {"type": "string"}, "rationale": {"type": "string", "description": "Why this principle matters"}, "enforcement_level": {"type": "string", "description": "advisory (default), recommended, or mandatory"}, "finding_id": {"type": "integer", "description": "Auto-create DerivedFrom edge to this finding"}, "decision_id": {"type": "integer", "description": "Auto-create DerivedFrom edge to this decision"}}, "required": ["project", "scope", "text"]}),
        },
        ToolDef {
            name: "pm_stats".into(),
            description: "KG node and edge counts for a project.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string"}}, "required": ["project"]}),
        },
        ToolDef {
            name: "pm_search".into(),
            description: "Search across all KG node types by text content. Returns ranked results with graph connectivity and evidence scoring. Use to find nodes without knowing IDs.".into(),
            input_schema: serde_json::json!({"type": "object", "required": ["query"], "properties": {"query": {"type": "string", "description": "Text to search for across all KG node types"}}}),
        },
        ToolDef {
            name: "pm_query".into(),
            description: "Natural language KG query. Searches, ranks, shows top 3 results with graph neighbors.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}),
        },
        ToolDef {
            name: "pm_orphan_repair".into(),
            description: "Deep structural KG analysis. Finds orphaned nodes, decisions without causal upstream, cross-project bleed, missing phase assignments. Returns specific repair actions.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name or alias"}}, "required": ["project"]}),
        },
        ToolDef {
            name: "pm_context".into(),
            description: "Topic-centric KG context brief. Searches topic across all node types, groups by type, expands 1-hop neighbors, adds cross-references. Returns organized knowledge summary for LLM context injection.".into(),
            input_schema: serde_json::json!({"type": "object", "required": ["topic"], "properties": {"topic": {"type": "string", "description": "Topic to retrieve context for"}, "limit": {"type": "integer", "description": "Max results per type (default 5)"}}}),
        },
        ToolDef {
            name: "pm_project_create".into(),
            description: "Create a new project or subproject. If parent is provided, creates as a subproject under the named parent.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"name": {"type": "string", "description": "Project name (required)"}, "alias": {"type": "string", "description": "Short alias for the project"}, "parent": {"type": "string", "description": "Parent project name or alias to create as subproject under"}}, "required": ["name"]}),
        },
        ToolDef {
            name: "pm_project_list".into(),
            description: "List all projects in a tree hierarchy showing parent/child relationships and node counts per project.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "pm_project_activate".into(),
            description: "Mark a project or subproject as active. Active projects appear in dashboard by default.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": {"type": "string", "description": "Project name or alias to activate"}
                }
            }),
        },
        ToolDef {
            name: "pm_project_deactivate".into(),
            description: "Mark a project or subproject as inactive. Inactive projects hidden from dashboard unless explicitly requested. Use for future planning projects.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": {"type": "string", "description": "Project name or alias to deactivate"}
                }
            }),
        },
        ToolDef {
            name: "pm_phase_update".into(),
            description: "Update phase details and status. Completion gating: all experiments must be resolved before completing. Auto-sets started_at/completed_at timestamps.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"phase_id": {"type": "integer"}, "description": {"type": "string"}, "goals": {"type": "string"}, "success_criteria": {"type": "string"}, "status": {"type": "string", "description": "pending, in_progress, complete, or paused"}}, "required": ["phase_id"]}),
        },

        ToolDef {
            name: "pm_session_set_experiment".into(),
            description: "Set the active experiment for the current session. Findings without explicit experiment_id will auto-route here.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"experiment_id": {"type": "integer"}}, "required": ["experiment_id"]}),
        },
        ToolDef {
            name: "pm_session_start".into(),
            description: "Start a research session. Creates a timestamped session record. Call at the beginning of a work session.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name or alias (optional, scopes session to a project)"}}}),
        },
        ToolDef {
            name: "pm_session_end".into(),
            description: "End the current research session with an optional summary. Records end timestamp.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"summary": {"type": "string", "description": "Brief summary of what was accomplished this session"}}}),
        },
        ToolDef {
            name: "pm_set_confidence".into(),
            description: "Set confidence level on any TMS-enabled node (finding, decision, hypothesis, principle, constraint). Value 0.0-1.0.".into(),
            input_schema: serde_json::json!({"type": "object", "required": ["node_type", "node_id", "confidence"], "properties": {"node_type": {"type": "string", "description": "finding, decision, hypothesis, principle, or constraint"}, "node_id": {"type": "integer"}, "confidence": {"type": "number", "description": "Confidence level 0.0-1.0"}}}),
        },
        ToolDef {
            name: "pm_set_belief".into(),
            description: "Set belief status on any TMS-enabled node. When a node is contradicted, TMS auto-suspends dependents. Use this to manually believed/suspended/retracted.".into(),
            input_schema: serde_json::json!({"type": "object", "required": ["node_type", "node_id", "status"], "properties": {"node_type": {"type": "string", "description": "finding, decision, hypothesis, principle, or constraint"}, "node_id": {"type": "integer"}, "status": {"type": "string", "description": "believed, suspended, or retracted"}}}),
        },
        ToolDef {
            name: "pm_kg_audit".into(),
            description: "Comprehensive KG structural audit. Validates causal backbone compliance, hypothesis coverage, literature utilization, edge density, temporal coherence, cross-project references. Returns health score 0-100.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"project": {"type": "string", "description": "Project name or alias"}}, "required": ["project"]}),
        },
        ToolDef {
            name: "pm_since".into(),
            description: "Show all nodes created or modified since a date or session. Delta query for catching up on changes.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"since": {"type": "string", "description": "ISO date or datetime (e.g., '2026-03-20' or '2026-03-20 14:00:00')"}, "session_id": {"type": "integer", "description": "Show changes since this session started (alternative to 'since')"}}}),
        },
    ]
}
