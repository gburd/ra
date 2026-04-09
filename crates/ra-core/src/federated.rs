//! Federated query types for cross-database query optimization.
//!
//! This module models queries that span multiple data sources,
//! including remote databases with varying capabilities, network
//! characteristics, and cost profiles.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::algebra::RelExpr;
use crate::expr::Expr;
use crate::statistics::Statistics;

/// A query that may reference both local and remote data sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FederatedQuery {
    /// The query plan (relational algebra expression).
    pub plan: RelExpr,
    /// Data sources referenced by table name.
    pub sources: HashMap<String, DataSource>,
}

impl FederatedQuery {
    /// Create a new federated query with the given plan and sources.
    #[must_use]
    pub fn new(
        plan: RelExpr,
        sources: HashMap<String, DataSource>,
    ) -> Self {
        Self { plan, sources }
    }

    /// Return all remote sources in this query.
    #[must_use]
    pub fn remote_sources(&self) -> Vec<(&str, &RemoteConnection)> {
        let mut remotes = Vec::new();
        for (name, source) in &self.sources {
            if let DataSource::Remote { connection, .. } = source {
                remotes.push((name.as_str(), connection));
            }
        }
        remotes
    }

    /// Return all local sources in this query.
    #[must_use]
    pub fn local_sources(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter_map(|(name, source)| match source {
                DataSource::Local { .. } => Some(name.as_str()),
                DataSource::Remote { .. } => None,
            })
            .collect()
    }

    /// Check whether this query involves any remote sources.
    #[must_use]
    pub fn is_distributed(&self) -> bool {
        self.sources
            .values()
            .any(|s| matches!(s, DataSource::Remote { .. }))
    }

    /// Return the number of distinct remote endpoints.
    #[must_use]
    pub fn remote_endpoint_count(&self) -> usize {
        let mut endpoints: Vec<&str> = Vec::new();
        for source in self.sources.values() {
            if let DataSource::Remote { connection, .. } = source {
                if !endpoints.contains(&connection.endpoint.as_str())
                {
                    endpoints.push(&connection.endpoint);
                }
            }
        }
        endpoints.len()
    }
}

/// A data source in a federated query: either local or remote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataSource {
    /// A table stored in the local database engine.
    Local {
        /// Table name.
        table: String,
        /// Statistics for the local table.
        statistics: Statistics,
    },
    /// A table stored on a remote database.
    Remote {
        /// Connection information for the remote database.
        connection: RemoteConnection,
        /// Remote table name.
        table: String,
        /// Statistics, if available from the remote source.
        statistics: Option<Statistics>,
        /// What query operations the remote database supports.
        capabilities: QueryCapabilities,
    },
}

impl DataSource {
    /// Create a local data source.
    #[must_use]
    pub fn local(
        table: impl Into<String>,
        statistics: Statistics,
    ) -> Self {
        Self::Local {
            table: table.into(),
            statistics,
        }
    }

    /// Create a remote data source.
    #[must_use]
    pub fn remote(
        connection: RemoteConnection,
        table: impl Into<String>,
        statistics: Option<Statistics>,
        capabilities: QueryCapabilities,
    ) -> Self {
        Self::Remote {
            connection,
            table: table.into(),
            statistics,
            capabilities,
        }
    }

    /// Return statistics if available.
    #[must_use]
    pub fn statistics(&self) -> Option<&Statistics> {
        match self {
            Self::Local { statistics, .. } => Some(statistics),
            Self::Remote { statistics, .. } => statistics.as_ref(),
        }
    }

    /// Check if this source is remote.
    #[must_use]
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }

    /// Return capabilities (local sources have full capabilities).
    #[must_use]
    pub fn capabilities(&self) -> QueryCapabilities {
        match self {
            Self::Local { .. } => QueryCapabilities::full(),
            Self::Remote { capabilities, .. } => capabilities.clone(),
        }
    }
}

/// Connection details for a remote database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConnection {
    /// The type of remote database.
    pub database_type: DatabaseType,
    /// Connection endpoint (e.g., hostname:port or URI).
    pub endpoint: String,
    /// Estimated round-trip latency in milliseconds.
    pub latency_ms: u64,
    /// Estimated available bandwidth in megabits per second.
    pub bandwidth_mbps: u64,
}

impl RemoteConnection {
    /// Create a new remote connection.
    #[must_use]
    pub fn new(
        database_type: DatabaseType,
        endpoint: impl Into<String>,
        latency_ms: u64,
        bandwidth_mbps: u64,
    ) -> Self {
        Self {
            database_type,
            endpoint: endpoint.into(),
            latency_ms,
            bandwidth_mbps,
        }
    }

    /// Estimate time to transfer `bytes` over this connection.
    ///
    /// Returns estimated transfer time in milliseconds.
    #[must_use]
    pub fn transfer_time_ms(&self, bytes: u64) -> f64 {
        if self.bandwidth_mbps == 0 {
            return f64::MAX;
        }
        // Convert bandwidth from Mbps to bytes per millisecond
        // 1 Mbps = 125_000 bytes/sec = 125 bytes/ms
        let bytes_per_ms = self.bandwidth_mbps as f64 * 125.0;
        let transfer_ms = bytes as f64 / bytes_per_ms;
        let latency = self.latency_ms as f64;
        latency + transfer_ms
    }
}

/// Supported remote database types.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum DatabaseType {
    /// `PostgreSQL`.
    PostgreSQL,
    /// `MySQL` / `MariaDB`.
    MySQL,
    /// `SQLite` (e.g., via network-attached storage).
    SQLite,
    /// Snowflake cloud data warehouse.
    Snowflake,
    /// Google `BigQuery`.
    BigQuery,
    /// Apache Spark SQL.
    SparkSQL,
    /// `DuckDB` (embedded analytical database).
    DuckDB,
    /// Generic JDBC-compatible source.
    GenericJdbc,
}

impl fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::PostgreSQL => "PostgreSQL",
            Self::MySQL => "MySQL",
            Self::SQLite => "SQLite",
            Self::Snowflake => "Snowflake",
            Self::BigQuery => "BigQuery",
            Self::SparkSQL => "SparkSQL",
            Self::DuckDB => "DuckDB",
            Self::GenericJdbc => "GenericJDBC",
        };
        write!(f, "{name}")
    }
}

impl DatabaseType {
    /// Return default capabilities for this database type.
    #[must_use]
    pub fn default_capabilities(self) -> QueryCapabilities {
        let extra_functions = self.extra_functions();
        match self {
            Self::GenericJdbc => QueryCapabilities {
                supports_join_pushdown: false,
                supports_aggregate_pushdown: false,
                supports_window_pushdown: false,
                supports_subquery: false,
                max_query_complexity: Some(50),
                ..QueryCapabilities::base_with(extra_functions)
            },
            _ => QueryCapabilities::full_with(extra_functions),
        }
    }

    /// Database-specific functions beyond the standard set.
    fn extra_functions(self) -> Vec<String> {
        match self {
            Self::PostgreSQL => vec![
                "STDDEV".into(),
                "VARIANCE".into(),
                "STRING_AGG".into(),
                "ARRAY_AGG".into(),
            ],
            Self::MySQL | Self::SQLite => {
                vec!["GROUP_CONCAT".into()]
            }
            Self::Snowflake | Self::BigQuery => vec![
                "STDDEV".into(),
                "VARIANCE".into(),
                "APPROX_COUNT_DISTINCT".into(),
            ],
            Self::SparkSQL => vec![
                "COLLECT_LIST".into(),
                "COLLECT_SET".into(),
            ],
            Self::DuckDB => vec![
                "STRING_AGG".into(),
                "LIST".into(),
                "APPROX_COUNT_DISTINCT".into(),
            ],
            Self::GenericJdbc => Vec::new(),
        }
    }
}

/// Describes which query operations a remote database can handle.
#[expect(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryCapabilities {
    /// Can push down filter (WHERE) clauses.
    pub supports_filter_pushdown: bool,
    /// Can push down column projections (SELECT list).
    pub supports_project_pushdown: bool,
    /// Can push down join operations.
    pub supports_join_pushdown: bool,
    /// Can push down GROUP BY / aggregation.
    pub supports_aggregate_pushdown: bool,
    /// Can push down window functions.
    pub supports_window_pushdown: bool,
    /// Can push down ORDER BY.
    pub supports_sort_pushdown: bool,
    /// Can push down LIMIT/OFFSET.
    pub supports_limit_pushdown: bool,
    /// Can handle subqueries in pushdown.
    pub supports_subquery: bool,
    /// Names of supported aggregate/scalar functions.
    pub supported_functions: Vec<String>,
    /// Maximum query complexity score the remote can handle.
    /// `None` means unlimited.
    pub max_query_complexity: Option<u32>,
}

impl QueryCapabilities {
    /// Standard aggregate functions supported by most databases.
    fn standard_functions() -> Vec<String> {
        vec![
            "COUNT".into(),
            "SUM".into(),
            "AVG".into(),
            "MIN".into(),
            "MAX".into(),
        ]
    }

    /// Full capabilities (everything supported).
    #[must_use]
    pub fn full() -> Self {
        Self::full_with(Vec::new())
    }

    /// Full capabilities with additional functions.
    #[must_use]
    pub fn full_with(extra_functions: Vec<String>) -> Self {
        let mut funcs = Self::standard_functions();
        funcs.extend(extra_functions);
        Self {
            supports_filter_pushdown: true,
            supports_project_pushdown: true,
            supports_join_pushdown: true,
            supports_aggregate_pushdown: true,
            supports_window_pushdown: true,
            supports_sort_pushdown: true,
            supports_limit_pushdown: true,
            supports_subquery: true,
            supported_functions: funcs,
            max_query_complexity: None,
        }
    }

    /// Base capabilities (filter + project + sort + limit) with
    /// additional functions.
    #[must_use]
    pub fn base_with(extra_functions: Vec<String>) -> Self {
        let mut funcs = Self::standard_functions();
        funcs.extend(extra_functions);
        Self {
            supports_filter_pushdown: true,
            supports_project_pushdown: true,
            supports_join_pushdown: false,
            supports_aggregate_pushdown: false,
            supports_window_pushdown: false,
            supports_sort_pushdown: true,
            supports_limit_pushdown: true,
            supports_subquery: false,
            supported_functions: funcs,
            max_query_complexity: None,
        }
    }

    /// Minimal capabilities (only filter and project pushdown).
    #[must_use]
    pub fn minimal() -> Self {
        Self {
            supports_filter_pushdown: true,
            supports_project_pushdown: true,
            supports_join_pushdown: false,
            supports_aggregate_pushdown: false,
            supports_window_pushdown: false,
            supports_sort_pushdown: false,
            supports_limit_pushdown: false,
            supports_subquery: false,
            supported_functions: Vec::new(),
            max_query_complexity: Some(10),
        }
    }

    /// Check whether a specific function is supported.
    #[must_use]
    pub fn supports_function(&self, name: &str) -> bool {
        self.supported_functions
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name))
    }

    /// Count how many pushdown types are supported.
    #[must_use]
    pub fn pushdown_count(&self) -> u32 {
        let mut count = 0u32;
        if self.supports_filter_pushdown {
            count += 1;
        }
        if self.supports_project_pushdown {
            count += 1;
        }
        if self.supports_join_pushdown {
            count += 1;
        }
        if self.supports_aggregate_pushdown {
            count += 1;
        }
        if self.supports_window_pushdown {
            count += 1;
        }
        if self.supports_sort_pushdown {
            count += 1;
        }
        if self.supports_limit_pushdown {
            count += 1;
        }
        count
    }
}

impl Default for QueryCapabilities {
    fn default() -> Self {
        Self::full()
    }
}

/// Where to execute (parts of) a federated query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionLocation {
    /// Execute the entire query on the remote database, then
    /// transfer the result back.
    ShipQuery {
        /// Which remote to execute on.
        target: RemoteConnection,
        /// The full query to ship.
        query: RelExpr,
    },

    /// Fetch all (optionally filtered) data from the remote,
    /// then execute locally.
    ShipData {
        /// Where to fetch data from.
        source: RemoteConnection,
        /// Table to fetch.
        table: String,
        /// Optional filter predicate to apply at the remote
        /// before shipping (reduces transfer).
        predicate: Option<Expr>,
    },

    /// Push down part of the query remotely, fetch intermediate
    /// results, then finish execution locally.
    Hybrid {
        /// The subquery to push down to the remote.
        remote_subquery: RelExpr,
        /// The local operations on the intermediate results.
        local_operations: RelExpr,
        /// Which remote to push the subquery to.
        target: RemoteConnection,
    },

    /// Execute locally only (no remote involvement).
    Local {
        /// The query to execute locally.
        query: RelExpr,
    },
}

impl fmt::Display for ExecutionLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShipQuery { target, .. } => {
                write!(
                    f,
                    "SHIP_QUERY to {} ({})",
                    target.endpoint, target.database_type
                )
            }
            Self::ShipData {
                source, table, predicate, ..
            } => {
                let filter_note = if predicate.is_some() {
                    " (with filter pushdown)"
                } else {
                    " (full scan)"
                };
                write!(
                    f,
                    "SHIP_DATA from {}.{}{filter_note}",
                    source.endpoint, table
                )
            }
            Self::Hybrid { target, .. } => {
                write!(
                    f,
                    "HYBRID via {} ({})",
                    target.endpoint, target.database_type
                )
            }
            Self::Local { .. } => write!(f, "LOCAL"),
        }
    }
}

/// Cost breakdown for a federated execution strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FederatedCostBreakdown {
    /// The chosen strategy.
    pub strategy: String,
    /// Remote execution cost in milliseconds.
    pub remote_exec_ms: f64,
    /// Network transfer cost in milliseconds.
    pub network_transfer_ms: f64,
    /// Data volume transferred in bytes.
    pub transfer_bytes: u64,
    /// Local execution cost in milliseconds.
    pub local_exec_ms: f64,
    /// Total estimated cost in milliseconds.
    pub total_ms: f64,
    /// Rows transferred over the network.
    pub rows_transferred: u64,
}

impl FederatedCostBreakdown {
    /// Format the transfer size in human-readable form.
    #[must_use]
    pub fn transfer_size_display(&self) -> String {
        format_bytes(self.transfer_bytes)
    }

    /// Calculate savings percentage compared to an alternative cost.
    #[must_use]
    pub fn savings_percent(&self, alternative_total_ms: f64) -> f64 {
        if alternative_total_ms <= 0.0 {
            return 0.0;
        }
        ((alternative_total_ms - self.total_ms)
            / alternative_total_ms)
            * 100.0
    }
}

/// Describes a complete federated execution plan with cost analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FederatedPlan {
    /// The chosen execution location/strategy.
    pub location: ExecutionLocation,
    /// Cost breakdown for the chosen strategy.
    pub cost: FederatedCostBreakdown,
    /// Alternative strategies that were considered.
    pub alternatives: Vec<FederatedCostBreakdown>,
    /// Steps in the execution plan (human-readable).
    pub steps: Vec<String>,
}

impl FederatedPlan {
    /// Return the best alternative (cheapest non-chosen strategy).
    #[must_use]
    pub fn best_alternative(&self) -> Option<&FederatedCostBreakdown>
    {
        self.alternatives
            .iter()
            .min_by(|a, b| {
                a.total_ms
                    .partial_cmp(&b.total_ms)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

/// Format a byte count in human-readable form.
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    let bytes_f = bytes as f64;
    if bytes >= 1_073_741_824 {
        format!("{:.1}GB", bytes_f / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes_f / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes_f / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "exact float equality needed for deterministic network transfer tests")]
mod tests {
    use super::*;

    fn sample_connection() -> RemoteConnection {
        RemoteConnection::new(
            DatabaseType::PostgreSQL,
            "db.example.com:5432",
            10,
            100,
        )
    }

    fn sample_stats() -> Statistics {
        let mut stats = Statistics::new(10_000.0);
        stats.avg_row_size = 200;
        stats.total_size = 2_000_000;
        stats
    }

    #[test]
    fn federated_query_is_distributed() {
        let mut sources = HashMap::new();
        sources.insert(
            "local_table".into(),
            DataSource::local("local_table", sample_stats()),
        );
        sources.insert(
            "remote_table".into(),
            DataSource::remote(
                sample_connection(),
                "remote_table",
                Some(sample_stats()),
                QueryCapabilities::full(),
            ),
        );

        let query = FederatedQuery::new(
            RelExpr::scan("local_table"),
            sources,
        );
        assert!(query.is_distributed());
    }

    #[test]
    fn federated_query_not_distributed() {
        let mut sources = HashMap::new();
        sources.insert(
            "t".into(),
            DataSource::local("t", sample_stats()),
        );
        let query =
            FederatedQuery::new(RelExpr::scan("t"), sources);
        assert!(!query.is_distributed());
    }

    #[test]
    fn remote_sources_listed() {
        let mut sources = HashMap::new();
        sources.insert(
            "local".into(),
            DataSource::local("local", sample_stats()),
        );
        sources.insert(
            "remote".into(),
            DataSource::remote(
                sample_connection(),
                "remote",
                None,
                QueryCapabilities::minimal(),
            ),
        );
        let query =
            FederatedQuery::new(RelExpr::scan("local"), sources);
        let remotes = query.remote_sources();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].0, "remote");
    }

    #[test]
    fn local_sources_listed() {
        let mut sources = HashMap::new();
        sources.insert(
            "a".into(),
            DataSource::local("a", sample_stats()),
        );
        sources.insert(
            "b".into(),
            DataSource::local("b", sample_stats()),
        );
        let query =
            FederatedQuery::new(RelExpr::scan("a"), sources);
        let locals = query.local_sources();
        assert_eq!(locals.len(), 2);
    }

    #[test]
    fn remote_endpoint_count() {
        let mut sources = HashMap::new();
        sources.insert(
            "r1".into(),
            DataSource::remote(
                RemoteConnection::new(
                    DatabaseType::PostgreSQL,
                    "host1:5432",
                    5,
                    100,
                ),
                "r1",
                None,
                QueryCapabilities::full(),
            ),
        );
        sources.insert(
            "r2".into(),
            DataSource::remote(
                RemoteConnection::new(
                    DatabaseType::MySQL,
                    "host2:3306",
                    10,
                    50,
                ),
                "r2",
                None,
                QueryCapabilities::full(),
            ),
        );
        sources.insert(
            "r3".into(),
            DataSource::remote(
                RemoteConnection::new(
                    DatabaseType::PostgreSQL,
                    "host1:5432",
                    5,
                    100,
                ),
                "r3",
                None,
                QueryCapabilities::full(),
            ),
        );
        let query =
            FederatedQuery::new(RelExpr::scan("r1"), sources);
        assert_eq!(query.remote_endpoint_count(), 2);
    }

    #[test]
    fn transfer_time_calculation() {
        let conn = RemoteConnection::new(
            DatabaseType::PostgreSQL,
            "host:5432",
            10,
            // 100 Mbps = 12_500 bytes/ms
            100,
        );
        // 12.5 MB = 12_500_000 bytes
        // Transfer: 12_500_000 / 12_500 = 1000ms
        // Plus latency: 10ms
        // Total: 1010ms
        let time = conn.transfer_time_ms(12_500_000);
        assert!((time - 1010.0).abs() < 1.0);
    }

    #[test]
    fn transfer_time_zero_bandwidth() {
        let conn = RemoteConnection::new(
            DatabaseType::MySQL,
            "host:3306",
            5,
            0,
        );
        assert_eq!(conn.transfer_time_ms(1000), f64::MAX);
    }

    #[test]
    fn database_type_display() {
        assert_eq!(DatabaseType::PostgreSQL.to_string(), "PostgreSQL");
        assert_eq!(DatabaseType::MySQL.to_string(), "MySQL");
        assert_eq!(DatabaseType::SQLite.to_string(), "SQLite");
        assert_eq!(DatabaseType::Snowflake.to_string(), "Snowflake");
        assert_eq!(DatabaseType::BigQuery.to_string(), "BigQuery");
        assert_eq!(DatabaseType::SparkSQL.to_string(), "SparkSQL");
        assert_eq!(DatabaseType::DuckDB.to_string(), "DuckDB");
        assert_eq!(
            DatabaseType::GenericJdbc.to_string(),
            "GenericJDBC"
        );
    }

    #[test]
    fn default_capabilities_postgresql() {
        let caps = DatabaseType::PostgreSQL.default_capabilities();
        assert!(caps.supports_filter_pushdown);
        assert!(caps.supports_join_pushdown);
        assert!(caps.supports_aggregate_pushdown);
        assert!(caps.supports_function("COUNT"));
        assert!(caps.supports_function("stddev"));
    }

    #[test]
    fn default_capabilities_generic_jdbc() {
        let caps = DatabaseType::GenericJdbc.default_capabilities();
        assert!(caps.supports_filter_pushdown);
        assert!(!caps.supports_join_pushdown);
        assert!(!caps.supports_aggregate_pushdown);
        assert_eq!(caps.max_query_complexity, Some(50));
    }

    #[test]
    fn query_capabilities_full() {
        let caps = QueryCapabilities::full();
        assert_eq!(caps.pushdown_count(), 7);
    }

    #[test]
    fn query_capabilities_minimal() {
        let caps = QueryCapabilities::minimal();
        assert_eq!(caps.pushdown_count(), 2);
        assert!(caps.supports_filter_pushdown);
        assert!(!caps.supports_join_pushdown);
    }

    #[test]
    fn data_source_statistics() {
        let local =
            DataSource::local("t", Statistics::new(100.0));
        assert!(local.statistics().is_some());

        let remote = DataSource::remote(
            sample_connection(),
            "t",
            None,
            QueryCapabilities::full(),
        );
        assert!(remote.statistics().is_none());
    }

    #[test]
    fn data_source_capabilities() {
        let local =
            DataSource::local("t", Statistics::new(100.0));
        let caps = local.capabilities();
        assert!(caps.supports_join_pushdown);

        let remote = DataSource::remote(
            sample_connection(),
            "t",
            None,
            QueryCapabilities::minimal(),
        );
        let caps = remote.capabilities();
        assert!(!caps.supports_join_pushdown);
    }

    #[test]
    fn execution_location_display() {
        let ship_query = ExecutionLocation::ShipQuery {
            target: sample_connection(),
            query: RelExpr::scan("t"),
        };
        let display = format!("{ship_query}");
        assert!(display.contains("SHIP_QUERY"));
        assert!(display.contains("db.example.com"));

        let ship_data = ExecutionLocation::ShipData {
            source: sample_connection(),
            table: "orders".into(),
            predicate: None,
        };
        let display = format!("{ship_data}");
        assert!(display.contains("SHIP_DATA"));
        assert!(display.contains("full scan"));

        let hybrid = ExecutionLocation::Hybrid {
            remote_subquery: RelExpr::scan("t"),
            local_operations: RelExpr::scan("t"),
            target: sample_connection(),
        };
        let display = format!("{hybrid}");
        assert!(display.contains("HYBRID"));
    }

    #[test]
    fn cost_breakdown_savings() {
        let cost = FederatedCostBreakdown {
            strategy: "hybrid".into(),
            remote_exec_ms: 50.0,
            network_transfer_ms: 200.0,
            transfer_bytes: 10_485_760,
            local_exec_ms: 30.0,
            total_ms: 280.0,
            rows_transferred: 100_000,
        };
        let savings = cost.savings_percent(16_000.0);
        assert!((savings - 98.25).abs() < 0.1);
    }

    #[test]
    fn cost_breakdown_transfer_display() {
        let cost = FederatedCostBreakdown {
            strategy: "test".into(),
            remote_exec_ms: 0.0,
            network_transfer_ms: 0.0,
            transfer_bytes: 10_485_760,
            local_exec_ms: 0.0,
            total_ms: 0.0,
            rows_transferred: 0,
        };
        assert_eq!(cost.transfer_size_display(), "10.0MB");
    }

    #[test]
    fn format_bytes_ranges() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(2048), "2.0KB");
        assert_eq!(format_bytes(1_048_576), "1.0MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0GB");
    }

    #[test]
    fn federated_plan_best_alternative() {
        let plan = FederatedPlan {
            location: ExecutionLocation::Local {
                query: RelExpr::scan("t"),
            },
            cost: FederatedCostBreakdown {
                strategy: "local".into(),
                remote_exec_ms: 0.0,
                network_transfer_ms: 0.0,
                transfer_bytes: 0,
                local_exec_ms: 100.0,
                total_ms: 100.0,
                rows_transferred: 0,
            },
            alternatives: vec![
                FederatedCostBreakdown {
                    strategy: "ship_query".into(),
                    remote_exec_ms: 50.0,
                    network_transfer_ms: 200.0,
                    transfer_bytes: 1000,
                    local_exec_ms: 0.0,
                    total_ms: 250.0,
                    rows_transferred: 100,
                },
                FederatedCostBreakdown {
                    strategy: "ship_data".into(),
                    remote_exec_ms: 10.0,
                    network_transfer_ms: 5000.0,
                    transfer_bytes: 100_000,
                    local_exec_ms: 50.0,
                    total_ms: 5060.0,
                    rows_transferred: 10_000,
                },
            ],
            steps: vec![
                "Execute locally".into(),
            ],
        };
        let best = plan.best_alternative().expect("should have alternative");
        assert_eq!(best.strategy, "ship_query");
    }

    #[test]
    fn serialize_roundtrip_federated_query() {
        let mut sources = HashMap::new();
        sources.insert(
            "t".into(),
            DataSource::local("t", Statistics::new(100.0)),
        );
        let query =
            FederatedQuery::new(RelExpr::scan("t"), sources);
        let json = serde_json::to_string(&query)
            .expect("serialization should succeed");
        let deserialized: FederatedQuery =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(query, deserialized);
    }

    #[test]
    fn serialize_roundtrip_remote_connection() {
        let conn = sample_connection();
        let json = serde_json::to_string(&conn)
            .expect("serialization should succeed");
        let deserialized: RemoteConnection =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(conn, deserialized);
    }

    #[test]
    fn serialize_roundtrip_execution_location() {
        let loc = ExecutionLocation::Hybrid {
            remote_subquery: RelExpr::scan("r"),
            local_operations: RelExpr::scan("l"),
            target: sample_connection(),
        };
        let json = serde_json::to_string(&loc)
            .expect("serialization should succeed");
        let deserialized: ExecutionLocation =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(loc, deserialized);
    }
}
