# project-manager v3 — Design Document

## Overview

Research project management system for long-horizon agentic work. Manages structured R&D objects (phases, experiments, findings, decisions, literature) with DAG-based execution, knowledge graph traversal, and AI agent runtime integration.

## Architecture

### Layer 1: Persistent Knowledge Store
- **Backend**: SQLite (single-file, zero-config, portable)
- **Schema**: projects, phases, experiments, findings, edges, decisions, literature
- **Referential integrity** via foreign keys
- **Full-text search** on findings and experiment notes
- **Temporal queries**: "what was learned about X since date Y"

### Layer 2: DAG Execution Engine
- Phase dependencies as directed acyclic graph
- **Impact propagation**: effective_impact = own + weighted downstream blocked phases
- **Auto-transitions**: phase completes when all experiments are non-pending
- **Stagnation detection**: triggers after N consecutive failed experiments
- **Mandatory review gates**: blocks experiments after K experiments or T hours without review
- **Topological sort** for execution ordering

### Layer 3: Knowledge Graph
- **Node types**: finding, experiment, decision, literature
- **Edge types**: produced_by, informed, supports, contradicts, supersedes, depends_on, related_to, cited_in
- **Bidirectional traversal** with depth control
- **Cluster detection**: connected components, grouping by experiment/phase
- **Contradiction resolution**: flagged for explicit decision
- **Staleness detection**: unreferenced findings older than N experiments

### Layer 4: Agent Runtime Integration (MCP Server)
- Runs as stdio MCP server alongside claude-code
- **Idle detection**: monitors tool call frequency, injects dashboard when idle
- **Auto-scaffold**: when phase completes, creates task tracker items for next phase
- **Review gates**: injects review output and blocks when triggered
- **Experiment lifecycle**: auto-updates PM experiment status from task tracker completion

### Layer 5: Cross-Session Continuity
- **Structured handoff**: on session end, generate handoff doc (current state + next actions)
- **Session start injection**: handoff + dashboard + stale findings
- **Research arc tracking**: high-level narrative of research direction
- **Time-boxed evaluation**: experiments have budgets, auto-terminate if exceeded

### Layer 6: Multi-Project Orchestration
- Projects have priority classes: active, background, paused, archived
- **Cross-project dashboard**: impact-weighted priority across all active projects
- **Opportunity cost tracking**: what is deferred when working on project A
- **Portfolio stagnation**: meta-review when all projects stagnate

## Technology

- **Language**: Rust (algebraic types for KG edges/DAG states, Serde for JSON/MCP, Rustler NIFs for future Elixir integration)
- **Storage**: SQLite via rusqlite
- **CLI**: clap
- **MCP Server**: tokio + stdio JSON-RPC (MCP protocol)
- **Testing**: cargo test + assert_cmd for CLI integration tests
- **Build**: cargo build --release, cross for cross-compilation

## CLI Interface

```
pm project list|activate|pause|archive
pm phase add|list|update|get <project> [flags]
pm exp add|list|update|get <project> [flags]
pm finding add|list|traverse <project> [flags]
pm decision add|list <project> [flags]
pm kg map|traverse|cluster <project> [flags]
pm scaffold <project> [--phase N] [--format json]
pm review <project>
pm dashboard
pm next [project]
pm import <project.json>  # migrate from v2
pm export <project> [--format json|md]
```

## Data Migration

### From v2 (bash + JSON files):
- `pm import volta-renaissance.json` reads the v2 project.json
- Maps phases, experiments, findings, decisions, literature to SQLite tables
- Preserves all IDs, timestamps, relationships
- Imports markdown docs as finding attachments

### Items to migrate:
- VR: 12 phases, 31 experiments, 6 decisions, 31 findings, 1709 lines of markdown docs
- PM-dev: 10 phases, 6 experiments, 15 findings
- Active projects config
- Hooks (pm-hooks, pm-kg-cluster)
- Claude-code plugin (4 slash commands)

## Testing Strategy

TDD — tests written before implementation for each module:
1. **Store tests**: CRUD for all entity types, FK enforcement, FTS queries
2. **DAG tests**: topological sort, impact propagation, auto-transition, stagnation detection
3. **KG tests**: edge CRUD, traversal depth, cluster detection, contradiction flagging
4. **CLI tests**: command parsing, output format, error handling
5. **MCP tests**: protocol compliance, idle detection thresholds, auto-scaffold triggers
6. **Migration tests**: v2 JSON import preserves all data, markdown import, ID stability
7. **Integration tests**: full workflow (create project → add phases → run experiments → review → scaffold)
