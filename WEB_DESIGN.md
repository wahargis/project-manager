# PM v3 Web Dashboard Design

## Architecture
- `pm serve [--port 9090]` starts HTTP server
- Backend: Rust HTTP server (tiny_http or warp)
- Frontend: Single index.html with embedded CSS/JS
- Data: JSON API endpoints reading from SQLite

## API Endpoints
```
GET /api/projects                    → [{id, name, status, alias}]
GET /api/projects/:id/phases         → [{id, name, status, impact, depends_on}]  
GET /api/projects/:id/findings       → [{id, text, experiment_id}]
GET /api/projects/:id/edges          → [{source_type, source_id, target_type, target_id, relation}]
GET /api/projects/:id/experiments    → [{id, name, status, phase_id}]
GET /api/projects/:id/decisions      → [{id, what, why, experiment_id}]
GET /api/dashboard                   → dashboard text output
```

## Frontend Views

### Dashboard View (default)
- Cross-project priority list (same as CLI dashboard)
- Per-project progress bars (phases complete / total)
- Stagnation warnings

### Knowledge Graph View (per project)
- 2D force-directed graph using force-graph library (CDN)
- Nodes: findings (blue circles), experiments (green squares), decisions (orange diamonds)
- Node size: proportional to edge count
- Edges: supports (green), contradicts (red), produced_by (gray), informed (purple)
- Hover: show full text
- Click: highlight connected nodes

### DAG View (per project)  
- Horizontal tree layout (left to right)
- Phase nodes sized by impact score
- Color by status: complete=green, in_progress=blue, pending=gray, deprioritized=dimmed
- Dependencies as directed edges
- Experiments listed under each phase node

## Implementation Plan
1. Add `warp` or `tiny_http` dependency to Cargo.toml
2. Implement JSON API endpoints (reuse Store queries)
3. Write index.html with force-graph CDN import
4. Serve index.html from embedded static content (include_str!)
5. Test with VR project data (93 findings, 39 experiments)
