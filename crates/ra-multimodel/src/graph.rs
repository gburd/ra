//! Graph database optimization rules.
//!
//! Provides operators and rewrite rules for graph traversal patterns
//! used by databases like Neo4j, `JanusGraph`, and Amazon Neptune.
//! Key optimizations include join-to-traversal conversion,
//! bidirectional search, and vertex-centric index selection.

use serde::{Deserialize, Serialize};

use ra_core::cost::Cost;

/// Graph-specific operators that extend the relational algebra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GraphOp {
    /// Traverse edges from a source to a destination.
    Traverse {
        /// Source node expression.
        source: String,
        /// Edge type to follow.
        edge_type: String,
        /// Direction of traversal.
        direction: TraversalDirection,
    },

    /// Variable-length path traversal.
    VarLengthPath {
        /// Source node expression.
        source: String,
        /// Edge type to follow.
        edge_type: String,
        /// Minimum number of hops.
        min_hops: u32,
        /// Maximum number of hops.
        max_hops: u32,
    },

    /// Bidirectional BFS from two known endpoints.
    BidirectionalBfs {
        /// Source node.
        source: String,
        /// Target node.
        target: String,
        /// Maximum depth per direction.
        max_depth: u32,
    },

    /// Expand-into: check edge existence between two bound nodes.
    ExpandInto {
        /// Source node.
        source: String,
        /// Target node.
        target: String,
        /// Edge type to check.
        edge_type: String,
    },

    /// Label-filtered node scan.
    LabelScan {
        /// The node label to scan.
        label: String,
    },

    /// Vertex-centric index scan on edge properties.
    VertexCentricScan {
        /// The vertex to expand from.
        vertex: String,
        /// Edge type.
        edge_type: String,
        /// Property to filter on.
        property: String,
    },
}

/// Direction of a graph traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraversalDirection {
    /// Follow edges in their stored direction.
    Outgoing,
    /// Follow edges against their stored direction.
    Incoming,
    /// Follow edges in either direction.
    Both,
}

/// Statistics about a graph schema element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphStats {
    /// Total number of vertices.
    pub vertex_count: f64,
    /// Total number of edges.
    pub edge_count: f64,
    /// Per-label vertex counts.
    pub label_counts: Vec<LabelStats>,
    /// Per-edge-type statistics.
    pub edge_type_stats: Vec<EdgeTypeStats>,
}

/// Statistics for a specific node label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelStats {
    /// The label name.
    pub label: String,
    /// Number of vertices with this label.
    pub count: f64,
    /// Average degree (edges per vertex) for this label.
    pub avg_degree: f64,
}

/// Statistics for a specific edge type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeTypeStats {
    /// The edge type name.
    pub edge_type: String,
    /// Number of edges of this type.
    pub count: f64,
    /// Average out-degree for source vertices.
    pub avg_out_degree: f64,
    /// Average in-degree for target vertices.
    pub avg_in_degree: f64,
}

/// Convert a non-negative f64 to u64 for memory estimates, clamping
/// negative and overflow values.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f64_to_mem(val: f64) -> u64 {
    if val <= 0.0 {
        0
    } else if val >= u64::MAX as f64 {
        u64::MAX
    } else {
        val as u64
    }
}

/// Estimate cost for a graph traversal operation.
#[must_use]
pub fn estimate_traversal_cost(
    source_count: f64,
    avg_degree: f64,
    hops: u32,
    selectivity: f64,
) -> Cost {
    let mut rows = source_count;
    for _ in 0..hops {
        rows *= avg_degree * selectivity;
    }
    Cost::new(rows * 0.1, rows * 0.01, 0.0, f64_to_mem(rows * 64.0))
}

/// Estimate cost for a bidirectional BFS.
#[must_use]
pub fn estimate_bidirectional_cost(branching_factor: f64, depth: u32) -> Cost {
    let half_depth = depth.div_ceil(2);
    let exp = i32::try_from(half_depth).unwrap_or(i32::MAX);
    let nodes_explored = 2.0 * branching_factor.powi(exp);
    Cost::new(
        nodes_explored * 0.2,
        nodes_explored * 0.05,
        0.0,
        f64_to_mem(nodes_explored * 128.0),
    )
}

/// Estimate cost for a label scan.
#[must_use]
pub fn estimate_label_scan_cost(label_count: f64) -> Cost {
    Cost::new(
        label_count * 0.01,
        label_count * 0.005,
        0.0,
        f64_to_mem(label_count * 32.0),
    )
}

/// Estimate cost for a vertex-centric index scan.
#[must_use]
pub fn estimate_vc_index_cost(avg_degree: f64, selectivity: f64) -> Cost {
    let index_cost = avg_degree.ln().max(1.0);
    let result_cost = avg_degree * selectivity;
    Cost::new(
        index_cost + result_cost * 0.1,
        index_cost * 0.5,
        0.0,
        f64_to_mem(result_cost * 64.0),
    )
}

/// Estimate cost for an expand-into operation.
#[must_use]
pub fn estimate_expand_into_cost(avg_degree: f64) -> Cost {
    let probe_cost = avg_degree.ln().max(1.0);
    Cost::new(probe_cost * 0.1, probe_cost * 0.05, 0.0, 128)
}

impl std::fmt::Display for TraversalDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Outgoing => write!(f, "OUTGOING"),
            Self::Incoming => write!(f, "INCOMING"),
            Self::Both => write!(f, "BOTH"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn traversal_direction_display() {
        assert_eq!(TraversalDirection::Outgoing.to_string(), "OUTGOING");
        assert_eq!(TraversalDirection::Incoming.to_string(), "INCOMING");
        assert_eq!(TraversalDirection::Both.to_string(), "BOTH");
    }

    #[test]
    fn graph_op_traverse_roundtrip() {
        let op = GraphOp::Traverse {
            source: "person".into(),
            edge_type: "KNOWS".into(),
            direction: TraversalDirection::Outgoing,
        };
        let json = serde_json::to_string(&op).expect("serialization should succeed");
        let deserialized: GraphOp =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(op, deserialized);
    }

    #[test]
    fn graph_op_var_length_path() {
        let op = GraphOp::VarLengthPath {
            source: "alice".into(),
            edge_type: "KNOWS".into(),
            min_hops: 1,
            max_hops: 4,
        };
        if let GraphOp::VarLengthPath {
            min_hops, max_hops, ..
        } = &op
        {
            assert_eq!(*min_hops, 1);
            assert_eq!(*max_hops, 4);
        } else {
            panic!("expected VarLengthPath");
        }
    }

    #[test]
    fn graph_op_bidirectional() {
        let op = GraphOp::BidirectionalBfs {
            source: "alice".into(),
            target: "bob".into(),
            max_depth: 6,
        };
        let json = serde_json::to_string(&op).expect("serialization should succeed");
        let deserialized: GraphOp =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(op, deserialized);
    }

    #[test]
    fn graph_stats_roundtrip() {
        let stats = GraphStats {
            vertex_count: 1_000_000.0,
            edge_count: 5_000_000.0,
            label_counts: vec![LabelStats {
                label: "Person".into(),
                count: 500_000.0,
                avg_degree: 10.0,
            }],
            edge_type_stats: vec![EdgeTypeStats {
                edge_type: "KNOWS".into(),
                count: 2_500_000.0,
                avg_out_degree: 5.0,
                avg_in_degree: 5.0,
            }],
        };
        let json = serde_json::to_string(&stats).expect("serialization should succeed");
        let deserialized: GraphStats =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(stats, deserialized);
    }

    #[test]
    fn traversal_cost_single_hop() {
        let cost = estimate_traversal_cost(100.0, 10.0, 1, 1.0);
        assert!(cost.cpu > 0.0);
        assert!(cost.io > 0.0);
    }

    #[test]
    fn traversal_cost_scales_with_hops() {
        let cost_1 = estimate_traversal_cost(100.0, 10.0, 1, 1.0);
        let cost_2 = estimate_traversal_cost(100.0, 10.0, 2, 1.0);
        assert!(cost_2.total() > cost_1.total());
    }

    #[test]
    fn traversal_cost_selectivity_reduces() {
        let full = estimate_traversal_cost(100.0, 10.0, 2, 1.0);
        let selective = estimate_traversal_cost(100.0, 10.0, 2, 0.1);
        assert!(selective.total() < full.total());
    }

    #[test]
    fn bidirectional_cost_less_than_unidirectional() {
        let bidir = estimate_bidirectional_cost(10.0, 6);
        let unidir = estimate_traversal_cost(1.0, 10.0, 6, 1.0);
        assert!(bidir.total() < unidir.total());
    }

    #[test]
    fn label_scan_cost_proportional() {
        let small = estimate_label_scan_cost(100.0);
        let large = estimate_label_scan_cost(100_000.0);
        assert!(large.total() > small.total());
    }

    #[test]
    fn vc_index_cost_less_than_full_scan() {
        let full = estimate_traversal_cost(1.0, 10_000.0, 1, 1.0);
        let indexed = estimate_vc_index_cost(10_000.0, 0.01);
        assert!(indexed.total() < full.total());
    }

    #[test]
    fn expand_into_cost_log_degree() {
        let low_degree = estimate_expand_into_cost(10.0);
        let high_degree = estimate_expand_into_cost(10_000.0);
        assert!(high_degree.total() > low_degree.total());
        assert!(high_degree.total() < low_degree.total() * 1000.0);
    }
}
