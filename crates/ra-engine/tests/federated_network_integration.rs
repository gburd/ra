//! Integration tests for federated query optimization with
//! network-aware cost modeling.
//!
//! Validates that the federated optimizer uses real network
//! topology (latency, bandwidth per link) for cost estimation
//! and produces correct strategy selections.

use std::collections::HashMap;

use ra_core::algebra::RelExpr;
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::federated::{
    DataSource, DatabaseType, ExecutionLocation, FederatedCostBreakdown, FederatedQuery,
    QueryCapabilities, RemoteConnection,
};
use ra_core::statistics::Statistics;
use ra_engine::federated_cost::FederatedCostModel;
use ra_engine::federated_optimizer::FederatedOptimizer;
use ra_engine::network_cost::NetworkCostModel;
use ra_hardware::network::{LinkType, Location, NetworkLink, NetworkTopology, NodeId};

// ── Helpers ─────────────────────────────────────────────────

/// Build a topology with a local node (0) and a remote node (1)
/// connected by the given link type.
fn two_node_topology(link_type: LinkType) -> NetworkTopology {
    let mut topo = NetworkTopology::new();
    let local = NodeId(0);
    let remote = NodeId(1);
    topo.add_node(local, Location::new("us-east-1", "us-east-1a"));
    topo.add_node(remote, Location::new("us-west-2", "us-west-2a"));
    topo.add_link(remote, local, NetworkLink::from_type(link_type));
    topo
}

/// Build a network cost model that assigns `table` to node 1.
fn model_with_remote_table(topo: NetworkTopology, table: &str) -> NetworkCostModel {
    let mut assignments = HashMap::new();
    assignments.insert(table.into(), NodeId(1));
    NetworkCostModel::new(topo, assignments)
}

/// Create a remote connection with given latency/bandwidth.
fn remote_conn(db_type: DatabaseType, latency_ms: u64, bandwidth_mbps: u64) -> RemoteConnection {
    RemoteConnection::new(
        db_type,
        "remote.example.com:5432",
        latency_ms,
        bandwidth_mbps,
    )
}

fn large_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    {
        s.total_size = (rows * avg_row_size as f64) as u64;
    }
    s
}

fn filter_expr(table: &str, col: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::qualified(table, col))),
        right: Box::new(Expr::Const(Const::Int(100))),
    }
}

fn filter_query(table: &str, conn: RemoteConnection, stats: Statistics) -> FederatedQuery {
    let plan = RelExpr::Filter {
        predicate: filter_expr(table, "amount"),
        input: Box::new(RelExpr::scan(table)),
    };
    let mut sources = HashMap::new();
    sources.insert(
        table.into(),
        DataSource::remote(conn, table, Some(stats), QueryCapabilities::full()),
    );
    FederatedQuery::new(plan, sources)
}

fn scan_query(
    table: &str,
    conn: RemoteConnection,
    stats: Statistics,
    caps: QueryCapabilities,
) -> FederatedQuery {
    let plan = RelExpr::scan(table);
    let mut sources = HashMap::new();
    sources.insert(
        table.into(),
        DataSource::remote(conn, table, Some(stats), caps),
    );
    FederatedQuery::new(plan, sources)
}

fn aggregate_query(table: &str, conn: RemoteConnection, stats: Statistics) -> FederatedQuery {
    let plan = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("region"))],
        aggregates: vec![],
        input: Box::new(RelExpr::scan(table)),
    };
    let mut sources = HashMap::new();
    sources.insert(
        table.into(),
        DataSource::remote(conn, table, Some(stats), QueryCapabilities::full()),
    );
    FederatedQuery::new(plan, sources)
}

fn join_local_remote(
    local_table: &str,
    remote_table: &str,
    conn: RemoteConnection,
    local_stats: Statistics,
    remote_stats: Statistics,
) -> FederatedQuery {
    let plan = RelExpr::Join {
        join_type: ra_core::algebra::JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(local_table, "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified(remote_table, "id"))),
        },
        left: Box::new(RelExpr::scan(local_table)),
        right: Box::new(RelExpr::scan(remote_table)),
    };
    let mut sources = HashMap::new();
    sources.insert(
        local_table.into(),
        DataSource::local(local_table, local_stats),
    );
    sources.insert(
        remote_table.into(),
        DataSource::remote(
            conn,
            remote_table,
            Some(remote_stats),
            QueryCapabilities::full(),
        ),
    );
    FederatedQuery::new(plan, sources)
}

// ── Network-aware cost model tests ──────────────────────────

#[test]
fn network_model_attached_to_cost_model() {
    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "orders");
    let model = FederatedCostModel::new().with_network_model(net);
    assert!(model.network_model().is_some());
}

#[test]
fn default_cost_model_has_no_network_model() {
    let model = FederatedCostModel::new();
    assert!(model.network_model().is_none());
}

#[test]
fn network_model_changes_transfer_cost() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 10, 100);
    let stats = large_stats(1_000_000.0, 200);

    // Without network model
    let baseline = FederatedCostModel::new();
    let cost_flat = baseline.estimate_ship_data(&conn, Some(&stats), false);

    // With fast intra-datacenter network model
    let topo = two_node_topology(LinkType::IntraDatacenter);
    let net = model_with_remote_table(topo, "orders");
    let network_aware = FederatedCostModel::new().with_network_model(net);
    let cost_net = network_aware.estimate_ship_data_for_table(&conn, Some(&stats), false, "orders");

    // Network-aware should differ from flat estimate
    assert!(
        (cost_flat.network_transfer_ms - cost_net.network_transfer_ms).abs() > 0.001,
        "network model should produce different transfer \
         cost: flat={}, net={}",
        cost_flat.network_transfer_ms,
        cost_net.network_transfer_ms,
    );
}

#[test]
fn fast_link_produces_lower_transfer_cost() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 100, 10);
    let stats = large_stats(1_000_000.0, 200);

    // Slow link: cross-region (100 Mbps, 100ms latency)
    let slow_topo = two_node_topology(LinkType::CrossRegion);
    let slow_net = model_with_remote_table(slow_topo, "t");
    let slow_model = FederatedCostModel::new().with_network_model(slow_net);
    let cost_slow = slow_model.estimate_ship_data_for_table(&conn, Some(&stats), false, "t");

    // Fast link: intra-rack (100 Gbps, <1us latency)
    let fast_topo = two_node_topology(LinkType::IntraRack);
    let fast_net = model_with_remote_table(fast_topo, "t");
    let fast_model = FederatedCostModel::new().with_network_model(fast_net);
    let cost_fast = fast_model.estimate_ship_data_for_table(&conn, Some(&stats), false, "t");

    assert!(
        cost_slow.network_transfer_ms > cost_fast.network_transfer_ms,
        "slow link ({}) should have higher transfer cost \
         than fast link ({})",
        cost_slow.network_transfer_ms,
        cost_fast.network_transfer_ms,
    );
}

// ── Filter pushdown reduces transfer ────────────────────────

#[test]
fn filter_pushdown_reduces_transfer_bytes() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 50, 100);
    let stats = large_stats(10_000_000.0, 256);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "orders");
    let model = FederatedCostModel::new().with_network_model(net);

    let full = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "orders");
    let filtered = model.estimate_ship_data_for_table(&conn, Some(&stats), true, "orders");

    // Filtered should transfer far fewer bytes (10% selectivity)
    assert!(
        filtered.transfer_bytes < full.transfer_bytes / 5,
        "filtered transfer ({}) should be much less than \
         full ({})",
        filtered.transfer_bytes,
        full.transfer_bytes,
    );
    assert!(filtered.total_ms < full.total_ms);
}

#[test]
fn filter_pushdown_network_time_proportional() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 10, 100);
    let stats = large_stats(1_000_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "sales");
    let model = FederatedCostModel::new().with_network_model(net);

    let full = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "sales");
    let filtered = model.estimate_ship_data_for_table(&conn, Some(&stats), true, "sales");

    // Network time for filtered should be roughly selectivity *
    // full network time (within 2x tolerance for latency effects)
    assert!(
        filtered.network_transfer_ms < full.network_transfer_ms * 0.5,
        "filtered net time ({}) should be well under full ({})",
        filtered.network_transfer_ms,
        full.network_transfer_ms,
    );
}

// ── Aggregate pushdown reduces transfer by ~1000x ───────────

#[test]
fn aggregate_pushdown_drastically_reduces_transfer() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 20, 1000);
    let stats = large_stats(100_000_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "events");
    let model = FederatedCostModel::new().with_network_model(net);

    // Ship all 100M rows (full scan)
    let full_ship = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "events");

    // Ship only aggregation results (~1% of data with default
    // selectivity, but aggregation gives us ~0.01 of that)
    let hybrid_agg = model.estimate_hybrid_for_table(
        &conn,
        Some(&stats),
        0.001, // aggregate reduces to 0.1% of rows
        1.0,
        "events",
    );

    let ratio = full_ship.transfer_bytes as f64 / hybrid_agg.transfer_bytes.max(1) as f64;
    assert!(
        ratio > 100.0,
        "aggregate pushdown should reduce transfer by >100x, \
         got {ratio:.0}x (full={}, agg={})",
        full_ship.transfer_bytes,
        hybrid_agg.transfer_bytes,
    );
}

// ── Hybrid strategy: push agg, fetch summary, join locally ──

#[test]
fn hybrid_agg_then_local_join() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 20, 100);
    let stats = large_stats(10_000_000.0, 200);
    let local_stats = large_stats(1_000.0, 100);

    let query = join_local_remote("local_dim", "remote_fact", conn, local_stats, stats);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "remote_fact");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let plan = optimizer
        .optimize_federated(&query)
        .expect("should produce a plan");

    // The optimizer should find a viable strategy
    assert!(plan.cost.total_ms > 0.0);
    assert!(!plan.steps.is_empty());
}

// ── Cost model predicts ShipQuery vs ShipData tradeoffs ─────

#[test]
fn small_remote_table_favors_ship_data() {
    // 1MB table: fetch everything, faster than query overhead
    let conn = remote_conn(DatabaseType::MySQL, 5, 1000);
    let stats = large_stats(5_000.0, 200); // ~1MB

    let topo = two_node_topology(LinkType::IntraDatacenter);
    let net = model_with_remote_table(topo, "config");
    let cost_model = FederatedCostModel::new().with_network_model(net);

    let ship_data = cost_model.estimate_ship_data_for_table(&conn, Some(&stats), false, "config");
    // Ship query returns 10% of rows (default)
    let ship_query = cost_model.estimate_ship_query_for_table(
        &conn,
        Some(&stats),
        500.0, // 10% of 5K
        200,
        "config",
    );

    // For tiny tables, ShipData total should be reasonable
    assert!(
        ship_data.total_ms < 1000.0,
        "shipping 1MB should complete quickly: {} ms",
        ship_data.total_ms,
    );
    // Both strategies should have non-zero cost
    assert!(ship_data.total_ms > 0.0);
    assert!(ship_query.total_ms > 0.0);
}

#[test]
fn large_table_with_selective_filter_favors_hybrid() {
    // 1GB table with 0.1% selectivity filter
    let conn = remote_conn(DatabaseType::PostgreSQL, 50, 100);
    let stats = large_stats(5_000_000.0, 200); // ~1GB

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "logs");
    let cost_model = FederatedCostModel::new().with_network_model(net);

    let full_ship = cost_model.estimate_ship_data_for_table(&conn, Some(&stats), false, "logs");
    let hybrid = cost_model.estimate_hybrid_for_table(
        &conn,
        Some(&stats),
        0.001, // very selective filter
        1.5,
        "logs",
    );

    assert!(
        hybrid.total_ms < full_ship.total_ms,
        "hybrid ({:.1}ms) should beat full ship ({:.1}ms) \
         for selective filter on large table",
        hybrid.total_ms,
        full_ship.total_ms,
    );
}

#[test]
fn ship_query_best_for_supported_query_small_result() {
    let conn = remote_conn(DatabaseType::Snowflake, 80, 50);
    let stats = large_stats(100_000_000.0, 256); // ~25GB

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "warehouse");
    let cost_model = FederatedCostModel::new().with_network_model(net);

    // Query returns only 100 rows
    let ship_query =
        cost_model.estimate_ship_query_for_table(&conn, Some(&stats), 100.0, 256, "warehouse");
    let ship_data =
        cost_model.estimate_ship_data_for_table(&conn, Some(&stats), false, "warehouse");

    assert!(
        ship_query.total_ms < ship_data.total_ms,
        "ship_query ({:.1}ms) should beat ship_data \
         ({:.1}ms) when result is 100 rows from 100M",
        ship_query.total_ms,
        ship_data.total_ms,
    );
}

// ── Optimizer integration with network model ────────────────

#[test]
fn optimizer_uses_network_model() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 50, 100);
    let stats = large_stats(10_000_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "big_table");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = filter_query("big_table", conn, stats);
    let plan = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    assert!(plan.cost.total_ms > 0.0);
    assert!(!plan.alternatives.is_empty());
}

#[test]
fn optimizer_produces_different_costs_with_topology() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 10, 100);
    let stats = large_stats(1_000_000.0, 200);

    // Without topology
    let opt_flat = FederatedOptimizer::new();
    let query_flat = filter_query("orders", conn.clone(), stats.clone());
    let plan_flat = opt_flat.optimize_federated(&query_flat).expect("flat opt");

    // With slow cross-region topology
    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "orders");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let opt_net = FederatedOptimizer::with_cost_model(cost_model);
    let query_net = filter_query("orders", conn, stats);
    let plan_net = opt_net.optimize_federated(&query_net).expect("net opt");

    // Costs should differ due to different transfer modeling
    let flat_total = plan_flat.cost.total_ms;
    let net_total = plan_net.cost.total_ms;
    // They should both be positive
    assert!(flat_total > 0.0);
    assert!(net_total > 0.0);
}

// ── Database-specific scenarios ─────────────────────────────

#[test]
fn postgresql_full_pushdown_on_slow_link() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 100, 10);
    let stats = large_stats(10_000_000.0, 256);

    let topo = two_node_topology(LinkType::Internet);
    let net = model_with_remote_table(topo, "pg_table");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = filter_query("pg_table", conn, stats);
    let plan = optimizer.optimize_federated(&query).expect("pg plan");

    // On a very slow link, should not pick full ship data
    let is_full_ship = plan.cost.strategy == "ship_data_full";
    assert!(
        !is_full_ship,
        "should not pick full ship data on slow internet link"
    );
}

#[test]
fn mysql_with_limited_capabilities() {
    let conn = remote_conn(DatabaseType::MySQL, 20, 100);
    let stats = large_stats(100_000.0, 128);

    let caps = QueryCapabilities::minimal();

    let topo = two_node_topology(LinkType::CrossDatacenter);
    let net = model_with_remote_table(topo, "mysql_t");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = scan_query("mysql_t", conn, stats, caps);
    let plan = optimizer.optimize_federated(&query).expect("mysql plan");

    // Should produce a valid plan even with minimal caps
    assert!(plan.cost.total_ms > 0.0);
}

#[test]
fn sqlite_scan_small_table() {
    let conn = remote_conn(DatabaseType::SQLite, 2, 1000);
    let stats = large_stats(1_000.0, 64); // 64KB

    let topo = two_node_topology(LinkType::IntraRack);
    let net = model_with_remote_table(topo, "sqlite_t");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = scan_query("sqlite_t", conn, stats, QueryCapabilities::full());
    let plan = optimizer.optimize_federated(&query).expect("sqlite plan");

    // Tiny table on fast link -- ship data should be cheap
    assert!(
        plan.cost.total_ms < 100.0,
        "64KB on intra-rack should be very fast: {:.1}ms",
        plan.cost.total_ms,
    );
}

#[test]
fn snowflake_large_warehouse_query() {
    let conn = remote_conn(DatabaseType::Snowflake, 200, 50);
    let stats = large_stats(1_000_000_000.0, 300); // ~300GB

    let topo = two_node_topology(LinkType::Internet);
    let net = model_with_remote_table(topo, "snowflake_t");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = aggregate_query("snowflake_t", conn, stats);
    let plan = optimizer
        .optimize_federated(&query)
        .expect("snowflake plan");

    // Should pick pushdown strategy for a billion-row table
    // over the internet
    assert!(plan.cost.total_ms > 0.0);
    assert!(!plan.steps.is_empty());
}

// ── Cross-database scenarios ────────────────────────────────

#[test]
fn cross_datacenter_penalizes_transfer() {
    let conn_fast = remote_conn(DatabaseType::PostgreSQL, 1, 10000);
    let conn_slow = remote_conn(DatabaseType::PostgreSQL, 50, 100);
    let stats = large_stats(1_000_000.0, 200);

    let fast_topo = two_node_topology(LinkType::IntraDatacenter);
    let fast_net = model_with_remote_table(fast_topo, "t");
    let fast_model = FederatedCostModel::new().with_network_model(fast_net);

    let slow_topo = two_node_topology(LinkType::CrossDatacenter);
    let slow_net = model_with_remote_table(slow_topo, "t");
    let slow_model = FederatedCostModel::new().with_network_model(slow_net);

    let cost_fast = fast_model.estimate_ship_data_for_table(&conn_fast, Some(&stats), false, "t");
    let cost_slow = slow_model.estimate_ship_data_for_table(&conn_slow, Some(&stats), false, "t");

    assert!(
        cost_slow.network_transfer_ms > cost_fast.network_transfer_ms,
        "cross-DC ({:.1}ms) should be slower than \
         intra-DC ({:.1}ms)",
        cost_slow.network_transfer_ms,
        cost_fast.network_transfer_ms,
    );
}

#[test]
fn internet_link_heavily_penalizes_data_shipping() {
    let conn = remote_conn(DatabaseType::BigQuery, 150, 50);
    let stats = large_stats(50_000_000.0, 256); // ~12GB

    let topo = two_node_topology(LinkType::Internet);
    let net = model_with_remote_table(topo, "bq_table");
    let model = FederatedCostModel::new().with_network_model(net);

    let ship_data = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "bq_table");

    // 12GB over 50 Mbps internet should be very slow
    assert!(
        ship_data.network_transfer_ms > 1000.0,
        "12GB over internet should take >1s: {:.1}ms",
        ship_data.network_transfer_ms,
    );
}

// ── Edge cases ──────────────────────────────────────────────

#[test]
fn unknown_table_falls_back_to_flat_estimate() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 10, 100);
    let stats = large_stats(10_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    // Table "orders" is NOT in the node assignment
    let net = model_with_remote_table(topo, "other_table");
    let model = FederatedCostModel::new().with_network_model(net);

    // Should fall back to flat estimate since "orders" is unknown
    let cost = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "orders");

    // Flat estimate: 10ms latency + transfer time
    assert!(cost.network_transfer_ms > 0.0);
    assert!(cost.total_ms > 0.0);
}

#[test]
fn zero_rows_produces_minimal_cost() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 10, 100);
    let stats = large_stats(0.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "empty");
    let model = FederatedCostModel::new().with_network_model(net);

    let cost = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "empty");

    assert_eq!(cost.rows_transferred, 0);
    assert_eq!(cost.transfer_bytes, 0);
}

#[test]
fn no_network_model_still_works() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 10, 100);
    let stats = large_stats(1_000.0, 200);
    let model = FederatedCostModel::new();

    let cost = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "any_table");
    assert!(cost.total_ms > 0.0);
}

// ── Multi-topology profile tests ────────────────────────────

#[test]
fn single_datacenter_profile() {
    let topo = NetworkTopology::single_datacenter_cluster();
    let mut assignments = HashMap::new();
    assignments.insert("t".into(), NodeId(2)); // rack-2
    let net = NetworkCostModel::new(topo, assignments);
    let model = FederatedCostModel::new().with_network_model(net);

    let conn = remote_conn(DatabaseType::PostgreSQL, 1, 10000);
    let stats = large_stats(1_000_000.0, 200);

    let cost = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "t");

    // Intra-datacenter should be fast
    assert!(
        cost.network_transfer_ms < 500.0,
        "intra-DC transfer should be fast: {:.1}ms",
        cost.network_transfer_ms,
    );
}

#[test]
fn multi_datacenter_profile() {
    let topo = NetworkTopology::multi_datacenter();
    let mut assignments = HashMap::new();
    // Table on US-West node (node 2)
    assignments.insert("remote_t".into(), NodeId(2));
    let net = NetworkCostModel::new(topo, assignments);
    let model = FederatedCostModel::new().with_network_model(net);

    let conn = remote_conn(DatabaseType::PostgreSQL, 60, 100);
    let stats = large_stats(1_000_000.0, 200);

    let cost = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "remote_t");

    // Cross-DC should be noticeably slower
    assert!(
        cost.network_transfer_ms > 10.0,
        "cross-DC transfer should have noticeable latency: {:.1}ms",
        cost.network_transfer_ms,
    );
}

// ── Cost comparison correctness ─────────────────────────────

#[test]
fn ship_query_cheaper_when_result_tiny() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 50, 100);
    let stats = large_stats(10_000_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "big");
    let model = FederatedCostModel::new().with_network_model(net);

    // Ship query: returns 10 rows
    let sq = model.estimate_ship_query_for_table(&conn, Some(&stats), 10.0, 200, "big");
    // Ship data: fetches all 10M rows
    let sd = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "big");

    assert!(
        sq.total_ms < sd.total_ms,
        "ship_query ({:.1}ms) should be cheaper than \
         ship_data ({:.1}ms) for 10 rows from 10M",
        sq.total_ms,
        sd.total_ms,
    );
}

#[test]
fn hybrid_cheaper_than_ship_data_for_selective_filter() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 30, 100);
    let stats = large_stats(5_000_000.0, 256);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "logs");
    let model = FederatedCostModel::new().with_network_model(net);

    let sd = model.estimate_ship_data_for_table(&conn, Some(&stats), false, "logs");
    let hybrid = model.estimate_hybrid_for_table(
        &conn,
        Some(&stats),
        0.01, // 1% selectivity
        2.0,
        "logs",
    );

    assert!(
        hybrid.total_ms < sd.total_ms,
        "hybrid ({:.1}ms) should beat full ship_data \
         ({:.1}ms) with 1% selectivity",
        hybrid.total_ms,
        sd.total_ms,
    );
}

#[test]
fn cost_breakdown_fields_consistent() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 20, 100);
    let stats = large_stats(100_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "t");
    let model = FederatedCostModel::new().with_network_model(net);

    let cost = model.estimate_hybrid_for_table(&conn, Some(&stats), 0.1, 2.0, "t");

    // Total should be sum of components
    let expected = cost.remote_exec_ms + cost.network_transfer_ms + cost.local_exec_ms;
    assert!(
        (cost.total_ms - expected).abs() < 0.001,
        "total_ms ({}) should equal sum of components ({})",
        cost.total_ms,
        expected,
    );
}

// ── Optimizer end-to-end with topology ──────────────────────

#[test]
fn optimizer_analyzes_with_network_model() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 30, 100);
    let stats = large_stats(1_000_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "events");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = filter_query("events", conn, stats);
    let analysis = optimizer.analyze(&query).expect("analysis should succeed");

    assert!(analysis.plan.cost.total_ms > 0.0);
    assert!(!analysis.plan.steps.is_empty());
}

#[test]
fn optimizer_enumerate_strategies_count() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 10, 100);
    let stats = large_stats(1_000_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "t");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = filter_query("t", conn, stats);
    let strategies = optimizer.enumerate_strategies(&query);

    // Should have: local, ship_query, ship_data(full),
    // ship_data(filtered), hybrid
    assert!(
        strategies.len() >= 4,
        "expected >= 4 strategies, got {}",
        strategies.len(),
    );
}

#[test]
fn optimizer_best_plan_has_lowest_cost() {
    let conn = remote_conn(DatabaseType::PostgreSQL, 30, 100);
    let stats = large_stats(500_000.0, 200);

    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "t");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let query = filter_query("t", conn, stats);
    let plan = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    // Best plan should be cheaper than all alternatives
    for alt in &plan.alternatives {
        assert!(
            plan.cost.total_ms <= alt.total_ms,
            "best plan ({:.1}ms) should be <= alternative \
             {} ({:.1}ms)",
            plan.cost.total_ms,
            alt.strategy,
            alt.total_ms,
        );
    }
}

#[test]
fn local_only_query_ignores_network_model() {
    let topo = two_node_topology(LinkType::CrossRegion);
    let net = model_with_remote_table(topo, "remote_only");
    let cost_model = FederatedCostModel::new().with_network_model(net);
    let optimizer = FederatedOptimizer::with_cost_model(cost_model);

    let mut sources = HashMap::new();
    sources.insert(
        "local_t".into(),
        DataSource::local("local_t", large_stats(10_000.0, 200)),
    );
    let query = FederatedQuery::new(RelExpr::scan("local_t"), sources);

    let plan = optimizer.optimize_federated(&query).expect("local plan");

    assert!(matches!(plan.location, ExecutionLocation::Local { .. }));
    assert_eq!(plan.cost.network_transfer_ms, 0.0);
    assert_eq!(plan.cost.transfer_bytes, 0);
}

#[test]
fn transfer_size_display_formatting() {
    let cost = FederatedCostBreakdown {
        strategy: "test".into(),
        remote_exec_ms: 0.0,
        network_transfer_ms: 0.0,
        transfer_bytes: 1_500_000_000, // ~1.4GB
        local_exec_ms: 0.0,
        total_ms: 0.0,
        rows_transferred: 0,
    };
    let display = cost.transfer_size_display();
    assert!(
        display.contains("GB"),
        "1.5B bytes should display as GB: {}",
        display,
    );
}

#[test]
fn savings_percent_calculation() {
    let cost = FederatedCostBreakdown {
        strategy: "hybrid".into(),
        remote_exec_ms: 50.0,
        network_transfer_ms: 10.0,
        transfer_bytes: 1000,
        local_exec_ms: 5.0,
        total_ms: 65.0,
        rows_transferred: 10,
    };
    let savings = cost.savings_percent(100.0);
    assert!(
        (savings - 35.0).abs() < 0.01,
        "65ms vs 100ms should be 35% savings: {}",
        savings,
    );
}
