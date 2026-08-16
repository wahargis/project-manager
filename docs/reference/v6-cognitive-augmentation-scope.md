# v6 Cognitive Augmentation Layer - Scope & Implementation Design

**Status**: Phase #62 InProgress (impact:55, project-manager-dev)
**Author**: scoping pass 2026-04-28
**Branch**: main

## TL;DR

The v6 CogAug layer is **substantially landed** at the MCP-tool layer (Pillars 1 and 3 functionally
exist in `pm_query`, `pm_context`, `pm_search`, `pm_decision`, and Pillar 2 is partially landed in
`pm_session_init` via `build_knowledge_briefing`). The remaining gap is **distribution and
activation**: the briefings exist as functions but are not yet wired into Claude Code hooks (no
`UserPromptSubmit`, no `pm-stop-nudge.sh`, no `pm-context-inject.sh` deployed). The smallest
demonstrable v6 win is to **make the existing briefings ambient via a UserPromptSubmit hook**
(E#119) plus close two CLI parity gaps (`pm context <topic>`, `pm query <text>`, fix `search --all`
double-execution). Estimated 250-450 LOC for the highest-leverage phase-1 slice.

This is **not** a "build v6 from scratch" task. It is a "ship the last 20% of v6" task.

## 1. Current State Inventory

### 1.1 MCP Tool Surface (37 tools registered in `src/mcp/tools.rs`)

Pillar-relevant tools already wired:

| Tool | Status | Notes |
|------|--------|-------|
| `pm_query` | LANDED (`review.rs:583`) | Top-3 ranked + 1-hop neighbor expansion |
| `pm_search` | LANDED (`review.rs:486`) | FTS5 + composite scoring (text + edges + evidence + recency) |
| `pm_context` | LANDED (`review.rs:1430`, commit 86a2921) | Topic-centric, groups by node type, 1-hop neighbors, cross-refs |
| `pm_session_init` | LANDED w/ briefing (`dashboard.rs:349`) | Includes `build_knowledge_briefing` per active project |
| `pm_session_context` | LANDED (`dashboard.rs:744`) | Phase-scoped briefing |
| `pm_decision` | LANDED w/ surfacing (`nodes.rs:349`) | Auto-surfaces 5 related KG nodes via text_search of decision text (~line 462) |
| `pm_log_finding` | LANDED w/ auto-link (`nodes.rs`, commit d93492c, 3ed20e9) | Composite-scored auto-edge to existing findings |
| `pm_kg_traverse` | LANDED | Single-node traversal |

### 1.2 CLI Surface (`src/cli/mod.rs`, dispatch in `src/cli_runner.rs`)

| Command | Present? | Backed By | Gap |
|---------|----------|-----------|-----|
| `pm search <project> <query> [--all]` | YES (line 895) | `tool_search` | **`--all` is broken**: iterates active projects but `tool_search` is project-agnostic; same global results printed N times. |
| `pm finding get <id>` | YES | direct store | OK |
| `pm finding traverse <id>` | YES | KG traversal | OK |
| `pm exp/dec/lit/hyp get <id>` | YES | direct store | OK |
| `pm context <topic>` | **MISSING** | would call `tool_context` | New top-level subcommand |
| `pm query <text>` | **MISSING** | would call `tool_query` | New top-level subcommand |
| `pm session-init` (CLI mirror) | MISSING | would call `tool_session_init` | New top-level subcommand |
| `pm kg traverse --depth N --topic <t>` | partial (KG Map/Traverse exist by ID) | extend with topic filter | Optional |

### 1.3 Storage Layer

- **SQLite** via `rusqlite` with bundled feature
- **FTS5** virtual tables on findings, decisions, hypotheses, principles, constraints, literature, experiments (per `store/migrations.rs`)
- `text_search` returns `SearchResult { node_type, node_id, project_seq, text_excerpt, modified_at, confidence, belief_status }` and is the primitive for retrieval
- Edge table is polymorphic: `(source_type, source_id) -> (target_type, target_id, relation)` with 13 edge types (Informed, Supports, Contradicts, DependsOn, ProducedBy, Supersedes, RelatedTo, CitedIn, Contains, DerivedFrom, TestedBy, ViolatedBy, ConvergesInto)

### 1.4 Hooks / Activation Layer

- **0 hook scripts deployed** in `~/.claude/hooks/` or `~/.config/project-manager/`
- v2-reference hooks exist at `v2-reference/pm-hooks.sh`, `v2-reference/pm-kg-cluster.sh` — not v3
- systemd user services exist for `pm-dashboard` and `pm-web` (web UI, port 9090)
- **MCP server** runs via stdio transport when invoked from Claude Code config

### 1.5 Dashboard References to Phase #62

E#119, E#120, E#122, E#125-131 are all parented to phase #62 (per `exp project-manager-dev list`).
8 are pending; E#100, E#102-105, E#112-114, E#119-122 are marked "Pass" — these are largely *design
specs that landed*, not unbuilt work. The actual remaining build queue for v6 is:

- E#123 (ambient PM via hooks)
- E#125 (DSPy/GEPA tool description optimization)
- E#126 (local Mac binary deploy)
- E#127 (delta-aware stop-nudge)
- E#128 (access-weighted retrieval)
- E#129 (automated belief revision via Contradicts edges)
- E#130 (temporal versioning / node_versions table)
- E#131 (event-driven feedback loop closure)

## 2. Three-Pillar Concrete Design

### Pillar 1 - Topic-Based Retrieval

**Hypothesis**: Topic similarity = FTS5 text relevance + graph connectivity + recency. Embedding-based
retrieval is **not required** for v6 because (a) FTS5 already gives sub-millisecond text search at
KG scale (~1.7K findings), (b) F#1187 caveat about "semantic token A/B with llama-cli unreliable"
applies to llama-cli benchmark methodology, **not** to PM retrieval. The KG already encodes
semantic relations (Supports, Contradicts, Informed) more reliably than embeddings would.

**Gap (concrete)**:

1. CLI lacks `pm context <topic>` and `pm query <text>` — adding them is ~30 LOC each (new
   `Commands::Context` and `Commands::Query` variants in `cli/mod.rs`, dispatch in `cli_runner.rs`
   that calls existing `tool_context`/`tool_query`).
2. `pm search --all` is bugged: it loops projects but `tool_search` is project-agnostic. Either
   (a) thread `project_filter: Option<i64>` through `text_search` and `tool_search`, or (b) document
   `--all` as no-op and remove the loop. (a) is correct; ~40 LOC including SQL `WHERE` clause
   addition on the FTS5 join.
3. Cross-project search: same fix as (2) — once `text_search` accepts an optional project filter,
   the default becomes cross-project and `--project <name>` becomes the scoping flag.

**LOC estimate**: 100-150 LOC including tests.

### Pillar 2 - Automatic Context Injection

**Already landed**: `tool_session_init` calls `build_knowledge_briefing(store, proj)` for every
active project (`dashboard.rs:457-466`), producing recent findings + constraints + untested hypotheses
+ contradictions. `tool_session_context` does the same for one project.

**Gap (concrete)**: the briefings are not *ambient*. To inject without being asked, we need:

1. **UserPromptSubmit hook** (E#119): a small shell script at `~/.claude/hooks/pm-context-inject.sh`
   that runs on every user prompt, calls `project-manager session-init` (new CLI mirror), and pipes
   stderr (Claude Code reads stderr from hooks as injected context). ~30 LOC shell + 20 LOC CLI
   subcommand.
2. **Stop hook enhancement** (E#127): replace static dashboard with `pm since --session` (delta of
   what changed since last nudge). Requires `pm since` CLI subcommand backed by `created_at >
   ?last_nudge_ts` query. ~80 LOC (new SQL + new subcommand + state file at
   `~/.local/share/pm/last-nudge.ts`).
3. **Phase activation hook**: on `pm phase update --status in_progress`, write a "phase activated"
   marker so the next session-init briefing leads with that phase. ~20 LOC.

**LOC estimate**: 150-200 LOC.

### Pillar 3 - Decision Support

**Already landed**: `tool_decision` (`nodes.rs:462-475`) auto-surfaces up to 5 related KG nodes via
`text_search(decision_what)` after creation, filtering out the just-created decision. Findings'
`pm_log_finding` similarly auto-links via composite scoring (commits d93492c, 3ed20e9).

**Gap (concrete)**:

1. **Surface BEFORE accept, not after**. Currently the decision is created, then related nodes are
   shown in the response. This is a logging confirmation, not a decision-support gate. To make it
   actively informational without being intrusive: add a `dry_run: bool` parameter to `pm_decision`
   that returns the related-nodes brief WITHOUT writing. Agent can call it twice: first dry-run to
   see related work, then real call after acknowledging. ~50 LOC + tests.
2. **Surface contradictions explicitly**. Today `tool_decision` shows related nodes flat. Re-rank
   so any node connected to the decision text via `Contradicts` edge appears at the top with a
   `WARNING:` prefix. Reuses existing `find_contradictions` from `kg/mod.rs`. ~40 LOC.
3. **Apply same pattern to `pm_experiment_create`**. Currently `pm_experiment_create` description
   says "BEFORE calling: use pm_search to check if a similar experiment already exists" but does
   not enforce. Add the same surfacing block as `tool_decision` does. ~30 LOC.

**LOC estimate**: 100-150 LOC.

## 3. The 8 Pending Experiments Mapped to Pillars

| Exp | Topic | Pillar | Blocker | Priority |
|-----|-------|--------|---------|----------|
| E#123 | Ambient PM via Claude Code hooks (PreToolUse, PostToolUse, PreCompact) | 2 | None | **P0** - smallest demonstrable win |
| E#127 | Delta-aware stop-nudge (`pm since --session`) | 2 | None (needs `pm since` impl) | P1 |
| E#119 | UserPromptSubmit hook for PM context | 2 | None | P0 - bundle with E#123 |
| E#128 | Access-weighted retrieval (access_count column) | 1 | Schema migration v16 | P2 |
| E#129 | Automated belief revision on Contradicts edges | 3 | None (logic only, AGM postulates) | P2 |
| E#130 | Temporal versioning (node_versions table) | meta | Schema migration + 4 trigger migrations | P3 (large) |
| E#131 | Event-driven feedback loop closure | 2 | Needs E#127 first (delta infra) | P2 |
| E#125 | DSPy/GEPA for tool description optimization | 1 (meta) | Requires DSPy harness + conversation log corpus | P3 (research-y) |
| E#126 | Local Mac aarch64 binary deploy | infra | cross-compile chain | P2 (separable) |

**Unblocked & high-leverage**: E#119, E#123, E#127. These ship Pillar 2 ambient activation —
without them the briefings exist but are never seen unless a user explicitly runs `pm dashboard`.

## 4. LOC Budget per Pillar

| Pillar | New Code | Modified Code | Tests | Total |
|--------|----------|---------------|-------|-------|
| 1 (CLI parity + cross-project) | 80 | 40 | 60 | ~180 |
| 2 (UserPromptSubmit + delta nudge) | 150 | 30 | 60 | ~240 |
| 3 (dry-run + contradictions + exp_create surfacing) | 90 | 40 | 60 | ~190 |
| **Phase-1 slice (P0 only)** | **100** | **20** | **40** | **~160** |

## 5. Implementation Order

**Phase 1 (P0, ~160 LOC, ship-this-week)**:

1. Add `pm context <topic>` and `pm query <text>` CLI subcommands (Pillar 1, 60 LOC). Demonstrable
   immediately on the command line.
2. Add `~/.claude/hooks/pm-context-inject.sh` UserPromptSubmit hook calling `project-manager
   session-init`, plus add a thin `Commands::SessionInit` CLI mirror (Pillar 2, 100 LOC). After
   deploy, every user prompt arrives with active-phase findings + constraints + hypotheses +
   contradictions injected. **This is the single largest behavioral change of v6.**

**Phase 2 (P1, ~150 LOC)**:

3. Fix `pm search --all` (Pillar 1, ~40 LOC).
4. `pm since --session` + delta-aware stop-nudge (Pillar 2, E#127).

**Phase 3 (P2, ~200 LOC)**:

5. `pm_decision dry_run=true` + Contradicts re-ranking (Pillar 3).
6. `pm_experiment_create` surfaces related work (Pillar 3).
7. Automated belief revision on Contradicts (E#129).

**Phase 4 (P3, larger)**:

8. Schema migration v16 + access-weighted retrieval (E#128).
9. Temporal versioning (E#130) — substantial schema work.
10. DSPy/GEPA optimization harness (E#125) — research-y, separable.

## 6. Smallest Demonstrable Win

**Ship `pm context` CLI + `pm-context-inject.sh` UserPromptSubmit hook.**

- Estimated **~160 LOC** (60 CLI + 30 shell hook + 70 tests/wiring).
- Visible behavior: Claude Code agent receives "active phase: Phase #62 / Recent findings: F#X,
  F#Y, F#Z / Active constraints: C#A / Untested hypotheses: H#B" injected into every prompt without
  being asked.
- Unblocks E#119 + E#123 simultaneously. Closes the largest user-perceived v6 gap.

## 7. Risk Register

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| UserPromptSubmit hook adds latency to every prompt | Medium | `pm session-init` already runs <100ms locally; gate on `PM_INJECT=1` env var; cache 30s |
| Briefing context bloat (5 findings x 150 char x N projects = 5+ KB per prompt) | Medium | Cap briefing to active phase of single active project; add `--terse` mode |
| Composite scoring promotes recent low-quality findings | Low | Recency weight is 0.3 vs text_match 1.0; evidence_weight 0.2 dominates for important findings |
| FTS5 tokenizer misses code-symbol queries (e.g. `dflash_proj_h`) | Medium | Already known; FTS5 trigram tokenizer migration is a Phase 4 option (separate) |
| Cross-project search leaks context across unrelated projects | Low | Default `--project <p>` behavior; `--all` becomes opt-in |
| `dry_run` on pm_decision adds round-trip latency | Low | Optional flag; default behavior unchanged |
| Hook breaks on Mac (no project-manager binary) | High pre-E#126 | Gate hook on `command -v project-manager`; ship with E#126 ideally |

## 8. Compounding with home_cloud ExecutionEngine

The `execution-engine` and sub-projects (`dag-executor`, `tool-system`, `eval-harness`,
`context-manager`, `memory-system`, `knowledge-graph`) are PM-tracked sibling projects under
`home-cloud`. v6 CogAug compounds with EE in two concrete ways:

1. **EE agents call `pm session-init` at task start**. The same UserPromptSubmit injection that
   primes interactive Claude Code sessions can prime EE-dispatched agents. The `tool_session_init`
   handler is already deterministic and stateless — wiring it into the EE harness's per-task system
   prompt is a 1-liner once the CLI mirror exists. Agents start with active findings/constraints
   without being told.
2. **Cross-project context for EE-dispatched research agents**. When `hc-research` spawns an agent
   for a volta-renaissance task, that agent currently sees only the prompt. With Pillar 1's
   cross-project search and Pillar 3's contradiction-surfacing, the agent's first
   `pm_context "<task topic>"` call returns relevant findings from BOTH volta-renaissance AND
   project-manager-dev (e.g., past PM-side decisions about how to log experiment results). This is
   especially valuable for the `ee-kg` and `ee-mem` subprojects, whose entire purpose is to feed
   prior context into agent reasoning.

The ExecutionEngine compounding is **not a v6 dependency** — it's a downstream user. v6 ships
without changes to EE; EE adopts the new CLI surface in its own milestone.

## 9. Honest Caveats

- The dashboard's "8 pending experiments" tally for milestone #12 is a heuristic — phase #62
  contains those 8 plus several "Pass" design-spec experiments (E#100, E#119-122) that landed as
  code, not as deferred work. The actual ship-list is the 8 pending experiments enumerated in
  Section 3.
- F#1187 ("semantic token A/B with llama-cli unreliable") is a **volta-renaissance** finding about
  llama-cli benchmarking methodology, NOT a PM-side caveat. It does not constrain v6 retrieval
  design — FTS5 is the right primitive for KG-scale text search and is already deployed.
- `pm_session_init` already injects knowledge briefings (E#104 marked Pass). The dashboard text
  "needs to inject knowledge, not just metadata" is stale guidance from before commit 0f0ed69
  (Apr 3). The actual gap is **distribution** — the briefings exist but no Claude Code hook
  invokes them ambiently.
- Pillar 1's "topic similarity" likely does NOT need embeddings. FTS5 + KG connectivity already
  out-performs naive embedding cosine for this scale (1.7K nodes). Consider embeddings only after
  measuring search miss rate empirically.

## 10. Acceptance Criteria for Phase 1 Ship

- [ ] `pm context <topic>` returns same output as `pm_context` MCP tool
- [ ] `pm query <text>` returns same output as `pm_query` MCP tool
- [ ] `~/.claude/hooks/pm-context-inject.sh` deployed and gated on `PM_INJECT=1`
- [ ] After enabling, a fresh Claude Code session shows active-phase briefing in first agent
      response without explicit `pm dashboard` call
- [ ] cargo test passes (no regressions)
- [ ] One PM-dev finding logged describing the deploy + measured prompt latency delta
