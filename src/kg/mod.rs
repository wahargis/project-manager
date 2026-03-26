use crate::store::{Edge, EdgeType, NodeType, Store, Finding};

/// Knowledge Graph engine for traversing findings, experiments, and decisions.

pub struct KgEngine<'a, S: Store> {
    store: &'a S,
}

#[derive(Debug, Clone)]
pub struct KgNode {
    pub node_type: NodeType,
    pub id: i64,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct TraversalResult {
    pub root: KgNode,
    pub edges: Vec<(Edge, KgNode)>,
}

impl<'a, S: Store> KgEngine<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self { store }
    }

    /// Single-hop traversal from a node — returns all directly connected nodes.
    pub fn traverse(&self, node_type: NodeType, node_id: i64) -> crate::store::Result<TraversalResult> {
        let label = match &node_type {
            NodeType::Finding => self.store.get_finding(node_id).map(|f| f.text)?,
            _ => format!("{:?} #{}", node_type, node_id),
        };
        let root = KgNode { node_type: node_type.clone(), id: node_id, label };

        let outgoing = self.store.get_edges_from(node_type.clone(), node_id)?;
        let incoming = self.store.get_edges_to(node_type.clone(), node_id)?;

        let mut edges = Vec::new();
        for edge in outgoing {
            let target_label = match &edge.target_type {
                NodeType::Finding => self.store.get_finding(edge.target_id).map(|f| f.text).unwrap_or_default(),
                _ => format!("{:?} #{}", edge.target_type, edge.target_id),
            };
            let target = KgNode { node_type: edge.target_type.clone(), id: edge.target_id, label: target_label };
            edges.push((edge, target));
        }
        for edge in incoming {
            let source_label = match &edge.source_type {
                NodeType::Finding => self.store.get_finding(edge.source_id).map(|f| f.text).unwrap_or_default(),
                _ => format!("{:?} #{}", edge.source_type, edge.source_id),
            };
            let source = KgNode { node_type: edge.source_type.clone(), id: edge.source_id, label: source_label };
            edges.push((edge, source));
        }

        Ok(TraversalResult { root, edges })
    }

    /// Multi-hop traversal up to max_depth.
    pub fn traverse_deep(&self, node_type: NodeType, node_id: i64, max_depth: usize) -> crate::store::Result<Vec<TraversalResult>> {
        let mut results = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue = vec![(node_type, node_id, 0usize)];

        while let Some((nt, nid, depth)) = queue.pop() {
            if depth > max_depth || visited.contains(&(format!("{:?}", nt), nid)) {
                continue;
            }
            visited.insert((format!("{:?}", nt.clone()), nid));

            let result = self.traverse(nt, nid)?;
            for (edge, target) in &result.edges {
                if !visited.contains(&(format!("{:?}", target.node_type), target.id)) {
                    queue.push((target.node_type.clone(), target.id, depth + 1));
                }
            }
            results.push(result);
        }
        Ok(results)
    }

    /// Find all contradictions in the KG.
    pub fn find_contradictions(&self, project_findings: &[Finding]) -> crate::store::Result<Vec<(Finding, Finding)>> {
        let mut contradictions = Vec::new();
        for f in project_findings {
            let edges = self.store.get_edges_from(NodeType::Finding, f.id)?;
            for edge in edges {
                if edge.relation == EdgeType::Contradicts {
                    if let Ok(target) = self.store.get_finding(edge.target_id) {
                        contradictions.push((f.clone(), target));
                    }
                }
            }
        }
        Ok(contradictions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::SqliteStore;

    fn setup() -> SqliteStore {
        let store = SqliteStore::in_memory().unwrap();
        let proj = store.create_project("test", None).unwrap();
        let phase = store.create_phase(proj.id, "P1", 10, &[]).unwrap();
        let exp = store.create_experiment(Some(phase.id), "exp1").unwrap();
        
        let f1 = store.create_finding(Some(exp.id), "Finding A").unwrap();
        let f2 = store.create_finding(Some(exp.id), "Finding B").unwrap();
        let f3 = store.create_finding(Some(exp.id), "Finding C").unwrap();
        
        store.create_edge(NodeType::Finding, f1.id, NodeType::Finding, f2.id, EdgeType::Supports).unwrap();
        store.create_edge(NodeType::Finding, f2.id, NodeType::Finding, f3.id, EdgeType::Contradicts).unwrap();
        
        store
    }

    #[test]
    fn single_hop_traversal() {
        let store = setup();
        let kg = KgEngine::new(&store);
        let result = kg.traverse(NodeType::Finding, 1).unwrap();
        assert_eq!(result.root.label, "Finding A");
        assert_eq!(result.edges.len(), 1); // outgoing: supports F2
        assert_eq!(result.edges[0].0.relation, EdgeType::Supports);
    }

    #[test]
    fn multi_hop_traversal() {
        let store = setup();
        let kg = KgEngine::new(&store);
        let results = kg.traverse_deep(NodeType::Finding, 1, 2).unwrap();
        // Should visit F1, then F2 (depth 1), then F3 (depth 2)
        assert!(results.len() >= 2);
    }

    #[test]
    fn find_contradictions() {
        let store = setup();
        let findings = store.list_findings(None).unwrap();
        let kg = KgEngine::new(&store);
        let contradictions = kg.find_contradictions(&findings).unwrap();
        assert_eq!(contradictions.len(), 1);
        assert_eq!(contradictions[0].0.text, "Finding B");
        assert_eq!(contradictions[0].1.text, "Finding C");
    }

    #[test]
    fn traversal_includes_incoming_edges() {
        let store = setup();
        let kg = KgEngine::new(&store);
        // F2 has: incoming (F1 supports), outgoing (contradicts F3)
        let result = kg.traverse(NodeType::Finding, 2).unwrap();
        assert_eq!(result.edges.len(), 2);
    }
}
