# Project Manager (`pm`)

**A local-first research-control and truth-maintenance system for long-running AI agents, human–agent teams, and experimental R&D.**

`pm` preserves the evolving **execution state** and **epistemic state** of a project: what is being attempted, what evidence has been observed, which hypotheses remain live, why decisions were made, where findings conflict, which constraints still apply, and what work is actionable now. It represents that state as a typed causal knowledge graph coupled to an impact-ranked phase DAG, then exposes the same model through a Rust CLI, a 37-tool MCP server, and an interactive web dashboard.

This is not a task list with notes attached, and it is not a chat-history or vector-memory wrapper. `pm` is the control layer between agent execution and project knowledge. It preserves causal provenance, governs research lifecycles, revises belief state when evidence changes, diagnoses stalled or structurally broken work, and reconstructs the relevant project context when a new session or agent takes over.

> A task tracker records what should be done. A notebook records what was written. `pm` records what the project currently believes, why it believes it, how that belief was tested, and what the evidence says should happen next.

**Status:** `pm` is a working pre-1.0 system under active development. The SQLite store, CLI, MCP server, web dashboard, execution DAG, causal graph, truth-maintenance, retrieval, session, and audit surfaces described below are implemented; command and schema compatibility may still change before 1.0.

## Why it exists

Long-horizon research and engineering work usually fails through **state drift**, not a lack of task creation. Across many sessions, agents and humans lose track of:

- which observation came from which experiment;
- which evidence actually justified a decision;
- whether a new result supports, contradicts, or supersedes an earlier belief;
- which branches failed, converged, or remain unresolved;
- whether a constraint is still valid;
- what is dependency-unblocked and worth doing next;
- what another agent needs in order to resume without reconstructing the project from chat logs.

`pm` turns that scattered state into evidence-aware institutional memory and an executable research model. Negative results remain part of the search history, decisions retain their rationale, beliefs can be suspended or retracted, and the current work queue is derived from project structure rather than remembered informally.

## What `pm` provides

| Capability | What it does |
|---|---|
| **Execution control** | Models phases as a dependency DAG, ranks actionable phases by impact, exposes cross-project priorities, scaffolds pending experiments into tasks, and detects consecutive-failure stagnation. |
| **Causal research ledger** | Connects phases, experiments, findings, decisions, hypotheses, research, literature, principles, and constraints through typed relations so the path from evidence to action remains inspectable. |
| **Agent-facing provenance enforcement** | MCP workflows require causal upstream evidence for decisions and non-root experiments, require principles to derive from findings or decisions, and automatically create structural and causal edges where the relationship is known. |
| **Truth maintenance** | Tracks confidence and belief state on evidence-bearing nodes. `Supports` and `Contradicts` edges trigger confidence updates and can suspend contradicted nodes and downstream dependents for review. |
| **Hypothesis and evidence lifecycles** | Governs hypothesis testing and resolution, literature progression, experiment completion, and phase completion; records predictions, evaluation criteria, results, measured constraints, and expiry. |
| **Contradiction handling** | Preserves explicit contradiction edges and also performs high-recall candidate detection using negation, antonym, numeric-divergence, and correction signals before optional semantic adjudication. |
| **Cognitive retrieval** | Combines SQLite text search with graph connectivity, evidence weight, and recency; provides topic briefs, free-text graph queries, one-hop expansion, and phase-neighborhood context rather than returning isolated notes. |
| **Session continuity** | Tracks sessions and active experiments, reports temporal deltas, generates session-start briefings and handoffs, and reconstructs the active phase’s findings, constraints, hypotheses, decisions, and literature. |
| **Graph review and repair** | Detects orphans, causal-chain breaks, cross-project bleed, temporal incoherence, sparse evidence graphs, expired constraints, dangling branches, merge points, and underused literature; produces specific repair guidance and a graph-health score. |
| **Multi-project orchestration** | Supports active, paused, and archived projects; parent/subproject hierarchies; per-project identifiers; and a portfolio-level dashboard of the highest-impact actionable work. |
| **Three operational surfaces** | Provides a scriptable CLI, a policy-rich MCP tool server for agents, and an interactive knowledge-graph / phase-DAG dashboard over one versioned SQLite store. |

## The control model

`pm` separates several concerns that are often collapsed into a single task database or note store:

| Layer | Question answered |
|---|---|
| **Phase DAG** | What work is unblocked, and what has the highest expected impact? |
| **Causal knowledge graph** | What produced this finding, informed this decision, or motivated this experiment? |
| **Truth-maintenance state** | What does the project currently believe, with what confidence, and what has been suspended or retracted? |
| **Lifecycle rules** | Is this transition evidentially valid, or is required context missing? |
| **Context and session surfaces** | What does the current agent need to know now? |
| **Review and repair** | Where has the research record become structurally weak, inconsistent, or disconnected? |

![Project Manager architecture](docs/assets/architecture.svg)

The core is a Rust domain and storage layer backed by a single SQLite database with versioned schema migrations. The interfaces serve different operational roles:

| Surface | Role |
|---|---|
| **CLI — `pm`** | Operator and automation surface: direct project manipulation, graph inspection, retrieval, review, import, handoff, and dashboard launch. |
| **MCP — `pm --mcp`** | Agent control surface: 37 JSON-RPC tools with richer causal guidance, lifecycle enforcement, automatic linking, and context-oriented responses. |
| **Web — `pm serve`** | Read-mostly visual analysis: searchable D3 knowledge graph, phase DAG, hierarchical project tree, typed node shapes, relationship styling, detailed tooltips, and collapsible project/phase subgraphs. |

Both CLI and MCP operate on the same store. The MCP surface intentionally adds stronger agent-facing policy than the lower-level CLI in several workflows.

## How a project moves through `pm`

1. **Structure the work.** Create a project or project hierarchy, define phases, assign impact, and express phase dependencies.
2. **Orient the session.** Ask for the portfolio dashboard, next actionable phases, a session briefing, or a topic-specific context brief.
3. **Run a bounded investigation.** Create an experiment that traces to the finding, decision, or prior experiment that motivated it.
4. **Capture evidence.** Complete the experiment, log detailed findings, attach measurements, and connect supporting, contradicting, cited, or derived evidence.
5. **Update project belief and direction.** Test or resolve hypotheses, record causal decisions with rationale, derive principles, and register constraints.
6. **Review the research system itself.** Audit causal completeness, graph density, temporal consistency, branch/merge topology, literature use, and orphaned nodes.
7. **Hand off without context collapse.** End the session with a summary and let the next agent reconstruct changes, active work, and the relevant knowledge neighborhood.

The system computes and explains the next action, but it does not silently execute research or autonomously close phases. Lifecycle transitions remain explicit operator or agent actions.

## Feature detail

### Dependency-aware execution and prioritization

Phases carry status, impact, goals, success criteria, timestamps, and dependencies. The DAG engine:

- returns phases in topological order;
- excludes completed and deprioritized work;
- selects only dependency-satisfied phases;
- ranks actionable phases by impact;
- reports experiment status within each phase;
- detects stagnation after repeated failed or inconclusive experiments;
- surfaces the top action across standalone projects and parent/subproject trees.

`pm next`, `pm dashboard`, `pm scaffold`, and `pm session-init` turn this structure into an actionable work surface. When all experiments in a phase are resolved, the system reports the condition and asks for an explicit phase review rather than assuming completion.

### Causal provenance and research topology

The knowledge graph is not an optional visualization layered over CRUD records. It is the project’s explanatory backbone.

Agent-facing write paths preserve common relationships automatically:

- `Phase --Contains--> Experiment`
- `Experiment --ProducedBy--> Finding`
- `Finding / Experiment --Informed--> Decision`
- `Finding --Supports / Contradicts--> Hypothesis`
- `Principle --DerivedFrom--> Finding / Decision`
- `Experiment --TestedBy--> Hypothesis / Constraint`

The MCP experiment workflow permits a root experiment at the beginning of a phase, then requires subsequent experiments to identify an upstream finding, decision, or experiment. Decisions require both a rationale and causal upstream evidence. Principles require evidence provenance.

The graph also represents research topology rather than forcing a linear history. Multiple downstream experiments are marked as branches; decisions informed by findings from multiple experiments are marked as convergence points. Review tools detect unresolved branch fan-out and causal breaks.

### Truth maintenance, belief revision, and confidence

Findings, decisions, hypotheses, principles, and constraints carry:

- a numeric confidence value;
- a belief status such as believed, suspended, or retracted.

Adding `Supports` or `Contradicts` relationships through the TMS-aware path updates target confidence. Contradiction can suspend a target and downstream dependents instead of leaving invalidated claims silently active. Operators or agents can then inspect the affected subgraph and reinstate, revise, or retract nodes explicitly.

For experiments with at least three findings containing comparable numeric measurements, `pm` also computes a Median Absolute Deviation–based confidence report. This is a lightweight repeatability signal, not a replacement for domain-specific statistical analysis.

### Contradiction detection without pretending heuristics are truth

`pm` distinguishes two mechanisms:

1. **Explicit contradiction state.** Confirmed `Contradicts` edges are part of the graph and participate in truth maintenance.
2. **Candidate detection.** New findings are compared against existing project findings using high-recall signals such as opposite negation, antonym pairs, materially divergent numbers in shared measurement contexts, and explicit correction language.

Candidate detection flags entries for review and can construct a typed natural-language-inference prompt for a second-stage model. The Rust heuristic does not automatically declare semantic contradiction.

### Governed lifecycles and anti-drift guardrails

The agent-facing workflows encode research discipline rather than accepting arbitrary state changes:

- a proposed hypothesis needs supporting evidence before entering testing;
- refuting a testing hypothesis requires a disproving finding and creates a contradiction edge;
- literature moves through an explicit progression from unread to read, cited/tested, and terminal outcomes such as integrated or dead-end;
- a phase cannot be completed through the guarded MCP path while experiments remain pending;
- decisions require non-empty rationale at the database layer;
- constraints can record source, severity, resource, measured value, and expiry;
- review surfaces expired constraints and unresolved hypotheses;
- closure or pruning language triggers an anti-cleanup warning so failed experiments and negative branches are not discarded merely because they are old or inconvenient.

The intent is not to prevent redirection. It is to make redirection explicit while preserving the evidence that narrowed the search space.

### Retrieval that returns a knowledge neighborhood

The retrieval layer uses SQLite text search plus graph-derived signals rather than embeddings:

- **`search` / `pm_search`** ranks matches using text overlap, graph connectivity, evidence weight, and recency.
- **`query` / `pm_query`** expands the highest-ranked results into their connected evidence neighborhood.
- **`context` / `pm_context`** builds a topic brief grouped across findings, decisions, hypotheses, experiments, literature, principles, constraints, research, and phases, with one-hop cross-references.
- **`session-init` / `pm_session_init`** assembles actionable phases, pending experiments, untested hypotheses, orphan warnings, recent findings, constraints, and contradictions.
- **`pm_session_context`** extracts a bounded phase subgraph and summarizes its active evidence and suggested next actions.
- **`pm_since`** reports nodes changed since a timestamp or prior session.

The result is context selected by project structure and causal proximity, not a flat list of text fragments.

### Structural diagnostics and repair

`pm review`, `pm orphan-repair`, and `pm kg-audit` inspect the quality of the research record itself.

Checks include:

- decisions with no causal upstream;
- orphaned findings, hypotheses, principles, constraints, experiments, literature, or research;
- missing project ownership and cross-project causal bleed;
- branch points, convergence points, and dangling branches;
- untested hypotheses and unresolved experiments;
- explicit contradictions and suspended/retracted beliefs;
- expired constraints and node-age context;
- causal-chain completeness;
- hypothesis coverage;
- literature utilization;
- edge density;
- temporal coherence;
- cross-project references.

The audit produces a 0–100 health score with a metric breakdown. The repair surface returns concrete edge or ownership changes rather than merely reporting that the graph is untidy.

### Multi-project and session continuity

Projects can be standalone or arranged as parent/subproject trees. `pm dashboard` computes the highest-impact available phase across the active portfolio, while the web dashboard can collapse or expand subproject hulls and phase neighborhoods.

Sessions record start and end timestamps, optional project scope, summary, and active experiment. This lets a finding fall back to the session’s active experiment when an explicit experiment ID is omitted, and gives later sessions a temporal anchor for change reports and handoffs.

## Quick start

### Build and install

A current Rust toolchain with Rust 2024 edition support is required.

```bash
git clone https://github.com/wahargis/project-manager.git
cd project-manager
cargo install --path .
```

The CLI uses `~/.local/share/pm/pm.db` by default. Use `--db <path>` for another database. The MCP server reads `PM_DB`.

### Create a project and execution DAG

```bash
pm --db /tmp/atlas.db project activate atlas --alias at

pm --db /tmp/atlas.db phase atlas add \
  "Map retrieval failure modes" --impact 80

pm --db /tmp/atlas.db phase atlas add \
  "Prototype topic-scoped context" --impact 100 --depends 1

pm --db /tmp/atlas.db phase atlas add \
  "Evaluate session handoff quality" --impact 70 --depends 2

pm --db /tmp/atlas.db next atlas
```

### Record an experiment, finding, hypothesis, and decision

```bash
pm --db /tmp/atlas.db exp atlas add \
  "Survey retrieval evaluation practice" \
  --phase 1 \
  --status pass \
  --result "Recall-oriented context reconstruction is rarely evaluated."

pm --db /tmp/atlas.db finding atlas add \
  "A survey of retrieval evaluation practice found that public benchmarks emphasize isolated-answer precision while rarely measuring whether a long-running agent reconstructs the complete causal context needed for its next action." \
  --experiment 1

pm --db /tmp/atlas.db hyp atlas add \
  "Topic-scoped graph briefings improve next-action selection" \
  --phase 2 \
  --finding 1

pm --db /tmp/atlas.db dec atlas add \
  "Use topic-scoped graph briefings as the first retrieval surface" \
  --why "The survey finding identifies context reconstruction—not isolated text similarity—as the immediate failure mode, and the graph briefing is directly testable in the next phase." \
  --experiment 1
```

### Orient, inspect, and review

```bash
pm --db /tmp/atlas.db dashboard
pm --db /tmp/atlas.db session-init
pm --db /tmp/atlas.db search atlas retrieval
pm --db /tmp/atlas.db context "retrieval evaluation" --limit 5
pm --db /tmp/atlas.db query "Why did the project choose topic-scoped briefings?"
pm --db /tmp/atlas.db review atlas
pm --db /tmp/atlas.db kg-audit atlas
pm --db /tmp/atlas.db handoff atlas
```

### Launch the dashboard

```bash
pm --db /tmp/atlas.db serve --port 9090
```

The MCP server also attempts to start the dashboard on port `9090`. Set `PM_WEB_PORT` to choose another port.

## MCP integration

Start the stdio server directly:

```bash
PM_DB=/tmp/atlas.db PM_WEB_PORT=9090 pm --mcp
```

A generic MCP client configuration looks like:

```json
{
  "mcpServers": {
    "project-manager": {
      "command": "/absolute/path/to/pm",
      "args": ["--mcp"],
      "env": {
        "PM_DB": "/absolute/path/to/pm.db",
        "PM_WEB_PORT": "9090"
      }
    }
  }
}
```

The 37 tools are organized around complete project workflows rather than raw table access:

| Tool family | Representative tools |
|---|---|
| **Orientation and execution** | `pm_dashboard`, `pm_next`, `pm_scaffold`, `pm_session_init`, `pm_session_context` |
| **Experiments and evidence** | `pm_experiment_create`, `pm_exp_complete`, `pm_log_finding`, `pm_decision` |
| **Hypotheses and research state** | `pm_hyp_add`, `pm_hyp_update`, `pm_lit_add`, `pm_lit_status`, `pm_constraint_add`, `pm_principle_add`, `pm_research_complete` |
| **Retrieval** | `pm_search`, `pm_query`, `pm_context`, `pm_since` |
| **Knowledge graph** | `pm_add_edge`, `pm_kg_traverse`, `pm_set_confidence`, `pm_set_belief` |
| **Integrity and repair** | `pm_review`, `pm_orphan_repair`, `pm_kg_audit`, `pm_stats` |
| **Portfolio and sessions** | project/phase lifecycle tools, `pm_session_start`, `pm_session_set_experiment`, `pm_session_end` |

Run `pm --help` for the CLI surface or issue MCP `tools/list` for the complete tool schemas.

## Knowledge model reference

### Node types

| Node | Purpose and state |
|---|---|
| `Project` | Active, paused, or archived research effort; optional alias and parent project. |
| `Phase` | Dependency-aware execution unit with impact, status, description, goals, success criteria, and lifecycle timestamps. |
| `Experiment` | Bounded investigation with pending/pass/fail/inconclusive status, hypothesis, result, and notes. |
| `Finding` | Empirical observation with experiment provenance, confidence, and belief state. |
| `Decision` | Choice with mandatory rationale, project scope, causal upstream, confidence, and belief state. |
| `Hypothesis` | Proposed/testing/confirmed/refuted claim with prediction, criteria, evidence references, confidence, and belief state. |
| `Research` | Longer-form research work or report attached to a phase. |
| `LiteratureEntry` | Citation record with title, authors, venue, year, arXiv/URL/code links, summary, relevance, key findings, and lifecycle status. |
| `Principle` | Evidence-derived guidance with universal/project/phase scope, rationale, enforcement level, status, confidence, and belief state. |
| `Constraint` | Hardware/software/process boundary with source, severity, resource, measurement, expiry, confidence, and belief state. |
| `FeedbackEntry` | Explicit correction or confirmation. |
| `Session` | Temporal continuity record with project scope, summary, and active experiment. |

### Edge types

`ProducedBy`, `Informed`, `Supports`, `Contradicts`, `Supersedes`, `DependsOn`, `RelatedTo`, `CitedIn`, `Contains`, `DerivedFrom`, `TestedBy`, `ViolatedBy`, `BranchesFrom`, and `ConvergesInto`.

Edges are polymorphic: any supported node type can participate where the relation is meaningful. A uniqueness constraint prevents duplicate relationships.

## Storage and deployment model

- One local SQLite database, managed through bundled `rusqlite`.
- Versioned, idempotent schema migrations.
- Per-project ordinal references alongside global database IDs.
- No hosted service or remote database dependency.
- MCP transport is line-delimited JSON-RPC over stdio.
- The web server is embedded with Warp; the current dashboard loads D3 from a CDN.
- v2 JSON data can be imported with `pm import <file.json> --name <project>`.

## Current boundaries

`pm` is a working research-control system, not an autonomous project executive.

- It computes priorities, detects invalid transitions, and recommends repairs; a human or agent still performs the work and explicitly commits lifecycle changes.
- Search is lexical and graph-aware, not embedding-based semantic retrieval.
- Heuristic contradiction detection produces candidates; semantic confirmation remains a separate review step.
- The SQLite architecture is optimized for local, single-operator or single-agent-runtime use rather than concurrent multi-user collaboration.
- The dashboard is primarily an analysis and navigation surface, not the full write interface.
- MCP is currently stdio-based rather than a network service.

These constraints keep the system inspectable and portable while leaving room for richer retrieval, orchestration, and collaborative deployment.

## Documentation

- [Architecture](docs/architecture.md) — system structure and interface boundaries.
- [Knowledge model](docs/knowledge-model.md) — node, edge, truth-maintenance, DAG, and graph-analysis concepts.
- [MCP server](docs/mcp.md) — agent integration.
- [Reference notes](docs/reference/README.md) — historical design and planning documents, clearly separated from current public documentation.

## Repository layout

| Path | Contents |
|---|---|
| `src/cli/` | Clap command and subcommand definitions. |
| `src/cli_runner.rs` | CLI command handlers, import, handoff, and operator-facing analysis. |
| `src/store/` | SQLite implementation, versioned migrations, sessions, search, and typed node/edge model. |
| `src/dag/` | Dependency and impact-based phase execution engine. |
| `src/kg/` | Single- and multi-hop traversal, edge filtering, and subgraph extraction. |
| `src/analysis/` | Numeric confidence scoring and contradiction-candidate analysis. |
| `src/mcp/` | MCP server, tool schemas, policy-rich node/edge workflows, session context, review, and repair. |
| `src/web.rs`, `src/web/index.html` | Embedded web API and D3 dashboard. |
| `docs/` | Public documentation, architecture assets, roadmap, and historical references. |
| `v2-reference/` | v2 reference data and import material. |

## License

Licensed under the [MIT License](LICENSE).
