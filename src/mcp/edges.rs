//! MCP tool implementations for edge/KG operations.
//!
//! Contains: pm_add_edge, pm_kg_traverse

use crate::store::sqlite::SqliteStore;
use crate::store::{Store, NodeType, EdgeType};
use crate::validation;

pub fn tool_add_edge(store: &SqliteStore, st: &str, si: i64, tt: &str, ti: i64, rel: &str) -> String {
    let source_type = match st {
        "finding" | "f" => NodeType::Finding,
        "experiment" | "e" => NodeType::Experiment,
        "decision" | "d" => NodeType::Decision,
        "phase" | "p" => NodeType::Phase,
        "research" | "r" => NodeType::Research,
        "principle" | "pr" => NodeType::Principle,
        "hypothesis" | "h" => NodeType::Hypothesis,
        "constraint" | "co" => NodeType::Constraint,
        "literature" | "l" => NodeType::Literature,
        "feedback" | "fb" => NodeType::Feedback,
        _ => return format!("Unknown source type: {}", st),
    };
    let target_type = match tt {
        "finding" | "f" => NodeType::Finding,
        "experiment" | "e" => NodeType::Experiment,
        "decision" | "d" => NodeType::Decision,
        "phase" | "p" => NodeType::Phase,
        "research" | "r" => NodeType::Research,
        "principle" | "pr" => NodeType::Principle,
        "hypothesis" | "h" => NodeType::Hypothesis,
        "constraint" | "co" => NodeType::Constraint,
        "literature" | "l" => NodeType::Literature,
        "feedback" | "fb" => NodeType::Feedback,
        _ => return format!("Unknown target type: {}", tt),
    };
    let v = validation::validate_edge_relation(rel);
    if !v.is_ok() {
        return format!("\u{274c} VALIDATION ERROR:\n{}", v.to_mcp_error());
    }
    let relation = match rel {
        "supports" => EdgeType::Supports,
        "contradicts" => EdgeType::Contradicts,
        "depends" => EdgeType::DependsOn,
        "informed" => EdgeType::Informed,
        "supersedes" => EdgeType::Supersedes,
        "related" => EdgeType::RelatedTo,
        "produced" => EdgeType::ProducedBy,
        "cited" => EdgeType::CitedIn,
        "contains" => EdgeType::Contains,
        "derived_from" => EdgeType::DerivedFrom,
        "tested_by" => EdgeType::TestedBy,
        "violated_by" => EdgeType::ViolatedBy,
        _ => return format!("Unknown relation: {}", rel),
    };
    match store.create_edge(source_type, si, target_type, ti, relation) {
        Ok(e) => format!("Edge #{} added: {:?} #{} --{:?}--> {:?} #{}", e.id, e.source_type, si, e.relation, e.target_type, ti),
        Err(crate::store::StoreError::Constraint(msg)) => format!("\u{274c} CONSTRAINT ERROR: {}", msg),
        Err(e) => format!("Error: {}", e),
    }
}

pub fn tool_kg_traverse(store: &SqliteStore, nt_str: &str, nid: i64) -> String {
    let nt = match nt_str {
        "finding" | "f" => NodeType::Finding,
        "experiment" | "e" => NodeType::Experiment,
        "decision" | "d" => NodeType::Decision,
        "phase" | "p" => NodeType::Phase,
        "research" | "r" => NodeType::Research,
        "principle" | "pr" => NodeType::Principle,
        _ => return format!("Unknown node type: {}", nt_str),
    };
    let kg = crate::kg::KgEngine::new(store);
    match kg.traverse(nt, nid) {
        Ok(result) => {
            let mut text = format!("ROOT: {:?} #{}: {}\n", result.root.node_type, result.root.id, &result.root.label[..result.root.label.len().min(100)]);
            for (edge, target, incoming) in &result.edges {
                if *incoming {
                    text += &format!("  <--{:?}-- {:?} #{}: {}\n", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(80)]);
                } else {
                    text += &format!("  --{:?}--> {:?} #{}: {}\n", edge.relation, target.node_type, target.id, &target.label[..target.label.len().min(80)]);
                }
            }
            text
        }
        Err(e) => format!("Error: {}", e),
    }
}
