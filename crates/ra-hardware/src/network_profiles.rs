//! Predefined network topology profiles for common deployment patterns.
//!
//! Each profile creates a fully-configured [`NetworkTopology`] with
//! realistic bandwidth, latency, and cost parameters drawn from
//! published cloud provider pricing and network benchmarks.

use crate::network::{LinkType, Location, NetworkLink, NetworkTopology, NodeId};

impl NetworkTopology {
    /// Single datacenter cluster: 4 nodes on 2 racks with 10 Gbps links.
    ///
    /// Models a typical on-premises Hadoop/Spark cluster in a single DC.
    /// Intra-rack nodes share a top-of-rack switch at 100 Gbps.
    /// Cross-rack links run at 10 Gbps through the aggregate layer.
    #[must_use]
    pub fn single_datacenter_cluster() -> Self {
        let mut topo = Self::with_default_link(NetworkLink::from_type(LinkType::IntraDatacenter));

        // 4 nodes across 2 racks
        let n0 = NodeId(0);
        let n1 = NodeId(1);
        let n2 = NodeId(2);
        let n3 = NodeId(3);

        topo.add_node(n0, Location::with_rack("us-east-1", "dc1", "rack-1"));
        topo.add_node(n1, Location::with_rack("us-east-1", "dc1", "rack-1"));
        topo.add_node(n2, Location::with_rack("us-east-1", "dc1", "rack-2"));
        topo.add_node(n3, Location::with_rack("us-east-1", "dc1", "rack-2"));

        // Intra-rack: 100 Gbps, <1us
        let intra_rack = NetworkLink::from_type(LinkType::IntraRack);
        topo.add_link(n0, n1, intra_rack.clone());
        topo.add_link(n2, n3, intra_rack);

        // Cross-rack: 10 Gbps, 5us
        let cross_rack = NetworkLink::from_type(LinkType::IntraDatacenter);
        topo.add_link(n0, n2, cross_rack.clone());
        topo.add_link(n0, n3, cross_rack.clone());
        topo.add_link(n1, n2, cross_rack.clone());
        topo.add_link(n1, n3, cross_rack);

        topo
    }

    /// Multi-datacenter deployment: 3 DCs with 1 Gbps inter-DC links.
    ///
    /// Models a geo-replicated database (`CockroachDB`, `YugabyteDB`)
    /// spanning US-East, US-West, and EU-West. Each DC has 2 nodes
    /// connected at 10 Gbps. Cross-DC links cost $0.01/GB (AWS
    /// cross-AZ pricing).
    #[must_use]
    #[expect(clippy::cast_possible_truncation)]
    pub fn multi_datacenter() -> Self {
        let mut topo = Self::with_default_link(NetworkLink::from_type(LinkType::CrossDatacenter));

        let dcs = [
            ("us-east-1", "us-east-1a"),
            ("us-west-2", "us-west-2a"),
            ("eu-west-1", "eu-west-1a"),
        ];

        // 2 nodes per DC, 6 total (3 DCs, so index fits in u32)
        for (dc_idx, &(region, dc)) in dcs.iter().enumerate() {
            let base = (dc_idx * 2) as u32;
            let n0 = NodeId(base);
            let n1 = NodeId(base + 1);
            topo.add_node(n0, Location::new(region, dc));
            topo.add_node(n1, Location::new(region, dc));
            topo.add_link(n0, n1, NetworkLink::from_type(LinkType::IntraDatacenter));
        }

        // Cross-DC links: 1 Gbps, varying latency based on distance
        let cross_dc_links = [
            // US-East <-> US-West: ~60ms
            (0, 2, 60_000_u64),
            (0, 3, 60_000),
            (1, 2, 60_000),
            (1, 3, 60_000),
            // US-East <-> EU-West: ~80ms
            (0, 4, 80_000),
            (0, 5, 80_000),
            (1, 4, 80_000),
            (1, 5, 80_000),
            // US-West <-> EU-West: ~140ms
            (2, 4, 140_000),
            (2, 5, 140_000),
            (3, 4, 140_000),
            (3, 5, 140_000),
        ];

        for (from, to, latency_us) in cross_dc_links {
            let link = NetworkLink::new(
                125_000_000, // 1 Gbps
                latency_us,
                0.01, // AWS cross-AZ pricing
                LinkType::CrossDatacenter,
            );
            topo.add_link(NodeId(from), NodeId(to), link);
        }

        topo
    }

    /// Cloud federation: AWS + GCP + Azure with internet links.
    ///
    /// Models a federated query engine that queries across multiple
    /// cloud providers. Each provider has 2 nodes. Cross-cloud
    /// traffic traverses the public internet at higher cost.
    #[must_use]
    #[expect(clippy::cast_possible_truncation)]
    pub fn cloud_federation() -> Self {
        let mut topo = Self::with_default_link(NetworkLink::from_type(LinkType::Internet));

        let clouds = [
            ("aws-us-east-1", "aws-use1-az1"),
            ("gcp-us-central1", "gcp-usc1-a"),
            ("azure-eastus", "azure-eastus-1"),
        ];

        // 2 nodes per cloud, 6 total (3 clouds, so index fits in u32)
        for (cloud_idx, &(region, dc)) in clouds.iter().enumerate() {
            let base = (cloud_idx * 2) as u32;
            let n0 = NodeId(base);
            let n1 = NodeId(base + 1);
            topo.add_node(n0, Location::new(region, dc));
            topo.add_node(n1, Location::new(region, dc));
            // Intra-cloud: 10 Gbps, 1ms
            topo.add_link(
                n0,
                n1,
                NetworkLink::new(1_250_000_000, 1_000, 0.01, LinkType::IntraDatacenter),
            );
        }

        // Cross-cloud: 50 Mbps effective, 50-100ms, internet egress
        let cross_cloud = [
            // AWS <-> GCP: ~50ms
            (0, 2, 50_000_u64),
            (0, 3, 50_000),
            (1, 2, 50_000),
            (1, 3, 50_000),
            // AWS <-> Azure: ~40ms (both in US East)
            (0, 4, 40_000),
            (0, 5, 40_000),
            (1, 4, 40_000),
            (1, 5, 40_000),
            // GCP <-> Azure: ~60ms
            (2, 4, 60_000),
            (2, 5, 60_000),
            (3, 4, 60_000),
            (3, 5, 60_000),
        ];

        for (from, to, latency_us) in cross_cloud {
            let link = NetworkLink::new(
                6_250_000, // 50 Mbps
                latency_us,
                0.09, // Internet egress pricing
                LinkType::Internet,
            );
            topo.add_link(NodeId(from), NodeId(to), link);
        }

        topo
    }

    /// Edge + cloud: edge nodes with 4G/5G uplink to a central cloud.
    ///
    /// Models an `IoT` or CDN architecture where edge nodes have
    /// limited bandwidth (1-10 Mbps over 4G/5G) and high latency
    /// (100-500ms) to a cloud aggregation tier.
    #[must_use]
    pub fn edge_cloud() -> Self {
        let mut topo = Self::with_default_link(NetworkLink::from_type(LinkType::Internet));

        // 2 cloud nodes in a datacenter
        let cloud0 = NodeId(0);
        let cloud1 = NodeId(1);
        topo.add_node(cloud0, Location::new("us-east-1", "us-east-1a"));
        topo.add_node(cloud1, Location::new("us-east-1", "us-east-1a"));
        topo.add_link(
            cloud0,
            cloud1,
            NetworkLink::from_type(LinkType::IntraDatacenter),
        );

        // 4 edge nodes with varying connectivity
        let edge_configs = [
            // (node_id, region, dc, bandwidth_bps, latency_us)
            (2, "edge-nyc", "edge-nyc-1", 10_000_000_u64, 20_000_u64), // 5G: 10 Mbps, 20ms
            (3, "edge-chi", "edge-chi-1", 5_000_000, 40_000),          // 5G: 5 Mbps, 40ms
            (4, "edge-la", "edge-la-1", 2_000_000, 80_000),            // 4G: 2 Mbps, 80ms
            (5, "edge-rural", "edge-rural-1", 1_000_000, 200_000),     // 4G rural: 1 Mbps, 200ms
        ];

        for (id, region, dc, bw, lat) in edge_configs {
            let node = NodeId(id);
            topo.add_node(node, Location::new(region, dc));

            // Edge -> cloud links (asymmetric: upload slower)
            let uplink = NetworkLink::new(
                bw,
                lat,
                0.09, // Internet egress
                LinkType::Internet,
            );
            let downlink = NetworkLink::new(
                bw * 3, // Download typically 3x faster
                lat,
                0.0, // Ingress is free on most clouds
                LinkType::Internet,
            );

            // Edge uploads to cloud
            topo.add_directional_link(node, cloud0, uplink.clone());
            topo.add_directional_link(node, cloud1, uplink);

            // Cloud downloads to edge
            topo.add_directional_link(cloud0, node, downlink.clone());
            topo.add_directional_link(cloud1, node, downlink);
        }

        topo
    }

    /// Data warehouse: Snowflake-style compute-storage separation.
    ///
    /// Models a cloud data warehouse where compute nodes access
    /// remote storage (S3/GCS) through a high-bandwidth network.
    /// Compute nodes are co-located in the same DC with fast
    /// interconnects, while storage access has higher latency.
    #[must_use]
    pub fn data_warehouse() -> Self {
        let mut topo = Self::with_default_link(NetworkLink::from_type(LinkType::IntraDatacenter));

        // 4 compute nodes in same DC
        for i in 0..4 {
            topo.add_node(
                NodeId(i),
                Location::with_rack("us-east-1", "us-east-1a", format!("compute-rack-{}", i / 2)),
            );
        }

        // 2 storage nodes (representing S3/GCS endpoints)
        let storage0 = NodeId(4);
        let storage1 = NodeId(5);
        topo.add_node(storage0, Location::new("us-east-1", "s3-endpoint"));
        topo.add_node(storage1, Location::new("us-east-1", "s3-endpoint"));

        // Compute-to-compute: 10 Gbps, 5us
        for i in 0..4_u32 {
            for j in (i + 1)..4 {
                let link_type = if i / 2 == j / 2 {
                    LinkType::IntraRack
                } else {
                    LinkType::IntraDatacenter
                };
                topo.add_link(NodeId(i), NodeId(j), NetworkLink::from_type(link_type));
            }
        }

        // Compute-to-storage: 25 Gbps (S3 Express), 1ms latency
        // $0.0025/GB for S3 standard GET requests (amortized)
        let storage_link = NetworkLink::new(
            3_125_000_000, // 25 Gbps
            1_000,         // 1ms
            0.0025,        // S3 GET cost amortized per GB
            LinkType::IntraDatacenter,
        );

        for i in 0..4_u32 {
            topo.add_link(NodeId(i), storage0, storage_link.clone());
            topo.add_link(NodeId(i), storage1, storage_link.clone());
        }

        // Storage-to-storage: high bandwidth (same endpoint)
        topo.add_link(
            storage0,
            storage1,
            NetworkLink::from_type(LinkType::IntraRack),
        );

        topo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_dc_has_4_nodes() {
        let topo = NetworkTopology::single_datacenter_cluster();
        assert_eq!(topo.node_count(), 4);
    }

    #[test]
    fn single_dc_all_same_datacenter() {
        let topo = NetworkTopology::single_datacenter_cluster();
        for i in 0..4_u32 {
            for j in 0..4 {
                assert!(topo.same_datacenter(NodeId(i), NodeId(j)));
            }
        }
    }

    #[test]
    fn single_dc_same_rack_pairs() {
        let topo = NetworkTopology::single_datacenter_cluster();
        assert!(topo.same_rack(NodeId(0), NodeId(1)));
        assert!(topo.same_rack(NodeId(2), NodeId(3)));
        assert!(!topo.same_rack(NodeId(0), NodeId(2)));
    }

    #[test]
    fn single_dc_intra_rack_faster_than_cross_rack() {
        let topo = NetworkTopology::single_datacenter_cluster();
        let one_gb = 1_073_741_824;
        let intra = topo.transfer_time(NodeId(0), NodeId(1), one_gb);
        let cross = topo.transfer_time(NodeId(0), NodeId(2), one_gb);
        assert!(intra < cross);
    }

    #[test]
    fn single_dc_no_transfer_cost() {
        let topo = NetworkTopology::single_datacenter_cluster();
        let cost = topo.transfer_cost(NodeId(0), NodeId(1), 1_073_741_824);
        assert!(cost.abs() < f64::EPSILON);
    }

    #[test]
    fn multi_dc_has_6_nodes() {
        let topo = NetworkTopology::multi_datacenter();
        assert_eq!(topo.node_count(), 6);
    }

    #[test]
    fn multi_dc_has_3_datacenters() {
        let topo = NetworkTopology::multi_datacenter();
        assert_eq!(topo.datacenters().len(), 3);
    }

    #[test]
    fn multi_dc_has_3_regions() {
        let topo = NetworkTopology::multi_datacenter();
        assert_eq!(topo.regions().len(), 3);
    }

    #[test]
    fn multi_dc_intra_dc_same_datacenter() {
        let topo = NetworkTopology::multi_datacenter();
        assert!(topo.same_datacenter(NodeId(0), NodeId(1)));
        assert!(topo.same_datacenter(NodeId(2), NodeId(3)));
        assert!(topo.same_datacenter(NodeId(4), NodeId(5)));
    }

    #[test]
    fn multi_dc_cross_dc_different_datacenter() {
        let topo = NetworkTopology::multi_datacenter();
        assert!(!topo.same_datacenter(NodeId(0), NodeId(2)));
        assert!(!topo.same_datacenter(NodeId(0), NodeId(4)));
    }

    #[test]
    fn multi_dc_cross_dc_has_billing_cost() {
        let topo = NetworkTopology::multi_datacenter();
        let one_gb = 1_073_741_824;
        let cost = topo.transfer_cost(NodeId(0), NodeId(2), one_gb);
        assert!(cost > 0.0);
    }

    #[test]
    fn multi_dc_closer_dc_faster() {
        let topo = NetworkTopology::multi_datacenter();
        let bytes = 1_000_000;
        // US-East to US-West: 60ms latency
        let us_to_us = topo.transfer_time(NodeId(0), NodeId(2), bytes);
        // US-West to EU-West: 140ms latency
        let us_to_eu = topo.transfer_time(NodeId(2), NodeId(4), bytes);
        assert!(us_to_us < us_to_eu);
    }

    #[test]
    fn cloud_federation_has_6_nodes() {
        let topo = NetworkTopology::cloud_federation();
        assert_eq!(topo.node_count(), 6);
    }

    #[test]
    fn cloud_federation_intra_cloud_faster() {
        let topo = NetworkTopology::cloud_federation();
        let bytes = 10_000_000;
        let intra = topo.transfer_time(NodeId(0), NodeId(1), bytes);
        let inter = topo.transfer_time(NodeId(0), NodeId(2), bytes);
        assert!(intra < inter);
    }

    #[test]
    fn cloud_federation_cross_cloud_expensive() {
        let topo = NetworkTopology::cloud_federation();
        let one_gb = 1_073_741_824;
        let cross_cost = topo.transfer_cost(NodeId(0), NodeId(2), one_gb);
        let intra_cost = topo.transfer_cost(NodeId(0), NodeId(1), one_gb);
        assert!(cross_cost > intra_cost);
    }

    #[test]
    fn edge_cloud_has_6_nodes() {
        let topo = NetworkTopology::edge_cloud();
        assert_eq!(topo.node_count(), 6);
    }

    #[test]
    fn edge_cloud_asymmetric_bandwidth() {
        let topo = NetworkTopology::edge_cloud();
        // Upload (edge -> cloud) should be slower than download
        let edge = NodeId(2);
        let cloud = NodeId(0);
        let bytes = 100_000_000; // 100 MB
        let upload = topo.transfer_time(edge, cloud, bytes);
        let download = topo.transfer_time(cloud, edge, bytes);
        assert!(upload > download);
    }

    #[test]
    fn edge_cloud_rural_slower_than_city() {
        let topo = NetworkTopology::edge_cloud();
        let bytes = 10_000_000;
        let city = topo.transfer_time(NodeId(2), NodeId(0), bytes);
        let rural = topo.transfer_time(NodeId(5), NodeId(0), bytes);
        assert!(rural > city);
    }

    #[test]
    fn edge_cloud_upload_costs_money() {
        let topo = NetworkTopology::edge_cloud();
        let one_gb = 1_073_741_824;
        let cost = topo.transfer_cost(NodeId(2), NodeId(0), one_gb);
        assert!(cost > 0.0);
    }

    #[test]
    fn data_warehouse_has_6_nodes() {
        let topo = NetworkTopology::data_warehouse();
        assert_eq!(topo.node_count(), 6);
    }

    #[test]
    fn data_warehouse_compute_nodes_connected() {
        let topo = NetworkTopology::data_warehouse();
        for i in 0..4_u32 {
            for j in (i + 1)..4 {
                assert!(topo.get_explicit_link(NodeId(i), NodeId(j)).is_some());
            }
        }
    }

    #[test]
    fn data_warehouse_storage_accessible() {
        let topo = NetworkTopology::data_warehouse();
        for i in 0..4_u32 {
            assert!(topo.get_explicit_link(NodeId(i), NodeId(4)).is_some());
        }
    }

    #[test]
    fn data_warehouse_storage_has_cost() {
        let topo = NetworkTopology::data_warehouse();
        let one_gb = 1_073_741_824;
        let cost = topo.transfer_cost(NodeId(0), NodeId(4), one_gb);
        assert!(cost > 0.0);
    }

    #[test]
    fn data_warehouse_compute_to_compute_free() {
        let topo = NetworkTopology::data_warehouse();
        let one_gb = 1_073_741_824;
        let cost = topo.transfer_cost(NodeId(0), NodeId(1), one_gb);
        assert!(cost.abs() < f64::EPSILON);
    }

    #[test]
    fn data_warehouse_same_rack_faster() {
        let topo = NetworkTopology::data_warehouse();
        let bytes = 1_073_741_824;
        // Nodes 0,1 same rack; nodes 0,2 different rack
        let same_rack = topo.transfer_time(NodeId(0), NodeId(1), bytes);
        let diff_rack = topo.transfer_time(NodeId(0), NodeId(2), bytes);
        assert!(same_rack < diff_rack);
    }
}
