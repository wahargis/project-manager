//! Multi-hop graph traversal engine for the PM knowledge graph.
//!
//! Provides BFS-based traversal with configurable depth, edge type filtering,
//! proximity scoring, subgraph extraction, and shortest path finding.
//! This is the FOUNDATION for all v5 intelligence features (Decision #21:
//! graph-first, text-second).

use std::collections::{HashMap, HashSet, VecDeque};
use crate::store::{Edge, EdgeType, NodeType, Store};

/// A node in the traversal result with distance metadata.
#[derive(Debug, Clone)]
pub struct ProximityNode {
    pub node_type: NodeType,
    pub node_id: i64,
    pub distance: usize,        // hop count from origin
    pub path_edge_types: Vec<EdgeType>, // edge types along the path
}

/// A subgraph extracted by traversal.
#[derive(Debug)]
pub struct Subgraph {
    pub origin: (NodeType, i64),
    pub nodes: Vec<ProximityNode>,
    pub edges: Vec<Edge>,
}

/// Edge type filter for traversal.
#[derive(Debug, Clone)]
pub enum EdgeFilter {
    /// Traverse all edge types
    All,
    /// Only traverse these specific edge types
    Only(Vec<EdgeType>),
    /// Traverse all EXCEPT these edge types
    Except(Vec<EdgeType>),
}

impl EdgeFilter {
    fn allows(&self, edge_type: &EdgeType) -> bool {
        match self {
            EdgeFilter::All => true,
            EdgeFilter::Only(allowed) => allowed.contains(edge_type),
            EdgeFilter::Except(excluded) => !excluded.contains(edge_type),
        }
    }
}

/// Configuration for a traversal query.
#[derive(Debug, Clone)]
pub struct TraversalConfig {
    pub max_depth: usize,
    pub edge_filter: EdgeFilter,
    pub bidirectional: bool,     // follow edges in both directions
    pub include_origin: bool,    // include the starting node in results
}

impl Default for TraversalConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            edge_filter: EdgeFilter::All,
            bidirectional: true,
            include_origin: true,
        }
    }
}

/// Canonical key for a node (type + id).
fn node_key(nt: &NodeType, id: i64) -> (String, i64) {
    (format!("{:?}", nt), id)
}

/// Multi-hop BFS traversal from a starting node.
///
/// Returns all nodes reachable within `config.max_depth` hops,
/// filtered by `config.edge_filter`, with distance metadata.
pub fn traverse_bfs<S: Store>(
    store: &S,
    start_type: NodeType,
    start_id: i64,
    config: &TraversalConfig,
) -> Subgraph {
    let mut visited: HashSet<(String, i64)> = HashSet::new();
    let mut queue: VecDeque<(NodeType, i64, usize, Vec<EdgeType>)> = VecDeque::new();
    let mut result_nodes: Vec<ProximityNode> = Vec::new();
    let mut result_edges: Vec<Edge> = Vec::new();
    let mut seen_edges: HashSet<i64> = HashSet::new();

    let origin_key = node_key(&start_type, start_id);
    visited.insert(origin_key.clone());

    if config.include_origin {
        result_nodes.push(ProximityNode {
            node_type: start_type.clone(),
            node_id: start_id,
            distance: 0,
            path_edge_types: vec![],
        });
    }

    queue.push_back((start_type.clone(), start_id, 0, vec![]));

    while let Some((nt, nid, depth, path)) = queue.pop_front() {
        if depth >= config.max_depth {
            continue;
        }

        // Get outgoing edges
        if let Ok(edges) = store.get_edges_from(nt.clone(), nid) {
            for edge in &edges {
                if !config.edge_filter.allows(&edge.relation) {
                    continue;
                }

                let target_key = node_key(&edge.target_type, edge.target_id);
                if !visited.contains(&target_key) {
                    visited.insert(target_key);
                    let mut new_path = path.clone();
                    new_path.push(edge.relation.clone());

                    result_nodes.push(ProximityNode {
                        node_type: edge.target_type.clone(),
                        node_id: edge.target_id,
                        distance: depth + 1,
                        path_edge_types: new_path.clone(),
                    });

                    queue.push_back((edge.target_type.clone(), edge.target_id, depth + 1, new_path));
                }

                if !seen_edges.contains(&edge.id) {
                    seen_edges.insert(edge.id);
                    result_edges.push(edge.clone());
                }
            }
        }

        // Get incoming edges (if bidirectional)
        if config.bidirectional {
            if let Ok(edges) = store.get_edges_to(nt.clone(), nid) {
                for edge in &edges {
                    if !config.edge_filter.allows(&edge.relation) {
                        continue;
                    }

                    let source_key = node_key(&edge.source_type, edge.source_id);
                    if !visited.contains(&source_key) {
                        visited.insert(source_key);
                        let mut new_path = path.clone();
                        new_path.push(edge.relation.clone());

                        result_nodes.push(ProximityNode {
                            node_type: edge.source_type.clone(),
                            node_id: edge.source_id,
                            distance: depth + 1,
                            path_edge_types: new_path.clone(),
                        });

                        queue.push_back((edge.source_type.clone(), edge.source_id, depth + 1, new_path));
                    }

                    if !seen_edges.contains(&edge.id) {
                        seen_edges.insert(edge.id);
                        result_edges.push(edge.clone());
                    }
                }
            }
        }
    }

    Subgraph {
        origin: (start_type, start_id),
        nodes: result_nodes,
        edges: result_edges,
    }
}

/// Get all nodes within N hops, grouped by distance.
pub fn neighborhood<S: Store>(
    store: &S,
    start_type: NodeType,
    start_id: i64,
    max_depth: usize,
    edge_filter: EdgeFilter,
) -> HashMap<usize, Vec<ProximityNode>> {
    let config = TraversalConfig {
        max_depth,
        edge_filter,
        bidirectional: true,
        include_origin: false,
    };
    let subgraph = traverse_bfs(store, start_type, start_id, &config);

    let mut by_distance: HashMap<usize, Vec<ProximityNode>> = HashMap::new();
    for node in subgraph.nodes {
        by_distance.entry(node.distance).or_default().push(node);
    }
    by_distance
}

/// Find nodes of a specific type within N hops.
pub fn find_nearby<S: Store>(
    store: &S,
    start_type: NodeType,
    start_id: i64,
    target_type: NodeType,
    max_depth: usize,
) -> Vec<ProximityNode> {
    let config = TraversalConfig {
        max_depth,
        edge_filter: EdgeFilter::All,
        bidirectional: true,
        include_origin: false,
    };
    let subgraph = traverse_bfs(store, start_type, start_id, &config);

    subgraph.nodes.into_iter()
        .filter(|n| n.node_type == target_type)
        .collect()
}

/// Extract the phase subgraph — all nodes reachable from a phase via
/// Contains, ProducedBy, Informed, Supports, Contradicts edges.
/// This is the primary retrieval for session context (F3).
pub fn phase_subgraph<S: Store>(
    store: &S,
    phase_id: i64,
    max_depth: usize,
) -> Subgraph {
    let config = TraversalConfig {
        max_depth,
        edge_filter: EdgeFilter::Only(vec![
            EdgeType::Contains,
            EdgeType::ProducedBy,
            EdgeType::Informed,
            EdgeType::Supports,
            EdgeType::Contradicts,
            EdgeType::DependsOn,
            EdgeType::DerivedFrom,
        ]),
        bidirectional: true,
        include_origin: true,
    };
    traverse_bfs(store, NodeType::Phase, phase_id, &config)
}

/// Find all findings in the same neighborhood as a given finding.
/// Used by contradiction detection (F2) for candidate retrieval.
pub fn finding_neighborhood<S: Store>(
    store: &S,
    finding_id: i64,
    max_depth: usize,
) -> Vec<ProximityNode> {
    find_nearby(store, NodeType::Finding, finding_id, NodeType::Finding, max_depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::SqliteStore;

    fn setup_store() -> SqliteStore {
        SqliteStore::new(":memory:").unwrap()
    }

    #[test]
    fn test_bfs_single_hop() {
        let store = setup_store();
        let proj = store.create_project("test", None, None).unwrap();
        let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();
        let _finding = store.create_finding(None, "Test finding").unwrap();

        // Create edge: Phase -> Finding
        store.create_edge(NodeType::Phase, phase.id, NodeType::Finding, 1, EdgeType::Contains).unwrap();

        let config = TraversalConfig { max_depth: 1, ..Default::default() };
        let result = traverse_bfs(&store, NodeType::Phase, phase.id, &config);

        assert!(result.nodes.len() >= 2, "Should have origin + 1 connected node, got {}", result.nodes.len());
        assert!(!result.edges.is_empty(), "Should have at least 1 edge");
    }

    #[test]
    fn test_bfs_multi_hop() {
        let store = setup_store();
        let proj = store.create_project("test", None, None).unwrap();
        let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();
        let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();
        let finding = store.create_finding(Some(exp.id), "Finding from exp").unwrap();

        store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Contains).unwrap();
        store.create_edge(NodeType::Experiment, exp.id, NodeType::Finding, finding.id, EdgeType::ProducedBy).unwrap();

        // 1 hop from Phase should reach Experiment but not Finding
        let config1 = TraversalConfig { max_depth: 1, ..Default::default() };
        let result1 = traverse_bfs(&store, NodeType::Phase, phase.id, &config1);
        let finding_nodes: Vec<_> = result1.nodes.iter().filter(|n| n.node_type == NodeType::Finding).collect();
        assert!(finding_nodes.is_empty(), "Finding should NOT be reachable at depth 1");

        // 2 hops should reach Finding
        let config2 = TraversalConfig { max_depth: 2, ..Default::default() };
        let result2 = traverse_bfs(&store, NodeType::Phase, phase.id, &config2);
        let finding_nodes2: Vec<_> = result2.nodes.iter().filter(|n| n.node_type == NodeType::Finding).collect();
        assert!(!finding_nodes2.is_empty(), "Finding SHOULD be reachable at depth 2");
        assert_eq!(finding_nodes2[0].distance, 2);
    }

    #[test]
    fn test_edge_type_filter() {
        let store = setup_store();
        let proj = store.create_project("test", None, None).unwrap();
        let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();
        let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();

        store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Contains).unwrap();
        store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Informed).ok();

        // Filter to only Contains edges
        let config = TraversalConfig {
            max_depth: 1,
            edge_filter: EdgeFilter::Only(vec![EdgeType::Contains]),
            ..Default::default()
        };
        let result = traverse_bfs(&store, NodeType::Phase, phase.id, &config);
        // Should still find the experiment via Contains
        assert!(result.nodes.len() >= 2);
    }

    #[test]
    fn test_neighborhood_grouping() {
        let store = setup_store();
        let proj = store.create_project("test", None, None).unwrap();
        let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();
        let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();
        let finding = store.create_finding(Some(exp.id), "Test finding").unwrap();

        store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Contains).unwrap();
        store.create_edge(NodeType::Experiment, exp.id, NodeType::Finding, finding.id, EdgeType::ProducedBy).unwrap();

        let hood = neighborhood(&store, NodeType::Phase, phase.id, 3, EdgeFilter::All);

        assert!(hood.contains_key(&1), "Should have distance-1 nodes");
        assert!(hood.contains_key(&2), "Should have distance-2 nodes");
    }

    #[test]
    fn test_find_nearby_typed() {
        let store = setup_store();
        let proj = store.create_project("test", None, None).unwrap();
        let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();
        let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();
        let f1 = store.create_finding(Some(exp.id), "Finding 1").unwrap();
        let f2 = store.create_finding(Some(exp.id), "Finding 2").unwrap();

        store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Contains).unwrap();
        store.create_edge(NodeType::Experiment, exp.id, NodeType::Finding, f1.id, EdgeType::ProducedBy).unwrap();
        store.create_edge(NodeType::Experiment, exp.id, NodeType::Finding, f2.id, EdgeType::ProducedBy).unwrap();

        // From Phase, find all Findings within 3 hops
        let findings = find_nearby(&store, NodeType::Phase, phase.id, NodeType::Finding, 3);
        assert_eq!(findings.len(), 2, "Should find 2 findings");
    }

    #[test]
    fn test_cycle_handling() {
        let store = setup_store();
        let proj = store.create_project("test", None, None).unwrap();
        let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();
        let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();

        // Create a cycle: Phase -> Exp -> Phase (via different edge types)
        store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Contains).unwrap();
        store.create_edge(NodeType::Experiment, exp.id, NodeType::Phase, phase.id, EdgeType::Informed).unwrap();

        // Should not infinite loop
        let config = TraversalConfig { max_depth: 10, ..Default::default() };
        let result = traverse_bfs(&store, NodeType::Phase, phase.id, &config);
        assert!(result.nodes.len() <= 3, "Should handle cycles without duplication");
    }

    #[test]
    fn test_phase_subgraph() {
        let store = setup_store();
        let proj = store.create_project("test", None, None).unwrap();
        let phase = store.create_phase(proj.id, "Phase 1", 10, &[]).unwrap();
        let exp = store.create_experiment(Some(phase.id), "Exp 1").unwrap();
        let finding = store.create_finding(Some(exp.id), "Test finding").unwrap();

        store.create_edge(NodeType::Phase, phase.id, NodeType::Experiment, exp.id, EdgeType::Contains).unwrap();
        store.create_edge(NodeType::Experiment, exp.id, NodeType::Finding, finding.id, EdgeType::ProducedBy).unwrap();

        let subgraph = phase_subgraph(&store, phase.id, 3);
        assert!(subgraph.nodes.len() >= 3, "Phase subgraph should include phase, experiment, finding");
    }
}
