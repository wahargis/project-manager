# project-manager v3 — Task Decomposition

## Phase 0: Repository Setup
- [ ] P0.1: Init Rust project (`cargo init --name pm`)
- [ ] P0.2: Directory structure (src/store/, src/dag/, src/kg/, src/mcp/, src/cli/)
- [ ] P0.3: CI: Makefile with build, test, lint targets
- [ ] P0.4: SQLite dependency (rusqlite with bundled feature)

## Phase 1: Persistent Knowledge Store (TDD)
- [ ] P1.1: Write store tests — CRUD for projects table
- [ ] P1.2: Write store tests — CRUD for phases (with FK to project)
- [ ] P1.3: Write store tests — CRUD for experiments (with FK to phase, status enum)
- [ ] P1.4: Write store tests — CRUD for findings (with FK to experiment)
- [ ] P1.5: Write store tests — CRUD for edges (polymorphic FK: source_type+id → target_type+id)
- [ ] P1.6: Write store tests — CRUD for decisions (with FK to experiment)
- [ ] P1.7: Write store tests — CRUD for literature
- [ ] P1.8: Write store tests — full-text search on findings
- [ ] P1.9: Write store tests — temporal queries (findings since date)
- [ ] P1.10: Implement Store interface + SQLite backend to pass all P1 tests
- [ ] P1.11: Write migration tests — v2 JSON import preserves all data
- [ ] P1.12: Implement v2 JSON importer

## Phase 2: DAG Execution Engine (TDD)
- [ ] P2.1: Write DAG tests — topological sort of phases
- [ ] P2.2: Write DAG tests — impact propagation through graph
- [ ] P2.3: Write DAG tests — next phase selection (deps satisfied, impact-weighted)
- [ ] P2.4: Write DAG tests — auto-transition (phase completes when all experiments non-pending)
- [ ] P2.5: Write DAG tests — stagnation detection (N consecutive fails)
- [ ] P2.6: Write DAG tests — review gate (blocks after K experiments without review)
- [ ] P2.7: Implement DAG engine to pass all P2 tests

## Phase 3: Knowledge Graph (TDD)
- [ ] P3.1: Write KG tests — edge CRUD with typed relationships
- [ ] P3.2: Write KG tests — single-hop traversal from any node
- [ ] P3.3: Write KG tests — multi-hop traversal with depth limit
- [ ] P3.4: Write KG tests — cluster detection (connected components)
- [ ] P3.5: Write KG tests — contradiction flagging
- [ ] P3.6: Write KG tests — staleness detection
- [ ] P3.7: Implement KG engine to pass all P3 tests

## Phase 4: CLI (TDD)
- [ ] P4.1: Write CLI tests — project commands (list, activate, pause, archive)
- [ ] P4.2: Write CLI tests — phase commands (add, list, update, get)
- [ ] P4.3: Write CLI tests — experiment commands (add, list, update, get)
- [ ] P4.4: Write CLI tests — finding commands (add, list, traverse)
- [ ] P4.5: Write CLI tests — decision and literature commands
- [ ] P4.6: Write CLI tests — kg commands (map, traverse, cluster)
- [ ] P4.7: Write CLI tests — scaffold command (JSON output)
- [ ] P4.8: Write CLI tests — review command
- [ ] P4.9: Write CLI tests — dashboard command
- [ ] P4.10: Write CLI tests — next command
- [ ] P4.11: Write CLI tests — import/export commands
- [ ] P4.12: Implement CLI using clap to pass all P4 tests
- [ ] P4.13: Short alias support (pm vr next, pm pm-dev dashboard)

## Phase 5: MCP Server (TDD)
- [ ] P5.1: Write MCP tests — stdio JSON-RPC protocol compliance
- [ ] P5.2: Write MCP tests — tool registration (dashboard, next, review, scaffold)
- [ ] P5.3: Write MCP tests — idle detection threshold configuration
- [ ] P5.4: Write MCP tests — auto-scaffold trigger on phase completion
- [ ] P5.5: Write MCP tests — review gate injection
- [ ] P5.6: Implement MCP server to pass all P5 tests
- [ ] P5.7: Claude-code settings.json integration (MCP server config)

## Phase 6: Cross-Session Continuity
- [ ] P6.1: Structured handoff document generation on session end
- [ ] P6.2: Session start context injection (handoff + dashboard + stale findings)
- [ ] P6.3: Research arc narrative tracking
- [ ] P6.4: Time-boxed experiment evaluation

## Phase 7: Multi-Project Orchestration
- [ ] P7.1: Project priority classes (active, background, paused, archived)
- [ ] P7.2: Cross-project impact-weighted dashboard
- [ ] P7.3: Opportunity cost tracking
- [ ] P7.4: Portfolio-level stagnation detection

## Phase 8: Data Migration
- [ ] P8.1: Export v2 VR project (12 phases, 31 exp, 6 dec, 31 findings, 1709 lines markdown)
- [ ] P8.2: Export v2 PM-dev project (10 phases, 6 exp, 15 findings)
- [ ] P8.3: Import VR markdown docs (10 files) as finding attachments
- [ ] P8.4: Verify zero data loss — all IDs, timestamps, relationships preserved
- [ ] P8.5: Migrate active-projects.json config
- [ ] P8.6: Update hooks (pm-hooks, pm-kg-cluster) to use v3 CLI
- [ ] P8.7: Update claude-code plugin commands to use v3 CLI

## Phase 9: Polish & Deploy
- [ ] P9.1: goreleaser config for cross-platform builds
- [ ] P9.2: Install script (curl | sh)
- [ ] P9.3: Man page / --help documentation
- [ ] P9.4: GitHub repo creation and initial push
- [ ] P9.5: Replace /usr/local/bin/project-manager with v3 binary
- [ ] P9.6: End-to-end smoke test with existing projects
