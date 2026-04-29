//! Network topology modeling for distributed query optimization.
//!
//! Models network links between cluster nodes with realistic bandwidth,
//! latency, and cloud billing costs. Supports topologies from
//! single-rack clusters to cross-region cloud federations.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Unique identifier for a node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "node-{}", self.0)
    }
}

/// Physical or logical location of a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Region (e.g. "us-east-1", "eu-west-1").
    pub region: String,
    /// Datacenter or availability zone within the region.
    pub datacenter: String,
    /// Optional rack identifier within the datacenter.
    pub rack: Option<String>,
}

impl Location {
    /// Create a new location.
    #[must_use]
    pub fn new(region: impl Into<String>, datacenter: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            datacenter: datacenter.into(),
            rack: None,
        }
    }

    /// Create a location with a rack assignment.
    #[must_use]
    pub fn with_rack(
        region: impl Into<String>,
        datacenter: impl Into<String>,
        rack: impl Into<String>,
    ) -> Self {
        Self {
            region: region.into(),
            datacenter: datacenter.into(),
            rack: Some(rack.into()),
        }
    }
}

/// Classification of network link by physical topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkType {
    /// Same rack (10-100 Gbps, <1us latency).
    IntraRack,
    /// Same datacenter, different rack (1-10 Gbps, 1-10us latency).
    IntraDatacenter,
    /// Cross-datacenter within same region (100Mbps-1Gbps, 1-50ms).
    CrossDatacenter,
    /// Cross-region (10-100Mbps, 50-200ms latency).
    CrossRegion,
    /// Public internet (variable bandwidth, high latency).
    Internet,
}

impl LinkType {
    /// Default bandwidth in bytes per second for this link type.
    #[must_use]
    pub fn default_bandwidth(&self) -> u64 {
        match self {
            Self::IntraRack => 12_500_000_000,      // 100 Gbps
            Self::IntraDatacenter => 1_250_000_000, // 10 Gbps
            Self::CrossDatacenter => 125_000_000,   // 1 Gbps
            Self::CrossRegion => 12_500_000,        // 100 Mbps
            Self::Internet => 6_250_000,            // 50 Mbps
        }
    }

    /// Default latency in microseconds for this link type.
    #[must_use]
    pub fn default_latency_us(&self) -> u64 {
        match self {
            Self::IntraRack => 1,
            Self::IntraDatacenter => 5,
            Self::CrossDatacenter => 5_000, // 5ms
            Self::CrossRegion => 100_000,   // 100ms
            Self::Internet => 150_000,      // 150ms
        }
    }

    /// Default cost per GB for cloud billing.
    #[must_use]
    pub fn default_cost_per_gb(&self) -> f64 {
        match self {
            Self::IntraRack | Self::IntraDatacenter => 0.0,
            Self::CrossDatacenter => 0.01, // AWS cross-AZ
            Self::CrossRegion => 0.02,     // AWS cross-region
            Self::Internet => 0.09,        // AWS internet egress
        }
    }
}

impl fmt::Display for LinkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntraRack => write!(f, "intra-rack"),
            Self::IntraDatacenter => write!(f, "intra-datacenter"),
            Self::CrossDatacenter => write!(f, "cross-datacenter"),
            Self::CrossRegion => write!(f, "cross-region"),
            Self::Internet => write!(f, "internet"),
        }
    }
}

/// A network link between two nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkLink {
    /// Bandwidth in bytes per second.
    pub bandwidth: u64,
    /// One-way latency in microseconds.
    pub latency_us: u64,
    /// Cost per GB transferred (for cloud billing).
    pub cost_per_gb: f64,
    /// Classification of the link.
    pub link_type: LinkType,
}

impl NetworkLink {
    /// Create a new network link.
    #[must_use]
    pub fn new(bandwidth: u64, latency_us: u64, cost_per_gb: f64, link_type: LinkType) -> Self {
        Self {
            bandwidth,
            latency_us,
            cost_per_gb,
            link_type,
        }
    }

    /// Create a link with default parameters for the given type.
    #[must_use]
    pub fn from_type(link_type: LinkType) -> Self {
        Self {
            bandwidth: link_type.default_bandwidth(),
            latency_us: link_type.default_latency_us(),
            cost_per_gb: link_type.default_cost_per_gb(),
            link_type,
        }
    }

    /// Estimate transfer time for the given number of bytes.
    #[must_use]
    pub fn transfer_time(&self, bytes: u64) -> Duration {
        let latency = Duration::from_micros(self.latency_us);
        let transfer = Duration::from_secs_f64(bytes as f64 / self.bandwidth as f64);
        latency + transfer
    }

    /// Estimate cloud billing cost for the given number of bytes.
    #[must_use]
    pub fn transfer_cost(&self, bytes: u64) -> f64 {
        const BYTES_PER_GB: f64 = 1_073_741_824.0;
        (bytes as f64 / BYTES_PER_GB) * self.cost_per_gb
    }

    /// Effective throughput in bytes/sec accounting for latency.
    ///
    /// For small transfers, latency dominates; for large transfers,
    /// bandwidth dominates.
    #[must_use]
    pub fn effective_throughput(&self, bytes: u64) -> f64 {
        let total_secs = self.transfer_time(bytes).as_secs_f64();
        if total_secs <= 0.0 {
            return self.bandwidth as f64;
        }
        bytes as f64 / total_secs
    }
}

/// Network topology describing the connectivity between cluster nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTopology {
    /// Nodes in the cluster.
    pub nodes: Vec<NodeId>,
    /// Network links between ordered node pairs `(from, to)`.
    ///
    /// Links are directional; both `(a, b)` and `(b, a)` can exist
    /// with different characteristics (asymmetric bandwidth).
    #[serde(with = "link_map_serde")]
    links: HashMap<(NodeId, NodeId), NetworkLink>,
    /// Physical location of each node.
    locations: HashMap<NodeId, Location>,
    /// Fallback link used when no explicit link is configured.
    default_link: NetworkLink,
}

/// Custom serialization for `HashMap<(NodeId, NodeId), NetworkLink>`
/// since JSON requires string keys.
mod link_map_serde {
    use super::{fmt, HashMap, NetworkLink, NodeId};
    use serde::de::{self, MapAccess, Visitor};
    use serde::ser::SerializeMap;

    pub fn serialize<S>(
        map: &HashMap<(NodeId, NodeId), NetworkLink>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut ser_map = serializer.serialize_map(Some(map.len()))?;
        for ((from, to), link) in map {
            let key = format!("{}->{}", from.0, to.0);
            ser_map.serialize_entry(&key, link)?;
        }
        ser_map.end()
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<(NodeId, NodeId), NetworkLink>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct LinkMapVisitor;

        impl<'de> Visitor<'de> for LinkMapVisitor {
            type Value = HashMap<(NodeId, NodeId), NetworkLink>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map with keys like \"0->1\"")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut map = HashMap::with_capacity(access.size_hint().unwrap_or(0));
                while let Some((key, value)) = access.next_entry::<String, NetworkLink>()? {
                    let parts: Vec<&str> = key.split("->").collect();
                    if parts.len() != 2 {
                        return Err(de::Error::custom(format!("invalid link key: {key}")));
                    }
                    let from: u32 = parts[0].parse().map_err(de::Error::custom)?;
                    let to: u32 = parts[1].parse().map_err(de::Error::custom)?;
                    map.insert((NodeId(from), NodeId(to)), value);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_map(LinkMapVisitor)
    }
}

/// Errors from topology operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TopologyError {
    /// Referenced node is not in the topology.
    #[error("unknown node: {0}")]
    UnknownNode(NodeId),
    /// No link exists between the nodes and no default is suitable.
    #[error("no link between {0} and {1}")]
    NoLink(NodeId, NodeId),
}

impl NetworkTopology {
    /// Create a new empty topology.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            links: HashMap::new(),
            locations: HashMap::new(),
            default_link: NetworkLink::from_type(LinkType::Internet),
        }
    }

    /// Create a topology with the given default link for unknown pairs.
    #[must_use]
    pub fn with_default_link(default_link: NetworkLink) -> Self {
        Self {
            nodes: Vec::new(),
            links: HashMap::new(),
            locations: HashMap::new(),
            default_link,
        }
    }

    /// Add a node to the topology.
    pub fn add_node(&mut self, id: NodeId, location: Location) {
        if !self.nodes.contains(&id) {
            self.nodes.push(id);
        }
        self.locations.insert(id, location);
    }

    /// Add a bidirectional link between two nodes.
    pub fn add_link(&mut self, a: NodeId, b: NodeId, link: NetworkLink) {
        self.links.insert((a, b), link.clone());
        self.links.insert((b, a), link);
    }

    /// Add a directional link from one node to another.
    pub fn add_directional_link(&mut self, from: NodeId, to: NodeId, link: NetworkLink) {
        self.links.insert((from, to), link);
    }

    /// Get the link between two nodes, falling back to the default.
    #[must_use]
    pub fn get_link(&self, from: NodeId, to: NodeId) -> &NetworkLink {
        self.links.get(&(from, to)).unwrap_or(&self.default_link)
    }

    /// Get the link between two nodes without fallback.
    #[must_use]
    pub fn get_explicit_link(&self, from: NodeId, to: NodeId) -> Option<&NetworkLink> {
        self.links.get(&(from, to))
    }

    /// Get the location of a node.
    #[must_use]
    pub fn get_location(&self, node: NodeId) -> Option<&Location> {
        self.locations.get(&node)
    }

    /// Number of nodes in the topology.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of configured links (counting each direction separately).
    #[must_use]
    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// Estimate transfer time for data between two nodes.
    #[must_use]
    pub fn transfer_time(&self, from: NodeId, to: NodeId, bytes: u64) -> Duration {
        if from == to {
            return Duration::ZERO;
        }
        self.get_link(from, to).transfer_time(bytes)
    }

    /// Estimate cloud billing cost for data transfer between nodes.
    #[must_use]
    pub fn transfer_cost(&self, from: NodeId, to: NodeId, bytes: u64) -> f64 {
        if from == to {
            return 0.0;
        }
        self.get_link(from, to).transfer_cost(bytes)
    }

    /// Check if two nodes are in the same datacenter.
    #[must_use]
    pub fn same_datacenter(&self, a: NodeId, b: NodeId) -> bool {
        match (self.locations.get(&a), self.locations.get(&b)) {
            (Some(la), Some(lb)) => la.datacenter == lb.datacenter,
            _ => false,
        }
    }

    /// Check if two nodes are in the same region.
    #[must_use]
    pub fn same_region(&self, a: NodeId, b: NodeId) -> bool {
        match (self.locations.get(&a), self.locations.get(&b)) {
            (Some(la), Some(lb)) => la.region == lb.region,
            _ => false,
        }
    }

    /// Check if two nodes are in the same rack.
    #[must_use]
    pub fn same_rack(&self, a: NodeId, b: NodeId) -> bool {
        match (self.locations.get(&a), self.locations.get(&b)) {
            (Some(la), Some(lb)) => {
                la.datacenter == lb.datacenter && la.rack.is_some() && la.rack == lb.rack
            }
            _ => false,
        }
    }

    /// Infer the link type between two nodes from their locations.
    #[must_use]
    pub fn infer_link_type(&self, a: NodeId, b: NodeId) -> LinkType {
        if a == b {
            return LinkType::IntraRack;
        }
        match (self.locations.get(&a), self.locations.get(&b)) {
            (Some(la), Some(lb)) => {
                if la.datacenter == lb.datacenter {
                    if la.rack.is_some() && la.rack == lb.rack {
                        LinkType::IntraRack
                    } else {
                        LinkType::IntraDatacenter
                    }
                } else if la.region == lb.region {
                    LinkType::CrossDatacenter
                } else {
                    LinkType::CrossRegion
                }
            }
            _ => LinkType::Internet,
        }
    }

    /// Get all nodes in a specific datacenter.
    #[must_use]
    pub fn nodes_in_datacenter(&self, datacenter: &str) -> Vec<NodeId> {
        self.locations
            .iter()
            .filter(|(_, loc)| loc.datacenter == datacenter)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Get all nodes in a specific region.
    #[must_use]
    pub fn nodes_in_region(&self, region: &str) -> Vec<NodeId> {
        self.locations
            .iter()
            .filter(|(_, loc)| loc.region == region)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Get all unique datacenters in the topology.
    #[must_use]
    pub fn datacenters(&self) -> Vec<String> {
        let mut dcs: Vec<String> = self
            .locations
            .values()
            .map(|loc| loc.datacenter.clone())
            .collect();
        dcs.sort();
        dcs.dedup();
        dcs
    }

    /// Get all unique regions in the topology.
    #[must_use]
    pub fn regions(&self) -> Vec<String> {
        let mut regions: Vec<String> = self
            .locations
            .values()
            .map(|loc| loc.region.clone())
            .collect();
        regions.sort();
        regions.dedup();
        regions
    }

    /// Find the cheapest (lowest billing cost) link between two nodes.
    /// Returns the direct link cost; does not consider multi-hop routing.
    #[must_use]
    pub fn cheapest_cost(&self, from: NodeId, to: NodeId, bytes: u64) -> f64 {
        self.transfer_cost(from, to, bytes)
    }

    /// Find the fastest (lowest latency+transfer) link between nodes.
    #[must_use]
    pub fn fastest_time(&self, from: NodeId, to: NodeId, bytes: u64) -> Duration {
        self.transfer_time(from, to, bytes)
    }

    /// Compute the total cost to broadcast data from one node to all
    /// specified targets.
    #[must_use]
    pub fn broadcast_cost(&self, source: NodeId, targets: &[NodeId], bytes: u64) -> BroadcastCost {
        let mut total_time = Duration::ZERO;
        let mut max_time = Duration::ZERO;
        let mut total_billing = 0.0;

        for &target in targets {
            if target == source {
                continue;
            }
            let time = self.transfer_time(source, target, bytes);
            let billing = self.transfer_cost(source, target, bytes);
            total_billing += billing;
            total_time += time;
            if time > max_time {
                max_time = time;
            }
        }

        BroadcastCost {
            total_time,
            max_time,
            total_billing,
            target_count: targets.len(),
        }
    }
}

impl Default for NetworkTopology {
    fn default() -> Self {
        Self::new()
    }
}

/// Cost breakdown for a broadcast operation.
#[derive(Debug, Clone)]
pub struct BroadcastCost {
    /// Sum of all transfer times (sequential).
    pub total_time: Duration,
    /// Maximum transfer time to any single target (parallel).
    pub max_time: Duration,
    /// Total cloud billing cost.
    pub total_billing: f64,
    /// Number of targets.
    pub target_count: usize,
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "Exact float equality needed for deterministic network cost tests"
)]
mod tests {
    use super::*;

    fn two_node_topology() -> NetworkTopology {
        let mut topo = NetworkTopology::new();
        let n0 = NodeId(0);
        let n1 = NodeId(1);
        topo.add_node(n0, Location::with_rack("us-east-1", "us-east-1a", "rack-1"));
        topo.add_node(n1, Location::with_rack("us-east-1", "us-east-1a", "rack-2"));
        topo.add_link(n0, n1, NetworkLink::from_type(LinkType::IntraDatacenter));
        topo
    }

    fn cross_region_topology() -> NetworkTopology {
        let mut topo = NetworkTopology::new();
        let n0 = NodeId(0);
        let n1 = NodeId(1);
        topo.add_node(n0, Location::new("us-east-1", "us-east-1a"));
        topo.add_node(n1, Location::new("eu-west-1", "eu-west-1a"));
        topo.add_link(n0, n1, NetworkLink::from_type(LinkType::CrossRegion));
        topo
    }

    #[test]
    fn node_id_display() {
        assert_eq!(format!("{}", NodeId(42)), "node-42");
    }

    #[test]
    fn location_new() {
        let loc = Location::new("us-east-1", "us-east-1a");
        assert_eq!(loc.region, "us-east-1");
        assert_eq!(loc.datacenter, "us-east-1a");
        assert!(loc.rack.is_none());
    }

    #[test]
    fn location_with_rack() {
        let loc = Location::with_rack("us-east-1", "us-east-1a", "rack-1");
        assert_eq!(loc.rack.as_deref(), Some("rack-1"));
    }

    #[test]
    fn link_type_defaults_intra_rack() {
        let lt = LinkType::IntraRack;
        assert_eq!(lt.default_bandwidth(), 12_500_000_000);
        assert_eq!(lt.default_latency_us(), 1);
        assert_eq!(lt.default_cost_per_gb(), 0.0);
    }

    #[test]
    fn link_type_defaults_cross_region() {
        let lt = LinkType::CrossRegion;
        assert_eq!(lt.default_bandwidth(), 12_500_000);
        assert_eq!(lt.default_latency_us(), 100_000);
        assert!((lt.default_cost_per_gb() - 0.02).abs() < f64::EPSILON);
    }

    #[test]
    fn link_type_display() {
        assert_eq!(format!("{}", LinkType::IntraRack), "intra-rack");
        assert_eq!(format!("{}", LinkType::CrossDatacenter), "cross-datacenter");
    }

    #[test]
    fn network_link_from_type() {
        let link = NetworkLink::from_type(LinkType::IntraDatacenter);
        assert_eq!(link.bandwidth, 1_250_000_000);
        assert_eq!(link.latency_us, 5);
        assert_eq!(link.cost_per_gb, 0.0);
    }

    #[test]
    fn network_link_transfer_time_small() {
        let link = NetworkLink::from_type(LinkType::IntraRack);
        let time = link.transfer_time(1000);
        // Latency: 1us, transfer: 1000 / 12.5e9 ~ 80ns
        assert!(time.as_nanos() > 0);
        assert!(time < Duration::from_millis(1));
    }

    #[test]
    fn network_link_transfer_time_large() {
        let link = NetworkLink::from_type(LinkType::CrossRegion);
        // 1 GB at 100 Mbps = ~80 seconds + 100ms latency
        let one_gb = 1_073_741_824;
        let time = link.transfer_time(one_gb);
        assert!(time.as_secs() > 50);
        assert!(time.as_secs() < 200);
    }

    #[test]
    fn network_link_transfer_cost_free() {
        let link = NetworkLink::from_type(LinkType::IntraRack);
        assert_eq!(link.transfer_cost(1_000_000), 0.0);
    }

    #[test]
    fn network_link_transfer_cost_cross_region() {
        let link = NetworkLink::from_type(LinkType::CrossRegion);
        // 1 GB at $0.02/GB
        let one_gb = 1_073_741_824;
        let cost = link.transfer_cost(one_gb);
        assert!((cost - 0.02).abs() < 0.001);
    }

    #[test]
    fn network_link_effective_throughput() {
        let link = NetworkLink::from_type(LinkType::IntraRack);
        // For large transfers, effective throughput approaches bandwidth
        let throughput = link.effective_throughput(1_000_000_000);
        let bw = link.bandwidth as f64;
        assert!(throughput > bw * 0.99);
    }

    #[test]
    fn topology_new_is_empty() {
        let topo = NetworkTopology::new();
        assert_eq!(topo.node_count(), 0);
        assert_eq!(topo.link_count(), 0);
    }

    #[test]
    fn topology_add_node() {
        let mut topo = NetworkTopology::new();
        topo.add_node(NodeId(0), Location::new("us-east-1", "us-east-1a"));
        assert_eq!(topo.node_count(), 1);
    }

    #[test]
    fn topology_add_node_idempotent() {
        let mut topo = NetworkTopology::new();
        let loc = Location::new("us-east-1", "us-east-1a");
        topo.add_node(NodeId(0), loc.clone());
        topo.add_node(NodeId(0), loc);
        assert_eq!(topo.node_count(), 1);
    }

    #[test]
    fn topology_add_bidirectional_link() {
        let topo = two_node_topology();
        // Bidirectional: 2 directional links
        assert_eq!(topo.link_count(), 2);
    }

    #[test]
    fn topology_get_link_returns_configured() {
        let topo = two_node_topology();
        let link = topo.get_link(NodeId(0), NodeId(1));
        assert_eq!(link.link_type, LinkType::IntraDatacenter);
    }

    #[test]
    fn topology_get_link_falls_back_to_default() {
        let topo = two_node_topology();
        // No link configured between node 0 and non-existent node 99
        let link = topo.get_link(NodeId(0), NodeId(99));
        assert_eq!(link.link_type, LinkType::Internet);
    }

    #[test]
    fn topology_get_explicit_link() {
        let topo = two_node_topology();
        assert!(topo.get_explicit_link(NodeId(0), NodeId(1)).is_some());
        assert!(topo.get_explicit_link(NodeId(0), NodeId(99)).is_none());
    }

    #[test]
    fn topology_transfer_time_same_node() {
        let topo = two_node_topology();
        let time = topo.transfer_time(NodeId(0), NodeId(0), 1_000_000);
        assert_eq!(time, Duration::ZERO);
    }

    #[test]
    fn topology_transfer_time_different_nodes() {
        let topo = two_node_topology();
        let time = topo.transfer_time(NodeId(0), NodeId(1), 1_000_000);
        assert!(time > Duration::ZERO);
    }

    #[test]
    fn topology_transfer_cost_same_node() {
        let topo = two_node_topology();
        let cost = topo.transfer_cost(NodeId(0), NodeId(0), 1_000_000);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn topology_transfer_cost_cross_region() {
        let topo = cross_region_topology();
        let one_gb = 1_073_741_824;
        let cost = topo.transfer_cost(NodeId(0), NodeId(1), one_gb);
        assert!(cost > 0.0);
    }

    #[test]
    fn topology_same_datacenter() {
        let topo = two_node_topology();
        assert!(topo.same_datacenter(NodeId(0), NodeId(1)));
    }

    #[test]
    fn topology_different_datacenter() {
        let topo = cross_region_topology();
        assert!(!topo.same_datacenter(NodeId(0), NodeId(1)));
    }

    #[test]
    fn topology_same_region() {
        let topo = two_node_topology();
        assert!(topo.same_region(NodeId(0), NodeId(1)));
    }

    #[test]
    fn topology_different_region() {
        let topo = cross_region_topology();
        assert!(!topo.same_region(NodeId(0), NodeId(1)));
    }

    #[test]
    fn topology_same_rack() {
        let mut topo = NetworkTopology::new();
        let n0 = NodeId(0);
        let n1 = NodeId(1);
        topo.add_node(n0, Location::with_rack("us-east-1", "us-east-1a", "rack-1"));
        topo.add_node(n1, Location::with_rack("us-east-1", "us-east-1a", "rack-1"));
        assert!(topo.same_rack(n0, n1));
    }

    #[test]
    fn topology_different_rack() {
        let topo = two_node_topology();
        assert!(!topo.same_rack(NodeId(0), NodeId(1)));
    }

    #[test]
    fn topology_same_rack_requires_rack_field() {
        let mut topo = NetworkTopology::new();
        topo.add_node(NodeId(0), Location::new("us-east-1", "us-east-1a"));
        topo.add_node(NodeId(1), Location::new("us-east-1", "us-east-1a"));
        // No rack info, so same_rack is false
        assert!(!topo.same_rack(NodeId(0), NodeId(1)));
    }

    #[test]
    fn topology_infer_link_type_same_rack() {
        let mut topo = NetworkTopology::new();
        topo.add_node(NodeId(0), Location::with_rack("us-east-1", "dc1", "r1"));
        topo.add_node(NodeId(1), Location::with_rack("us-east-1", "dc1", "r1"));
        assert_eq!(
            topo.infer_link_type(NodeId(0), NodeId(1)),
            LinkType::IntraRack
        );
    }

    #[test]
    fn topology_infer_link_type_same_dc() {
        let topo = two_node_topology();
        assert_eq!(
            topo.infer_link_type(NodeId(0), NodeId(1)),
            LinkType::IntraDatacenter
        );
    }

    #[test]
    fn topology_infer_link_type_cross_dc() {
        let mut topo = NetworkTopology::new();
        topo.add_node(NodeId(0), Location::new("us-east-1", "us-east-1a"));
        topo.add_node(NodeId(1), Location::new("us-east-1", "us-east-1b"));
        assert_eq!(
            topo.infer_link_type(NodeId(0), NodeId(1)),
            LinkType::CrossDatacenter
        );
    }

    #[test]
    fn topology_infer_link_type_cross_region() {
        let topo = cross_region_topology();
        assert_eq!(
            topo.infer_link_type(NodeId(0), NodeId(1)),
            LinkType::CrossRegion
        );
    }

    #[test]
    fn topology_infer_link_type_self() {
        let topo = two_node_topology();
        assert_eq!(
            topo.infer_link_type(NodeId(0), NodeId(0)),
            LinkType::IntraRack
        );
    }

    #[test]
    fn topology_nodes_in_datacenter() {
        let topo = two_node_topology();
        let nodes = topo.nodes_in_datacenter("us-east-1a");
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn topology_nodes_in_datacenter_empty() {
        let topo = two_node_topology();
        let nodes = topo.nodes_in_datacenter("nonexistent");
        assert!(nodes.is_empty());
    }

    #[test]
    fn topology_nodes_in_region() {
        let topo = cross_region_topology();
        let us_nodes = topo.nodes_in_region("us-east-1");
        assert_eq!(us_nodes.len(), 1);
        let eu_nodes = topo.nodes_in_region("eu-west-1");
        assert_eq!(eu_nodes.len(), 1);
    }

    #[test]
    fn topology_datacenters() {
        let topo = cross_region_topology();
        let dcs = topo.datacenters();
        assert_eq!(dcs.len(), 2);
    }

    #[test]
    fn topology_regions() {
        let topo = cross_region_topology();
        let regions = topo.regions();
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn topology_broadcast_cost_single_target() {
        let topo = two_node_topology();
        let cost = topo.broadcast_cost(NodeId(0), &[NodeId(1)], 1_000_000);
        assert_eq!(cost.target_count, 1);
        assert!(cost.max_time > Duration::ZERO);
    }

    #[test]
    fn topology_broadcast_cost_includes_self() {
        let topo = two_node_topology();
        let cost = topo.broadcast_cost(NodeId(0), &[NodeId(0), NodeId(1)], 1_000_000);
        // Self-transfer should not add time
        assert_eq!(cost.target_count, 2);
    }

    #[test]
    fn topology_broadcast_cost_multiple_targets() {
        let mut topo = NetworkTopology::new();
        for i in 0..4 {
            topo.add_node(NodeId(i), Location::new("us-east-1", "us-east-1a"));
        }
        for i in 1..4 {
            topo.add_link(
                NodeId(0),
                NodeId(i),
                NetworkLink::from_type(LinkType::IntraDatacenter),
            );
        }
        let targets: Vec<NodeId> = (1..4).map(NodeId).collect();
        let cost = topo.broadcast_cost(NodeId(0), &targets, 1_000_000);
        assert_eq!(cost.target_count, 3);
        assert!(cost.total_time > cost.max_time);
    }

    #[test]
    fn topology_directional_link() {
        let mut topo = NetworkTopology::new();
        topo.add_node(NodeId(0), Location::new("us-east-1", "dc1"));
        topo.add_node(NodeId(1), Location::new("us-east-1", "dc1"));
        topo.add_directional_link(
            NodeId(0),
            NodeId(1),
            NetworkLink::new(1_000_000_000, 10, 0.01, LinkType::IntraDatacenter),
        );
        // Forward link exists
        assert!(topo.get_explicit_link(NodeId(0), NodeId(1)).is_some());
        // Reverse link does not
        assert!(topo.get_explicit_link(NodeId(1), NodeId(0)).is_none());
    }

    #[test]
    fn topology_with_custom_default_link() {
        let custom = NetworkLink::new(500_000_000, 50, 0.05, LinkType::CrossDatacenter);
        let topo = NetworkTopology::with_default_link(custom);
        let link = topo.get_link(NodeId(0), NodeId(99));
        assert_eq!(link.link_type, LinkType::CrossDatacenter);
    }

    #[test]
    fn topology_cheapest_cost_same_as_transfer_cost() {
        let topo = cross_region_topology();
        let bytes = 1_000_000;
        let tc = topo.transfer_cost(NodeId(0), NodeId(1), bytes);
        let cc = topo.cheapest_cost(NodeId(0), NodeId(1), bytes);
        assert!((tc - cc).abs() < f64::EPSILON);
    }

    #[test]
    fn topology_fastest_time_same_as_transfer_time() {
        let topo = cross_region_topology();
        let bytes = 1_000_000;
        let tt = topo.transfer_time(NodeId(0), NodeId(1), bytes);
        let ft = topo.fastest_time(NodeId(0), NodeId(1), bytes);
        assert_eq!(tt, ft);
    }

    #[test]
    fn link_type_bandwidth_ordering() {
        // Faster links should have higher bandwidth
        assert!(
            LinkType::IntraRack.default_bandwidth() > LinkType::IntraDatacenter.default_bandwidth()
        );
        assert!(
            LinkType::IntraDatacenter.default_bandwidth()
                > LinkType::CrossDatacenter.default_bandwidth()
        );
        assert!(
            LinkType::CrossDatacenter.default_bandwidth()
                > LinkType::CrossRegion.default_bandwidth()
        );
        assert!(LinkType::CrossRegion.default_bandwidth() > LinkType::Internet.default_bandwidth());
    }

    #[test]
    fn link_type_latency_ordering() {
        // Faster links should have lower latency
        assert!(
            LinkType::IntraRack.default_latency_us()
                < LinkType::IntraDatacenter.default_latency_us()
        );
        assert!(
            LinkType::IntraDatacenter.default_latency_us()
                < LinkType::CrossDatacenter.default_latency_us()
        );
        assert!(
            LinkType::CrossDatacenter.default_latency_us()
                < LinkType::CrossRegion.default_latency_us()
        );
        assert!(
            LinkType::CrossRegion.default_latency_us() < LinkType::Internet.default_latency_us()
        );
    }

    #[test]
    fn link_type_cost_ordering() {
        // More distant links should cost more
        assert!(
            LinkType::IntraRack.default_cost_per_gb()
                <= LinkType::IntraDatacenter.default_cost_per_gb()
        );
        assert!(
            LinkType::IntraDatacenter.default_cost_per_gb()
                <= LinkType::CrossDatacenter.default_cost_per_gb()
        );
        assert!(
            LinkType::CrossDatacenter.default_cost_per_gb()
                <= LinkType::CrossRegion.default_cost_per_gb()
        );
        assert!(
            LinkType::CrossRegion.default_cost_per_gb() <= LinkType::Internet.default_cost_per_gb()
        );
    }

    #[test]
    fn topology_get_location() {
        let topo = two_node_topology();
        let loc = topo.get_location(NodeId(0));
        assert!(loc.is_some());
        assert_eq!(loc.map(|l| l.region.as_str()), Some("us-east-1"));
    }

    #[test]
    fn topology_get_location_missing() {
        let topo = two_node_topology();
        assert!(topo.get_location(NodeId(99)).is_none());
    }

    #[test]
    fn topology_default_is_empty() {
        let topo = NetworkTopology::default();
        assert_eq!(topo.node_count(), 0);
    }

    #[test]
    fn internet_cost_is_highest() {
        let link = NetworkLink::from_type(LinkType::Internet);
        let one_gb = 1_073_741_824;
        let cost = link.transfer_cost(one_gb);
        assert!(cost > 0.08);
    }

    #[test]
    fn serialize_roundtrip_link() {
        let link = NetworkLink::from_type(LinkType::CrossDatacenter);
        let json = serde_json::to_string(&link).expect("serialization should succeed");
        let deser: NetworkLink =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(link.bandwidth, deser.bandwidth);
        assert_eq!(link.latency_us, deser.latency_us);
        assert_eq!(link.link_type, deser.link_type);
    }

    #[test]
    fn serialize_roundtrip_topology() {
        let topo = two_node_topology();
        let json = serde_json::to_string(&topo).expect("serialization should succeed");
        let deser: NetworkTopology =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(deser.node_count(), 2);
        assert_eq!(deser.link_count(), 2);
    }
}
