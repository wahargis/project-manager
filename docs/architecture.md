# Architecture

Project Manager (`pm`) is a Rust application with one portable SQLite database and three frontends over the same core: a CLI, a Model Context Protocol (MCP) stdio server, and an embedded web dashboard.

![Architecture](assets/architecture.svg)

## System structure

- **Domain core.** All state is modeled as typed nodes and polymorphic edges in a single SQLite schema with versioned migrations. Validation and truth-maintenance rules live in Rust, so the same rules apply through every frontend.
- **CLI.** `pm` commands operate on the same domain core for scripted use and terminal workflows.
- **MCP server.** `pm --mcp` exposes the same operations as JSON-RPC tools over stdio, so any MCP-capable agent can read and write project state directly.
- **Web dashboard.** `pm serve` starts an embedded dashboard that renders projects, phases, findings, decisions, and graph state in a browser.

## Data flow

1. A project is activated and phases are created with dependency edges and impact scores.
2. Experiments and findings are attached to phases; decisions record the reasoning that produced them.
3. Edges (`Supports`, `Contradicts`, `DependsOn`, and the rest) propagate confidence and dependency state.
4. The DAG engine computes the next most impactful, dependency-satisfied phase; the KG engine flags contradictions, orphans, and stale items.
5. CLI, MCP, and dashboard all read the same computed state.

## Design boundaries

- SQLite is the only store; no network database is required.
- The MCP server is stdio-only today and uses the same tools as the CLI.
- The web dashboard is embedded and read-mostly.
- Planned work is tracked separately from shipped behavior; see `docs/roadmap.md`.

For the original design notes, see `docs/reference/design-v3.md`.
