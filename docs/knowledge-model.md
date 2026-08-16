# Knowledge Model

Project Manager stores research state as a typed knowledge graph.

## Node types

| Node | Purpose |
|---|---|
| Project | A top-level research effort. |
| Phase | A unit of execution with dependencies and impact. |
| Experiment | A concrete investigation, usually attached to a phase. |
| Finding | An empirical observation with confidence and belief state. |
| Decision | A recorded choice with mandatory rationale. |
| Hypothesis | A testable prediction. |
| Principle | Project-level guidance or policy. |
| Constraint | A hard boundary. |
| Research | A longer research or reflection note. |
| LiteratureEntry | A citation or reference. |
| FeedbackEntry | A correction or confirmation. |
| Session | A cross-session continuity record. |

## Edge types

Any node can connect to any other through the polymorphic edge table using relations such as `ProducedBy`, `Informed`, `Supports`, `Contradicts`, `Supersedes`, `DependsOn`, `RelatedTo`, `CitedIn`, `Contains`, `DerivedFrom`, `TestedBy`, `ViolatedBy`, `BranchesFrom`, and `ConvergesInto`.

## Truth maintenance

- A `Supports` edge raises confidence in the target.
- A `Contradicts` edge lowers confidence and can suspend the contradicted node and its downstream dependents.
- Decisions must record a non-empty `why`; experiments and decisions link to their upstream cause.

## DAG behavior

- Phases form a directed acyclic graph.
- Topological order and impact scores drive next-phase selection.
- Consecutive failures trigger stagnation detection rather than silent stalls.

## Graph analysis

- Bidirectional traversal supports neighborhood queries.
- Contradiction detection uses heuristic scoring and structured analysis.
- Orphan and staleness detection surfaces findings that have become disconnected or outdated.
- Structural audits report graph health.
