//! Federated cost model for estimating the cost of cross-database
//! query execution strategies.
//!
//! This module provides cost estimation for three strategies:
//! - Ship Query: send the query to a remote database
//! - Ship Data: fetch data from remote, execute locally
//! - Hybrid: push part of the query, fetch intermediate results
//!
//! When a [`NetworkCostModel`] is attached, the cost model uses
//! real network topology (latency, bandwidth per link) instead of
//! the flat estimates from [`RemoteConnection`].

use ra_core::algebra::RelExpr;
use ra_core::federated::{
    ExecutionLocation, FederatedCostBreakdown, FederatedQuery, RemoteConnection,
};
use ra_core::statistics::Statistics;

use crate::network_cost::NetworkCostModel;

/// Intermediate result from network transfer estimation.
struct NetworkTransferResult {
    transfer_ms: f64,
}

/// Cost model for federated query execution strategies.
#[derive(Debug, Clone)]
pub struct FederatedCostModel {
    /// Cost per CPU operation (arbitrary units per row).
    pub cpu_cost_per_row: f64,
    /// Cost per IO operation (arbitrary units per page).
    pub io_cost_per_page: f64,
    /// Default page size in bytes.
    pub page_size: u64,
    /// Overhead multiplier for remote execution vs local.
    pub remote_execution_overhead: f64,
    /// Selectivity estimate when statistics are unavailable.
    pub default_filter_selectivity: f64,
    /// Default row count when statistics are unavailable.
    pub default_row_count: f64,
    /// Default average row size when statistics are unavailable.
    pub default_avg_row_size: u64,
    /// Optional network topology model for accurate transfer costs.
    network_model: Option<NetworkCostModel>,
}

impl Default for FederatedCostModel {
    fn default() -> Self {
        Self {
            cpu_cost_per_row: 0.01,
            io_cost_per_page: 1.0,
            page_size: 8192,
            remote_execution_overhead: 1.2,
            default_filter_selectivity: 0.1,
            default_row_count: 100_000.0,
            default_avg_row_size: 200,
            network_model: None,
        }
    }
}

impl FederatedCostModel {
    /// Create a cost model with default parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a network cost model for topology-aware transfer
    /// estimates.
    ///
    /// When set, transfer cost calculations use real link bandwidth
    /// and latency from the topology instead of the flat estimates
    /// in [`RemoteConnection`].
    #[must_use]
    pub fn with_network_model(mut self, model: NetworkCostModel) -> Self {
        self.network_model = Some(model);
        self
    }

    /// Return the attached network cost model, if any.
    #[must_use]
    pub fn network_model(&self) -> Option<&NetworkCostModel> {
        self.network_model.as_ref()
    }

    /// Estimate network transfer time for `bytes` using the network
    /// topology model if available, otherwise fall back to the flat
    /// estimate from `connection`.
    fn network_transfer_ms(
        &self,
        connection: &RemoteConnection,
        table: &str,
        bytes: u64,
    ) -> NetworkTransferResult {
        if let Some(net) = &self.network_model {
            if let Some(source_node) = net.node_for_table(table) {
                // Transfer to local node (node 0 by convention)
                let local_node = ra_hardware::network::NodeId(0);
                // Use row_width=1 so rows*width = bytes
                let est = net.node_transfer_cost(source_node, local_node, bytes, 1);
                return NetworkTransferResult {
                    transfer_ms: est.transfer_time.as_secs_f64() * 1000.0,
                };
            }
        }
        // Fall back to flat estimate
        NetworkTransferResult {
            transfer_ms: connection.transfer_time_ms(bytes),
        }
    }

    /// Estimate cost of shipping the entire query to a remote.
    ///
    /// Cost = connection latency + remote execution time +
    ///        result transfer time.
    #[must_use]
    pub fn estimate_ship_query(
        &self,
        connection: &RemoteConnection,
        stats: Option<&Statistics>,
        result_rows: f64,
        result_row_size: u64,
    ) -> FederatedCostBreakdown {
        self.estimate_ship_query_for_table(connection, stats, result_rows, result_row_size, "")
    }

    /// Estimate cost of shipping a query to a remote, with
    /// topology-aware network costs when a table name is provided.
    #[must_use]
    pub fn estimate_ship_query_for_table(
        &self,
        connection: &RemoteConnection,
        stats: Option<&Statistics>,
        result_rows: f64,
        result_row_size: u64,
        table: &str,
    ) -> FederatedCostBreakdown {
        let row_count = stats.map_or(self.default_row_count, |s| s.row_count);
        let avg_row_size = stats.map_or(self.default_avg_row_size, |s| s.avg_row_size);

        // Remote execution: scan + process
        let pages = (row_count * avg_row_size as f64) / self.page_size as f64;
        let remote_exec_ms = (pages * self.io_cost_per_page + row_count * self.cpu_cost_per_row)
            * self.remote_execution_overhead;

        // Result transfer
        let result_bytes = (result_rows * result_row_size as f64) as u64;

        let transfer = self.network_transfer_ms(connection, table, result_bytes);

        let total_ms = remote_exec_ms + transfer.transfer_ms;

        let rows_transferred = result_rows as u64;

        FederatedCostBreakdown {
            strategy: "ship_query".into(),
            remote_exec_ms,
            network_transfer_ms: transfer.transfer_ms,
            transfer_bytes: result_bytes,
            local_exec_ms: 0.0,
            total_ms,
            rows_transferred,
        }
    }

    /// Estimate cost of fetching data from remote and executing
    /// locally.
    ///
    /// Cost = remote scan + data transfer + local execution.
    #[must_use]
    pub fn estimate_ship_data(
        &self,
        connection: &RemoteConnection,
        stats: Option<&Statistics>,
        has_filter: bool,
    ) -> FederatedCostBreakdown {
        self.estimate_ship_data_for_table(connection, stats, has_filter, "")
    }

    /// Estimate cost of fetching data from remote with
    /// topology-aware network costs.
    #[must_use]
    pub fn estimate_ship_data_for_table(
        &self,
        connection: &RemoteConnection,
        stats: Option<&Statistics>,
        has_filter: bool,
        table: &str,
    ) -> FederatedCostBreakdown {
        let row_count = stats.map_or(self.default_row_count, |s| s.row_count);
        let avg_row_size = stats.map_or(self.default_avg_row_size, |s| s.avg_row_size);

        // How many rows actually get transferred
        let transfer_rows = if has_filter {
            row_count * self.default_filter_selectivity
        } else {
            row_count
        };

        // Remote scan cost (even for ship-data, remote scans)
        let pages = (row_count * avg_row_size as f64) / self.page_size as f64;
        let remote_exec_ms = pages * self.io_cost_per_page * self.remote_execution_overhead;

        // Transfer cost
        let transfer_bytes = (transfer_rows * avg_row_size as f64) as u64;

        let transfer = self.network_transfer_ms(connection, table, transfer_bytes);

        // Local execution cost
        let local_exec_ms = transfer_rows * self.cpu_cost_per_row;

        let total_ms = remote_exec_ms + transfer.transfer_ms + local_exec_ms;

        let rows_transferred = transfer_rows as u64;

        FederatedCostBreakdown {
            strategy: if has_filter {
                "ship_data_filtered".into()
            } else {
                "ship_data_full".into()
            },
            remote_exec_ms,
            network_transfer_ms: transfer.transfer_ms,
            transfer_bytes,
            local_exec_ms,
            total_ms,
            rows_transferred,
        }
    }

    /// Estimate cost of a hybrid strategy where part of the query
    /// is pushed to the remote.
    ///
    /// The hybrid strategy pushes filters and possibly aggregations
    /// to the remote, fetches intermediate results, then finishes
    /// execution locally.
    #[must_use]
    pub fn estimate_hybrid(
        &self,
        connection: &RemoteConnection,
        stats: Option<&Statistics>,
        pushdown_selectivity: f64,
        local_complexity_factor: f64,
    ) -> FederatedCostBreakdown {
        self.estimate_hybrid_for_table(
            connection,
            stats,
            pushdown_selectivity,
            local_complexity_factor,
            "",
        )
    }

    /// Estimate hybrid cost with topology-aware network costs.
    #[must_use]
    pub fn estimate_hybrid_for_table(
        &self,
        connection: &RemoteConnection,
        stats: Option<&Statistics>,
        pushdown_selectivity: f64,
        local_complexity_factor: f64,
        table: &str,
    ) -> FederatedCostBreakdown {
        let row_count = stats.map_or(self.default_row_count, |s| s.row_count);
        let avg_row_size = stats.map_or(self.default_avg_row_size, |s| s.avg_row_size);

        // Remote pushdown execution
        let pages = (row_count * avg_row_size as f64) / self.page_size as f64;
        let remote_exec_ms = (pages * self.io_cost_per_page
            + row_count * self.cpu_cost_per_row * pushdown_selectivity)
            * self.remote_execution_overhead;

        // Intermediate result transfer
        let intermediate_rows = row_count * pushdown_selectivity;
        let transfer_bytes = (intermediate_rows * avg_row_size as f64) as u64;

        let transfer = self.network_transfer_ms(connection, table, transfer_bytes);

        // Local operations on intermediate results
        let local_exec_ms = intermediate_rows * self.cpu_cost_per_row * local_complexity_factor;

        let total_ms = remote_exec_ms + transfer.transfer_ms + local_exec_ms;

        let rows_transferred = intermediate_rows as u64;

        FederatedCostBreakdown {
            strategy: "hybrid".into(),
            remote_exec_ms,
            network_transfer_ms: transfer.transfer_ms,
            transfer_bytes,
            local_exec_ms,
            total_ms,
            rows_transferred,
        }
    }

    /// Estimate the cost of local-only execution (no remote
    /// involvement).
    #[must_use]
    pub fn estimate_local(&self, stats: Option<&Statistics>) -> FederatedCostBreakdown {
        let row_count = stats.map_or(self.default_row_count, |s| s.row_count);
        let avg_row_size = stats.map_or(self.default_avg_row_size, |s| s.avg_row_size);

        let pages = (row_count * avg_row_size as f64) / self.page_size as f64;
        let local_exec_ms = pages * self.io_cost_per_page + row_count * self.cpu_cost_per_row;

        FederatedCostBreakdown {
            strategy: "local".into(),
            remote_exec_ms: 0.0,
            network_transfer_ms: 0.0,
            transfer_bytes: 0,
            local_exec_ms,
            total_ms: local_exec_ms,
            rows_transferred: 0,
        }
    }

    /// Estimate the cost of a given execution location strategy.
    #[must_use]
    pub fn estimate_location(
        &self,
        location: &ExecutionLocation,
        query: &FederatedQuery,
    ) -> FederatedCostBreakdown {
        match location {
            ExecutionLocation::ShipQuery {
                target, query: q, ..
            } => {
                let table = Self::first_table(q);
                let stats = self.best_stats(query);
                let row_count = stats.map_or(self.default_row_count, |s| s.row_count);
                let avg_row_size = stats.map_or(self.default_avg_row_size, |s| s.avg_row_size);
                // Assume result is 10% of source for full queries
                let result_rows = row_count * 0.1;
                self.estimate_ship_query_for_table(target, stats, result_rows, avg_row_size, &table)
            }
            ExecutionLocation::ShipData {
                source,
                table,
                predicate,
            } => {
                let stats = self.best_stats(query);
                self.estimate_ship_data_for_table(source, stats, predicate.is_some(), table)
            }
            ExecutionLocation::Hybrid { target, .. } => {
                let table = Self::first_remote_table(query);
                let stats = self.best_stats(query);
                self.estimate_hybrid_for_table(
                    target,
                    stats,
                    self.default_filter_selectivity,
                    2.0,
                    &table,
                )
            }
            ExecutionLocation::Local { .. } => {
                let stats = self.best_stats(query);
                self.estimate_local(stats)
            }
        }
    }

    /// Extract the first table name from a relational expression.
    fn first_table(expr: &RelExpr) -> String {
        match expr {
            RelExpr::Scan { table, .. } => table.clone(),
            RelExpr::Filter { input, .. }
            | RelExpr::Project { input, .. }
            | RelExpr::Aggregate { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Window { input, .. }
            | RelExpr::Distinct { input, .. } => Self::first_table(input),
            RelExpr::Join { left, .. }
            | RelExpr::Union { left, .. }
            | RelExpr::Intersect { left, .. }
            | RelExpr::Except { left, .. } => Self::first_table(left),
            _ => String::new(),
        }
    }

    /// Find the first remote table name in a federated query.
    fn first_remote_table(query: &FederatedQuery) -> String {
        for (name, source) in &query.sources {
            if source.is_remote() {
                return name.clone();
            }
        }
        String::new()
    }

    /// Extract the best available statistics from the query.
    fn best_stats<'a>(&self, query: &'a FederatedQuery) -> Option<&'a Statistics> {
        let _ = self; // used for future expansion
        for source in query.sources.values() {
            if let Some(stats) = source.statistics() {
                return Some(stats);
            }
        }
        None
    }

    /// Estimate row count for a relational expression given a
    /// source table's statistics.
    #[must_use]
    pub fn estimate_output_rows(&self, expr: &RelExpr, stats: Option<&Statistics>) -> f64 {
        let base_rows = stats.map_or(self.default_row_count, |s| s.row_count);

        match expr {
            RelExpr::Scan { .. } => base_rows,
            RelExpr::Filter { .. } => base_rows * self.default_filter_selectivity,
            RelExpr::Project { input, .. } => self.estimate_output_rows(input, stats),
            RelExpr::Aggregate { .. } => {
                // Aggregation typically reduces rows significantly
                (base_rows * 0.01).max(1.0)
            }
            RelExpr::Limit { count, .. } => {
                let limit = *count as f64;
                base_rows.min(limit)
            }
            RelExpr::Distinct { input, .. } => self.estimate_output_rows(input, stats) * 0.8,
            RelExpr::Join { left, right, .. } => {
                let left_rows = self.estimate_output_rows(left, stats);
                let right_rows = self.estimate_output_rows(right, stats);
                // Assume 10% selectivity for joins
                left_rows * right_rows * 0.1
            }
            _ => base_rows,
        }
    }

    /// Estimate total data size in bytes for a relation.
    #[must_use]
    pub fn estimate_data_size(&self, stats: Option<&Statistics>) -> u64 {
        let row_count = stats.map_or(self.default_row_count, |s| s.row_count);
        let avg_row_size = stats.map_or(self.default_avg_row_size, |s| s.avg_row_size);
        let size = (row_count * avg_row_size as f64) as u64;
        size
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ra_core::federated::{DataSource, DatabaseType, QueryCapabilities};

    use super::*;

    fn sample_connection() -> RemoteConnection {
        RemoteConnection::new(DatabaseType::PostgreSQL, "db.example.com:5432", 10, 100)
    }

    fn sample_stats() -> Statistics {
        let mut stats = Statistics::new(1_000_000.0);
        stats.avg_row_size = 200;
        stats.total_size = 200_000_000;
        stats
    }

    fn sample_query() -> FederatedQuery {
        let mut sources = HashMap::new();
        sources.insert(
            "orders".into(),
            DataSource::remote(
                sample_connection(),
                "orders",
                Some(sample_stats()),
                QueryCapabilities::full(),
            ),
        );
        FederatedQuery::new(RelExpr::scan("orders"), sources)
    }

    #[test]
    fn ship_query_cost_estimation() {
        let model = FederatedCostModel::new();
        let conn = sample_connection();
        let stats = sample_stats();

        let cost = model.estimate_ship_query(&conn, Some(&stats), 10_000.0, 200);

        assert!(cost.remote_exec_ms > 0.0);
        assert!(cost.network_transfer_ms > 0.0);
        assert_eq!(cost.transfer_bytes, 2_000_000);
        assert_eq!(cost.local_exec_ms, 0.0);
        assert!(cost.total_ms > 0.0);
        assert_eq!(cost.strategy, "ship_query");
    }

    #[test]
    fn ship_data_full_cost() {
        let model = FederatedCostModel::new();
        let conn = sample_connection();
        let stats = sample_stats();

        let cost = model.estimate_ship_data(&conn, Some(&stats), false);

        assert!(cost.remote_exec_ms > 0.0);
        assert!(cost.network_transfer_ms > 0.0);
        assert!(cost.local_exec_ms > 0.0);
        assert_eq!(cost.strategy, "ship_data_full");
        // Full scan: all rows transferred
        assert_eq!(cost.rows_transferred, 1_000_000);
    }

    #[test]
    fn ship_data_filtered_cheaper() {
        let model = FederatedCostModel::new();
        let conn = sample_connection();
        let stats = sample_stats();

        let full = model.estimate_ship_data(&conn, Some(&stats), false);
        let filtered = model.estimate_ship_data(&conn, Some(&stats), true);

        assert!(filtered.total_ms < full.total_ms);
        assert!(filtered.transfer_bytes < full.transfer_bytes);
        assert_eq!(filtered.strategy, "ship_data_filtered");
    }

    #[test]
    fn hybrid_cost_estimation() {
        let model = FederatedCostModel::new();
        let conn = sample_connection();
        let stats = sample_stats();

        let cost = model.estimate_hybrid(&conn, Some(&stats), 0.01, 2.0);

        assert!(cost.remote_exec_ms > 0.0);
        assert!(cost.network_transfer_ms > 0.0);
        assert!(cost.local_exec_ms > 0.0);
        assert_eq!(cost.strategy, "hybrid");
        // 1% selectivity on 1M rows = 10K rows
        assert_eq!(cost.rows_transferred, 10_000);
    }

    #[test]
    fn hybrid_cheaper_than_full_data_ship() {
        let model = FederatedCostModel::new();
        let conn = sample_connection();
        let stats = sample_stats();

        let full = model.estimate_ship_data(&conn, Some(&stats), false);
        let hybrid = model.estimate_hybrid(&conn, Some(&stats), 0.01, 2.0);

        assert!(hybrid.total_ms < full.total_ms);
        assert!(hybrid.transfer_bytes < full.transfer_bytes);
    }

    #[test]
    fn local_cost_no_network() {
        let model = FederatedCostModel::new();
        let stats = sample_stats();

        let cost = model.estimate_local(Some(&stats));

        assert_eq!(cost.remote_exec_ms, 0.0);
        assert_eq!(cost.network_transfer_ms, 0.0);
        assert_eq!(cost.transfer_bytes, 0);
        assert!(cost.local_exec_ms > 0.0);
        assert_eq!(cost.strategy, "local");
    }

    #[test]
    fn default_stats_when_none_available() {
        let model = FederatedCostModel::new();
        let conn = sample_connection();

        let cost = model.estimate_ship_data(&conn, None, false);

        assert!(cost.total_ms > 0.0);
        assert!(cost.rows_transferred > 0);
    }

    #[test]
    fn estimate_location_ship_query() {
        let model = FederatedCostModel::new();
        let query = sample_query();
        let location = ExecutionLocation::ShipQuery {
            target: sample_connection(),
            query: RelExpr::scan("orders"),
        };

        let cost = model.estimate_location(&location, &query);
        assert_eq!(cost.strategy, "ship_query");
        assert!(cost.total_ms > 0.0);
    }

    #[test]
    fn estimate_location_ship_data() {
        let model = FederatedCostModel::new();
        let query = sample_query();
        let location = ExecutionLocation::ShipData {
            source: sample_connection(),
            table: "orders".into(),
            predicate: None,
        };

        let cost = model.estimate_location(&location, &query);
        assert_eq!(cost.strategy, "ship_data_full");
    }

    #[test]
    fn estimate_location_hybrid() {
        let model = FederatedCostModel::new();
        let query = sample_query();
        let location = ExecutionLocation::Hybrid {
            remote_subquery: Box::new(RelExpr::scan("orders")),
            local_operations: Box::new(RelExpr::scan("orders")),
            target: sample_connection(),
        };

        let cost = model.estimate_location(&location, &query);
        assert_eq!(cost.strategy, "hybrid");
    }

    #[test]
    fn estimate_location_local() {
        let model = FederatedCostModel::new();
        let query = sample_query();
        let location = ExecutionLocation::Local {
            query: RelExpr::scan("orders"),
        };

        let cost = model.estimate_location(&location, &query);
        assert_eq!(cost.strategy, "local");
        assert_eq!(cost.network_transfer_ms, 0.0);
    }

    #[test]
    fn estimate_output_rows_scan() {
        let model = FederatedCostModel::new();
        let stats = sample_stats();
        let rows = model.estimate_output_rows(&RelExpr::scan("t"), Some(&stats));
        assert!((rows - 1_000_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_output_rows_filter() {
        let model = FederatedCostModel::new();
        let stats = sample_stats();
        let expr = RelExpr::Filter {
            predicate: ra_core::expr::Expr::Const(ra_core::expr::Const::Bool(true)),
            input: Box::new(RelExpr::scan("t")),
        };
        let rows = model.estimate_output_rows(&expr, Some(&stats));
        assert!((rows - 100_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_output_rows_limit() {
        let model = FederatedCostModel::new();
        let stats = sample_stats();
        let expr = RelExpr::Limit {
            count: 100,
            offset: 0,
            input: Box::new(RelExpr::scan("t")),
        };
        let rows = model.estimate_output_rows(&expr, Some(&stats));
        assert!((rows - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_output_rows_aggregate() {
        let model = FederatedCostModel::new();
        let stats = sample_stats();
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        let rows = model.estimate_output_rows(&expr, Some(&stats));
        assert!(rows < 100_000.0);
    }

    #[test]
    fn estimate_data_size() {
        let model = FederatedCostModel::new();
        let stats = sample_stats();
        let size = model.estimate_data_size(Some(&stats));
        // 1M rows * 200 bytes = 200MB
        assert_eq!(size, 200_000_000);
    }

    #[test]
    fn estimate_data_size_no_stats() {
        let model = FederatedCostModel::new();
        let size = model.estimate_data_size(None);
        // 100K rows * 200 bytes = 20MB
        assert_eq!(size, 20_000_000);
    }
}
