//! Citus distributed query optimizer.
//!
//! Detects Citus extension metadata (distributed tables, reference
//! tables, co-location groups, columnar storage) and applies
//! Citus-specific optimization rules on top of the generic
//! distributed optimizer.
//!
//! Key optimizations:
//! - Co-located join detection (zero network cost)
//! - Reference table broadcast elimination
//! - Distributed aggregation pushdown by distribution key
//! - Shard pruning via partition key filtering
//! - Columnar storage cost adjustments
//!
//! See: `rfcs/text/0081-citusdb-distributed-query-rules.md`

use std::collections::{HashMap, HashSet};

use ra_core::algebra::{AggregateExpr, JoinType, RelExpr};
use ra_core::cost::Cost;
use ra_core::distribution::{
    DataDistribution, DistributedRelExpr, NodeId,
};
use ra_core::expr::Expr;
use ra_core::statistics::Statistics;

use crate::distributed_optimizer::{
    ClusterTopology, DistributedOptimizer, DistributedOptimizerConfig,
    DistributedOptimizerError,
};

// ------------------------------------------------------------------
// Citus metadata types
// ------------------------------------------------------------------

/// How a Citus table is distributed across workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistributionMethod {
    /// Hash-distributed by a single column.
    Hash,
    /// Append-distributed (range-based, legacy).
    Append,
    /// Range-distributed.
    Range,
}

/// Metadata for a single Citus distributed table.
#[derive(Debug, Clone)]
pub struct DistributedTableInfo {
    /// The column used as the distribution key.
    pub distribution_column: String,
    /// Hash, append, or range distribution.
    pub distribution_method: DistributionMethod,
    /// Co-location group identifier.
    pub colocation_group: u32,
    /// Number of shards for this table.
    pub shard_count: u32,
}

/// Storage format for a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    /// Standard PostgreSQL heap (row-oriented).
    Row,
    /// Citus columnar storage (column-oriented, compressed).
    Columnar,
}

/// Columnar storage metadata for cost adjustments.
#[derive(Debug, Clone)]
pub struct ColumnarTableInfo {
    /// Total number of columns in the table.
    pub total_columns: u32,
    /// Compression ratio (e.g., 10.0 means 10x compressed).
    pub compression_ratio: f64,
    /// Stripe size in rows (default 150,000 in Citus).
    pub stripe_row_count: u32,
    /// Chunk group size in rows (default 10,000).
    pub chunk_group_row_count: u32,
}

impl Default for ColumnarTableInfo {
    fn default() -> Self {
        Self {
            total_columns: 10,
            compression_ratio: 3.0,
            stripe_row_count: 150_000,
            chunk_group_row_count: 10_000,
        }
    }
}

/// A worker node in the Citus cluster.
#[derive(Debug, Clone)]
pub struct CitusWorkerNode {
    /// Unique node identifier.
    pub node_id: NodeId,
    /// Hostname or IP address.
    pub hostname: String,
    /// Port number.
    pub port: u16,
    /// Whether this node is currently active.
    pub is_active: bool,
}

/// Complete Citus cluster metadata.
///
/// Populated by querying Citus catalog tables
/// (`pg_dist_partition`, `pg_dist_shard`, `pg_dist_colocation`,
/// `pg_dist_node`, `columnar.options`).
#[derive(Debug, Clone)]
pub struct CitusMetadata {
    /// Distributed (sharded) tables and their distribution info.
    pub distributed_tables: HashMap<String, DistributedTableInfo>,
    /// Reference tables (replicated to all workers).
    pub reference_tables: HashSet<String>,
    /// Local tables (coordinator-only).
    pub local_tables: HashSet<String>,
    /// Co-location group ID to list of table names.
    pub colocation_groups: HashMap<u32, Vec<String>>,
    /// Global shard count (default 32 in Citus).
    pub shard_count: u32,
    /// Worker nodes in the cluster.
    pub worker_nodes: Vec<CitusWorkerNode>,
    /// Columnar storage info per table.
    pub columnar_tables: HashMap<String, ColumnarTableInfo>,
    /// Table statistics.
    pub table_stats: HashMap<String, Statistics>,
}

impl CitusMetadata {
    /// Create empty metadata for a cluster with the given shard
    /// count.
    #[must_use]
    pub fn new(shard_count: u32) -> Self {
        Self {
            distributed_tables: HashMap::new(),
            reference_tables: HashSet::new(),
            local_tables: HashSet::new(),
            colocation_groups: HashMap::new(),
            shard_count,
            worker_nodes: Vec::new(),
            columnar_tables: HashMap::new(),
            table_stats: HashMap::new(),
        }
    }

    /// Register a distributed table.
    pub fn add_distributed_table(
        &mut self,
        table: &str,
        info: DistributedTableInfo,
    ) {
        self.colocation_groups
            .entry(info.colocation_group)
            .or_default()
            .push(table.to_owned());
        self.distributed_tables.insert(table.to_owned(), info);
    }

    /// Register a reference table.
    pub fn add_reference_table(&mut self, table: &str) {
        self.reference_tables.insert(table.to_owned());
    }

    /// Register a local table.
    pub fn add_local_table(&mut self, table: &str) {
        self.local_tables.insert(table.to_owned());
    }

    /// Register a worker node.
    pub fn add_worker_node(&mut self, node: CitusWorkerNode) {
        self.worker_nodes.push(node);
    }

    /// Register columnar storage info for a table.
    pub fn add_columnar_table(
        &mut self,
        table: &str,
        info: ColumnarTableInfo,
    ) {
        self.columnar_tables.insert(table.to_owned(), info);
    }

    /// Register statistics for a table.
    pub fn add_table_stats(
        &mut self,
        table: &str,
        stats: Statistics,
    ) {
        self.table_stats.insert(table.to_owned(), stats);
    }

    /// Check if a table is distributed (sharded).
    #[must_use]
    pub fn is_distributed(&self, table: &str) -> bool {
        self.distributed_tables.contains_key(table)
    }

    /// Check if a table is a reference table.
    #[must_use]
    pub fn is_reference(&self, table: &str) -> bool {
        self.reference_tables.contains(table)
    }

    /// Check if a table uses columnar storage.
    #[must_use]
    pub fn is_columnar(&self, table: &str) -> bool {
        self.columnar_tables.contains_key(table)
    }

    /// Check if two distributed tables are co-located.
    #[must_use]
    pub fn are_colocated(
        &self,
        table_a: &str,
        table_b: &str,
    ) -> bool {
        let info_a = self.distributed_tables.get(table_a);
        let info_b = self.distributed_tables.get(table_b);
        match (info_a, info_b) {
            (Some(a), Some(b)) => {
                a.colocation_group == b.colocation_group
            }
            _ => false,
        }
    }

    /// Get the distribution column for a table, if distributed.
    #[must_use]
    pub fn distribution_column(
        &self,
        table: &str,
    ) -> Option<&str> {
        self.distributed_tables
            .get(table)
            .map(|info| info.distribution_column.as_str())
    }

    /// Return the number of active worker nodes.
    #[must_use]
    pub fn active_worker_count(&self) -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        let count =
            self.worker_nodes.iter().filter(|n| n.is_active).count()
                as u32;
        count
    }
}

// ------------------------------------------------------------------
// Shard pruning
// ------------------------------------------------------------------

/// Result of shard pruning analysis.
#[derive(Debug, Clone)]
pub struct ShardPruningResult {
    /// Number of shards that must be scanned.
    pub shards_remaining: u32,
    /// Total number of shards.
    pub total_shards: u32,
    /// Whether the filter fully determines a single shard.
    pub is_single_shard: bool,
}

impl ShardPruningResult {
    /// Fraction of shards that remain after pruning.
    #[must_use]
    pub fn selectivity(&self) -> f64 {
        if self.total_shards == 0 {
            return 1.0;
        }
        f64::from(self.shards_remaining) / f64::from(self.total_shards)
    }
}

/// Analyze a filter predicate for shard pruning opportunities.
///
/// When the predicate contains an equality condition on the
/// distribution column, only one shard needs to be scanned.
/// Range predicates prune proportionally.
#[must_use]
pub fn analyze_shard_pruning(
    predicate: &Expr,
    distribution_column: &str,
    total_shards: u32,
) -> ShardPruningResult {
    match predicate {
        Expr::BinOp {
            op: ra_core::expr::BinOp::Eq,
            left,
            right,
        } => {
            let references_dist_col =
                expr_references_column(left, distribution_column)
                    || expr_references_column(
                        right,
                        distribution_column,
                    );
            if references_dist_col {
                return ShardPruningResult {
                    shards_remaining: 1,
                    total_shards,
                    is_single_shard: true,
                };
            }
            ShardPruningResult {
                shards_remaining: total_shards,
                total_shards,
                is_single_shard: false,
            }
        }
        Expr::BinOp {
            op:
                ra_core::expr::BinOp::Gt
                | ra_core::expr::BinOp::Ge
                | ra_core::expr::BinOp::Lt
                | ra_core::expr::BinOp::Le,
            left,
            right,
        } => {
            let references_dist_col =
                expr_references_column(left, distribution_column)
                    || expr_references_column(
                        right,
                        distribution_column,
                    );
            if references_dist_col {
                let remaining = (total_shards / 2).max(1);
                return ShardPruningResult {
                    shards_remaining: remaining,
                    total_shards,
                    is_single_shard: false,
                };
            }
            ShardPruningResult {
                shards_remaining: total_shards,
                total_shards,
                is_single_shard: false,
            }
        }
        Expr::BinOp {
            op: ra_core::expr::BinOp::And,
            left,
            right,
        } => {
            let left_result = analyze_shard_pruning(
                left,
                distribution_column,
                total_shards,
            );
            let right_result = analyze_shard_pruning(
                right,
                distribution_column,
                total_shards,
            );
            if left_result.is_single_shard {
                return left_result;
            }
            if right_result.is_single_shard {
                return right_result;
            }
            let remaining = left_result
                .shards_remaining
                .min(right_result.shards_remaining);
            ShardPruningResult {
                shards_remaining: remaining,
                total_shards,
                is_single_shard: false,
            }
        }
        _ => ShardPruningResult {
            shards_remaining: total_shards,
            total_shards,
            is_single_shard: false,
        },
    }
}

/// Check if an expression references a specific column name.
fn expr_references_column(expr: &Expr, column: &str) -> bool {
    match expr {
        Expr::Column(col_ref) => col_ref.column == column,
        Expr::BinOp { left, right, .. } => {
            expr_references_column(left, column)
                || expr_references_column(right, column)
        }
        Expr::UnaryOp { operand, .. } => {
            expr_references_column(operand, column)
        }
        _ => false,
    }
}

// ------------------------------------------------------------------
// Columnar cost model
// ------------------------------------------------------------------

/// Cost adjustment parameters for columnar storage.
#[derive(Debug, Clone)]
pub struct ColumnarCostParams {
    /// Base I/O cost multiplier for columnar vs row scans.
    /// Columnar reads fewer pages for narrow projections.
    pub io_cost_factor: f64,
    /// CPU cost multiplier for decompression overhead.
    pub decompression_cpu_factor: f64,
    /// Minimum column selectivity (even reading 1 column has
    /// some overhead).
    pub min_column_selectivity: f64,
}

impl Default for ColumnarCostParams {
    fn default() -> Self {
        Self {
            io_cost_factor: 0.8,
            decompression_cpu_factor: 1.5,
            min_column_selectivity: 0.05,
        }
    }
}

/// Estimate the cost adjustment for a columnar table scan.
///
/// Returns a multiplier relative to a standard row-based scan.
/// Values < 1.0 mean columnar is cheaper; > 1.0 means more
/// expensive.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn columnar_scan_cost_factor(
    projected_columns: u32,
    columnar_info: &ColumnarTableInfo,
    params: &ColumnarCostParams,
) -> f64 {
    if columnar_info.total_columns == 0 {
        return 1.0;
    }

    let column_selectivity = (f64::from(projected_columns)
        / f64::from(columnar_info.total_columns))
    .max(params.min_column_selectivity);

    let io_factor =
        column_selectivity * params.io_cost_factor
            / columnar_info.compression_ratio.max(1.0);

    let cpu_factor = params.decompression_cpu_factor;

    // Combined: I/O savings partially offset by CPU decompression
    io_factor + (1.0 - io_factor) * (cpu_factor - 1.0) * 0.1
}

// ------------------------------------------------------------------
// Citus optimizer
// ------------------------------------------------------------------

/// Errors from the Citus optimizer.
#[derive(Debug, thiserror::Error)]
pub enum CitusOptimizerError {
    /// Citus extension not detected.
    #[error("citus extension not detected in the database")]
    NoCitusExtension,

    /// No worker nodes configured.
    #[error("no active worker nodes in the Citus cluster")]
    NoWorkerNodes,

    /// Table not found in Citus metadata.
    #[error("table not found in Citus metadata: {0}")]
    TableNotFound(String),

    /// Underlying distributed optimizer error.
    #[error("distributed optimizer error: {0}")]
    DistributedError(#[from] DistributedOptimizerError),
}

/// Configuration for the Citus optimizer.
#[derive(Debug, Clone)]
pub struct CitusOptimizerConfig {
    /// Base distributed optimizer config.
    pub distributed: DistributedOptimizerConfig,
    /// Cost parameters for columnar storage.
    pub columnar_params: ColumnarCostParams,
    /// Network cost multiplier for coordinator-worker transfers.
    pub coordinator_network_factor: f64,
    /// Network cost multiplier for worker-worker transfers.
    pub worker_network_factor: f64,
}

impl Default for CitusOptimizerConfig {
    fn default() -> Self {
        Self {
            distributed: DistributedOptimizerConfig::default(),
            columnar_params: ColumnarCostParams::default(),
            coordinator_network_factor: 1.0,
            worker_network_factor: 2.0,
        }
    }
}

/// Citus-aware distributed query optimizer.
///
/// Wraps the generic [`DistributedOptimizer`] and adds
/// Citus-specific rules for co-located joins, reference table
/// elimination, distributed aggregation pushdown, shard pruning,
/// and columnar storage cost adjustment.
#[derive(Debug)]
pub struct CitusOptimizer {
    config: CitusOptimizerConfig,
    metadata: CitusMetadata,
    inner: DistributedOptimizer,
}

impl CitusOptimizer {
    /// Create a Citus optimizer from metadata and config.
    ///
    /// Builds the underlying `ClusterTopology` and
    /// `DistributedOptimizer` from the Citus metadata.
    #[must_use]
    pub fn new(
        config: CitusOptimizerConfig,
        metadata: CitusMetadata,
    ) -> Self {
        let num_workers = metadata.active_worker_count().max(1);
        let mut topology =
            ClusterTopology::uniform(num_workers + 1);

        for (table, info) in &metadata.distributed_tables {
            let dist_col_expr = Expr::Column(
                ra_core::expr::ColumnRef::new(
                    &info.distribution_column,
                ),
            );
            topology.register_table(
                table,
                NodeId(0),
                DataDistribution::HashPartitioned {
                    keys: vec![dist_col_expr],
                    partition_count: info.shard_count,
                },
            );
        }

        for table in &metadata.reference_tables {
            topology.register_table(
                table,
                NodeId(0),
                DataDistribution::Replicated,
            );
        }

        for table in &metadata.local_tables {
            topology.register_table(
                table,
                NodeId(0),
                DataDistribution::SinglePartition {
                    node: NodeId(0),
                },
            );
        }

        let mut inner = DistributedOptimizer::new(
            config.distributed.clone(),
            topology,
        );

        for (table, stats) in &metadata.table_stats {
            inner.register_stats(table, stats.clone());
        }

        Self {
            config,
            metadata,
            inner,
        }
    }

    /// Get a reference to the Citus metadata.
    #[must_use]
    pub fn metadata(&self) -> &CitusMetadata {
        &self.metadata
    }

    /// Optimize a plan with Citus-aware rules.
    ///
    /// # Errors
    ///
    /// Returns an error if the cluster has no active workers or
    /// the underlying distributed optimizer fails.
    pub fn optimize(
        &self,
        plan: &RelExpr,
    ) -> Result<CitusOptimizedPlan, CitusOptimizerError> {
        if self.metadata.active_worker_count() == 0
            && self.metadata.worker_nodes.is_empty()
        {
            return Err(CitusOptimizerError::NoWorkerNodes);
        }

        let annotated = self.annotate(plan)?;
        Ok(annotated)
    }

    /// Recursively annotate a plan with Citus-aware distribution.
    fn annotate(
        &self,
        plan: &RelExpr,
    ) -> Result<CitusOptimizedPlan, CitusOptimizerError> {
        match plan {
            RelExpr::Scan { table, .. } => {
                self.annotate_scan(table, plan)
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => self.annotate_join(
                *join_type, condition, left, right, plan,
            ),
            RelExpr::Filter {
                predicate, input, ..
            } => self.annotate_filter(predicate, input, plan),
            RelExpr::Project { input, columns } => {
                let child = self.annotate(input)?;
                Ok(CitusOptimizedPlan {
                    plan: plan.clone(),
                    distribution: child.distribution,
                    strategy: child.strategy,
                    shard_pruning: None,
                    columnar_adjustment: self
                        .columnar_project_adjustment(
                            input, columns,
                        ),
                    execution: child.execution,
                })
            }
            RelExpr::Aggregate {
                input,
                group_by,
                aggregates,
            } => self.annotate_aggregate(
                input, group_by, aggregates, plan,
            ),
            _ => {
                let distributed =
                    self.inner.optimize_distribution(plan)?;
                Ok(CitusOptimizedPlan::from_distributed(
                    distributed,
                ))
            }
        }
    }

    /// Annotate a scan with Citus distribution info.
    fn annotate_scan(
        &self,
        table: &str,
        plan: &RelExpr,
    ) -> Result<CitusOptimizedPlan, CitusOptimizerError> {
        let distribution = if self.metadata.is_distributed(table) {
            let info = &self.metadata.distributed_tables[table];
            let dist_col = Expr::Column(
                ra_core::expr::ColumnRef::new(
                    &info.distribution_column,
                ),
            );
            DataDistribution::HashPartitioned {
                keys: vec![dist_col],
                partition_count: info.shard_count,
            }
        } else if self.metadata.is_reference(table) {
            DataDistribution::Replicated
        } else {
            DataDistribution::SinglePartition {
                node: NodeId(0),
            }
        };

        let execution = if self.metadata.is_distributed(table) {
            ExecutionLocation::Workers
        } else if self.metadata.is_reference(table) {
            ExecutionLocation::AllNodes
        } else {
            ExecutionLocation::Coordinator
        };

        let columnar_adj = if self.metadata.is_columnar(table) {
            let info = &self.metadata.columnar_tables[table];
            Some(columnar_scan_cost_factor(
                info.total_columns,
                info,
                &self.config.columnar_params,
            ))
        } else {
            None
        };

        Ok(CitusOptimizedPlan {
            plan: plan.clone(),
            distribution,
            strategy: CitusStrategy::LocalScan,
            shard_pruning: None,
            columnar_adjustment: columnar_adj,
            execution,
        })
    }

    /// Annotate a filter for shard pruning.
    fn annotate_filter(
        &self,
        predicate: &Expr,
        input: &RelExpr,
        plan: &RelExpr,
    ) -> Result<CitusOptimizedPlan, CitusOptimizerError> {
        let child = self.annotate(input)?;

        let table_name = extract_table_name(input);
        let pruning = table_name.and_then(|t| {
            let dist_col =
                self.metadata.distribution_column(&t)?;
            let info = &self.metadata.distributed_tables[&t];
            let result = analyze_shard_pruning(
                predicate,
                dist_col,
                info.shard_count,
            );
            if result.shards_remaining < result.total_shards {
                Some(result)
            } else {
                None
            }
        });

        let strategy = if pruning
            .as_ref()
            .is_some_and(|p| p.is_single_shard)
        {
            CitusStrategy::SingleShardQuery
        } else if pruning.is_some() {
            CitusStrategy::ShardPruned
        } else {
            child.strategy
        };

        let execution =
            if pruning.as_ref().is_some_and(|p| p.is_single_shard) {
                ExecutionLocation::SingleWorker
            } else {
                child.execution
            };

        Ok(CitusOptimizedPlan {
            plan: plan.clone(),
            distribution: child.distribution,
            strategy,
            shard_pruning: pruning,
            columnar_adjustment: child.columnar_adjustment,
            execution,
        })
    }

    /// Annotate a join with Citus-aware strategy selection.
    fn annotate_join(
        &self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        plan: &RelExpr,
    ) -> Result<CitusOptimizedPlan, CitusOptimizerError> {
        let left_ann = self.annotate(left)?;
        let right_ann = self.annotate(right)?;

        let left_table = extract_table_name(left);
        let right_table = extract_table_name(right);

        // Rule 1: Co-located join detection
        if let (Some(lt), Some(rt)) =
            (&left_table, &right_table)
        {
            if self.is_colocated_join(
                lt, rt, condition, join_type,
            ) {
                return Ok(CitusOptimizedPlan {
                    plan: plan.clone(),
                    distribution: left_ann.distribution,
                    strategy: CitusStrategy::ColocatedJoin,
                    shard_pruning: None,
                    columnar_adjustment: None,
                    execution: ExecutionLocation::Workers,
                });
            }
        }

        // Rule 2: Reference table join
        if let Some(rt) = &right_table {
            if self.metadata.is_reference(rt) {
                return Ok(CitusOptimizedPlan {
                    plan: plan.clone(),
                    distribution: left_ann.distribution,
                    strategy: CitusStrategy::ReferenceJoin,
                    shard_pruning: None,
                    columnar_adjustment: None,
                    execution: left_ann.execution,
                });
            }
        }
        if let Some(lt) = &left_table {
            if self.metadata.is_reference(lt) {
                return Ok(CitusOptimizedPlan {
                    plan: plan.clone(),
                    distribution: right_ann.distribution,
                    strategy: CitusStrategy::ReferenceJoin,
                    shard_pruning: None,
                    columnar_adjustment: None,
                    execution: right_ann.execution,
                });
            }
        }

        // Fall back to generic distributed optimizer
        let distributed =
            self.inner.optimize_distribution(plan)?;
        Ok(CitusOptimizedPlan::from_distributed(distributed))
    }

    /// Check if a join is co-located in Citus.
    ///
    /// A join is co-located when:
    /// 1. Both tables are distributed
    /// 2. Both are in the same co-location group
    /// 3. The join condition includes equality on both
    ///    distribution columns
    fn is_colocated_join(
        &self,
        left_table: &str,
        right_table: &str,
        condition: &Expr,
        _join_type: JoinType,
    ) -> bool {
        if !self.metadata.are_colocated(left_table, right_table) {
            return false;
        }

        let left_dist_col =
            match self.metadata.distribution_column(left_table) {
                Some(col) => col,
                None => return false,
            };
        let right_dist_col =
            match self.metadata.distribution_column(right_table) {
                Some(col) => col,
                None => return false,
            };

        condition_has_equality_on(
            condition,
            left_dist_col,
            right_dist_col,
        )
    }

    /// Annotate an aggregate with distributed pushdown awareness.
    #[allow(clippy::too_many_lines)]
    fn annotate_aggregate(
        &self,
        input: &RelExpr,
        group_by: &[Expr],
        aggregates: &[AggregateExpr],
        plan: &RelExpr,
    ) -> Result<CitusOptimizedPlan, CitusOptimizerError> {
        let child = self.annotate(input)?;
        let table_name = extract_table_name(input);

        // Rule 3: Check if GROUP BY includes the distribution
        // column, enabling worker-side aggregation.
        if let Some(ref t) = table_name {
            if let Some(dist_col) =
                self.metadata.distribution_column(t)
            {
                let group_by_includes_dist =
                    group_by.iter().any(|expr| {
                        expr_references_column(expr, dist_col)
                    });

                if group_by_includes_dist {
                    let can_pushdown =
                        aggregates_are_pushdownable(aggregates);
                    if can_pushdown {
                        return Ok(CitusOptimizedPlan {
                            plan: plan.clone(),
                            distribution:
                                child.distribution.clone(),
                            strategy:
                                CitusStrategy::DistributedAggregation,
                            shard_pruning: None,
                            columnar_adjustment: None,
                            execution: ExecutionLocation::Workers,
                        });
                    }
                }
            }
        }

        // Fall back to distributed optimizer
        let distributed =
            self.inner.optimize_distribution(plan)?;
        Ok(CitusOptimizedPlan::from_distributed(distributed))
    }

    /// Compute columnar cost adjustment for a projection.
    fn columnar_project_adjustment(
        &self,
        input: &RelExpr,
        columns: &[ra_core::algebra::ProjectionColumn],
    ) -> Option<f64> {
        let table = extract_table_name(input)?;
        let info = self.metadata.columnar_tables.get(&table)?;
        #[allow(clippy::cast_possible_truncation)]
        let projected = columns.len() as u32;
        Some(columnar_scan_cost_factor(
            projected,
            info,
            &self.config.columnar_params,
        ))
    }

    /// Estimate the cost of a Citus-optimized plan.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn estimate_cost(
        &self,
        plan: &CitusOptimizedPlan,
    ) -> Cost {
        let base_cost = match &plan.strategy {
            CitusStrategy::ColocatedJoin => Cost::ZERO,
            CitusStrategy::ReferenceJoin => Cost::ZERO,
            CitusStrategy::DistributedAggregation => {
                let workers =
                    self.metadata.active_worker_count().max(1);
                let agg_cpu = 100.0 / f64::from(workers);
                let gather_network =
                    f64::from(workers) * 0.1;
                Cost::new(agg_cpu, 0.0, gather_network, 0)
            }
            CitusStrategy::SingleShardQuery => {
                Cost::new(1.0, 1.0, 0.1, 0)
            }
            CitusStrategy::ShardPruned => {
                let selectivity = plan
                    .shard_pruning
                    .as_ref()
                    .map_or(1.0, ShardPruningResult::selectivity);
                Cost::new(
                    100.0 * selectivity,
                    100.0 * selectivity,
                    selectivity,
                    0,
                )
            }
            CitusStrategy::LocalScan => Cost::new(1.0, 1.0, 0.0, 0),
            CitusStrategy::GenericDistributed => {
                Cost::new(100.0, 100.0, 10.0, 0)
            }
        };

        if let Some(col_adj) = plan.columnar_adjustment {
            Cost::new(
                base_cost.cpu * col_adj,
                base_cost.io * col_adj,
                base_cost.network,
                base_cost.memory,
            )
        } else {
            base_cost
        }
    }
}

// ------------------------------------------------------------------
// Plan output types
// ------------------------------------------------------------------

/// Where a Citus query fragment executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionLocation {
    /// Runs only on the coordinator node.
    Coordinator,
    /// Runs on all worker nodes in parallel.
    Workers,
    /// Runs on a single worker (after shard pruning).
    SingleWorker,
    /// Runs on all nodes (coordinator + workers).
    AllNodes,
}

/// The Citus-specific strategy chosen for an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitusStrategy {
    /// Join is co-located: both sides on the same shards.
    ColocatedJoin,
    /// One side is a reference table: no data movement.
    ReferenceJoin,
    /// Aggregation pushed to workers by distribution key.
    DistributedAggregation,
    /// Query targets a single shard via equality filter.
    SingleShardQuery,
    /// Query prunes some shards via range filter.
    ShardPruned,
    /// Simple local scan (no distribution needed).
    LocalScan,
    /// Fell back to generic distributed optimizer.
    GenericDistributed,
}

impl CitusStrategy {
    /// Human-readable label for the strategy.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::ColocatedJoin => "ColocatedJoin",
            Self::ReferenceJoin => "ReferenceJoin",
            Self::DistributedAggregation => {
                "DistributedAggregation"
            }
            Self::SingleShardQuery => "SingleShardQuery",
            Self::ShardPruned => "ShardPruned",
            Self::LocalScan => "LocalScan",
            Self::GenericDistributed => "GenericDistributed",
        }
    }

    /// Whether this strategy requires network data transfer.
    #[must_use]
    pub fn requires_network_transfer(&self) -> bool {
        match self {
            Self::ColocatedJoin
            | Self::ReferenceJoin
            | Self::SingleShardQuery
            | Self::LocalScan => false,
            Self::DistributedAggregation
            | Self::ShardPruned
            | Self::GenericDistributed => true,
        }
    }
}

/// A Citus-optimized query plan with distribution annotations.
#[derive(Debug, Clone)]
pub struct CitusOptimizedPlan {
    /// The original relational expression.
    pub plan: RelExpr,
    /// Output data distribution.
    pub distribution: DataDistribution,
    /// The Citus strategy chosen for this operator.
    pub strategy: CitusStrategy,
    /// Shard pruning result, if applicable.
    pub shard_pruning: Option<ShardPruningResult>,
    /// Columnar cost adjustment factor, if applicable.
    pub columnar_adjustment: Option<f64>,
    /// Where this operator executes.
    pub execution: ExecutionLocation,
}

impl CitusOptimizedPlan {
    /// Create from a generic `DistributedRelExpr`.
    fn from_distributed(dre: DistributedRelExpr) -> Self {
        Self {
            plan: dre.plan,
            distribution: dre.distribution,
            strategy: CitusStrategy::GenericDistributed,
            shard_pruning: None,
            columnar_adjustment: None,
            execution: ExecutionLocation::Workers,
        }
    }
}

// ------------------------------------------------------------------
// Helper functions
// ------------------------------------------------------------------

/// Extract the base table name from a plan tree.
fn extract_table_name(plan: &RelExpr) -> Option<String> {
    match plan {
        RelExpr::Scan { table, .. } => Some(table.clone()),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Aggregate { input, .. } => {
            extract_table_name(input)
        }
        _ => None,
    }
}

/// Check if a condition contains `col_a = col_b` (in either order).
fn condition_has_equality_on(
    condition: &Expr,
    col_a: &str,
    col_b: &str,
) -> bool {
    match condition {
        Expr::BinOp {
            op: ra_core::expr::BinOp::Eq,
            left,
            right,
        } => {
            (expr_references_column(left, col_a)
                && expr_references_column(right, col_b))
                || (expr_references_column(left, col_b)
                    && expr_references_column(right, col_a))
        }
        Expr::BinOp {
            op: ra_core::expr::BinOp::And,
            left,
            right,
        } => {
            condition_has_equality_on(left, col_a, col_b)
                || condition_has_equality_on(right, col_a, col_b)
        }
        _ => false,
    }
}

/// Check if aggregates can be pushed to Citus workers.
///
/// Decomposable aggregates (SUM, COUNT, MIN, MAX, AVG) can run
/// as partial aggregates on workers with final aggregation on
/// the coordinator.
fn aggregates_are_pushdownable(
    aggregates: &[AggregateExpr],
) -> bool {
    use ra_core::algebra::AggregateFunction;

    if aggregates.is_empty() {
        return true;
    }

    aggregates.iter().all(|agg| {
        matches!(
            agg.function,
            AggregateFunction::Sum
                | AggregateFunction::Count
                | AggregateFunction::Min
                | AggregateFunction::Max
                | AggregateFunction::Avg
        ) && !agg.distinct
    })
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{AggregateExpr, AggregateFunction};
    use ra_core::expr::{BinOp, ColumnRef, Const};

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef::new(name))
    }

    fn eq(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn and(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::And,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn gt(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn sample_metadata() -> CitusMetadata {
        let mut meta = CitusMetadata::new(32);

        meta.add_distributed_table(
            "orders",
            DistributedTableInfo {
                distribution_column: "customer_id".to_owned(),
                distribution_method: DistributionMethod::Hash,
                colocation_group: 1,
                shard_count: 32,
            },
        );

        meta.add_distributed_table(
            "order_items",
            DistributedTableInfo {
                distribution_column: "customer_id".to_owned(),
                distribution_method: DistributionMethod::Hash,
                colocation_group: 1,
                shard_count: 32,
            },
        );

        meta.add_distributed_table(
            "products",
            DistributedTableInfo {
                distribution_column: "product_id".to_owned(),
                distribution_method: DistributionMethod::Hash,
                colocation_group: 2,
                shard_count: 32,
            },
        );

        meta.add_reference_table("countries");
        meta.add_reference_table("currencies");
        meta.add_local_table("config");

        meta.add_worker_node(CitusWorkerNode {
            node_id: NodeId(1),
            hostname: "worker-1".to_owned(),
            port: 5432,
            is_active: true,
        });
        meta.add_worker_node(CitusWorkerNode {
            node_id: NodeId(2),
            hostname: "worker-2".to_owned(),
            port: 5432,
            is_active: true,
        });
        meta.add_worker_node(CitusWorkerNode {
            node_id: NodeId(3),
            hostname: "worker-3".to_owned(),
            port: 5432,
            is_active: true,
        });
        meta.add_worker_node(CitusWorkerNode {
            node_id: NodeId(4),
            hostname: "worker-4".to_owned(),
            port: 5432,
            is_active: true,
        });

        let mut orders_stats = Statistics::new(100_000_000.0);
        orders_stats.avg_row_size = 256;
        orders_stats.total_size = 25_600_000_000;
        meta.add_table_stats("orders", orders_stats);

        let mut items_stats = Statistics::new(500_000_000.0);
        items_stats.avg_row_size = 128;
        items_stats.total_size = 64_000_000_000;
        meta.add_table_stats("order_items", items_stats);

        let mut countries_stats = Statistics::new(250.0);
        countries_stats.avg_row_size = 64;
        countries_stats.total_size = 16_000;
        meta.add_table_stats("countries", countries_stats);

        meta
    }

    fn sample_metadata_with_columnar() -> CitusMetadata {
        let mut meta = sample_metadata();
        meta.add_columnar_table(
            "events",
            ColumnarTableInfo {
                total_columns: 50,
                compression_ratio: 10.0,
                stripe_row_count: 150_000,
                chunk_group_row_count: 10_000,
            },
        );
        meta.add_distributed_table(
            "events",
            DistributedTableInfo {
                distribution_column: "tenant_id".to_owned(),
                distribution_method: DistributionMethod::Hash,
                colocation_group: 3,
                shard_count: 32,
            },
        );
        meta
    }

    fn make_optimizer() -> CitusOptimizer {
        let config = CitusOptimizerConfig::default();
        let metadata = sample_metadata();
        CitusOptimizer::new(config, metadata)
    }

    // ------ CitusMetadata tests ------

    #[test]
    fn metadata_distributed_table() {
        let meta = sample_metadata();
        assert!(meta.is_distributed("orders"));
        assert!(meta.is_distributed("order_items"));
        assert!(!meta.is_distributed("countries"));
        assert!(!meta.is_distributed("config"));
    }

    #[test]
    fn metadata_reference_table() {
        let meta = sample_metadata();
        assert!(meta.is_reference("countries"));
        assert!(meta.is_reference("currencies"));
        assert!(!meta.is_reference("orders"));
    }

    #[test]
    fn metadata_colocation() {
        let meta = sample_metadata();
        assert!(meta.are_colocated("orders", "order_items"));
        assert!(!meta.are_colocated("orders", "products"));
        assert!(!meta.are_colocated("orders", "countries"));
    }

    #[test]
    fn metadata_distribution_column() {
        let meta = sample_metadata();
        assert_eq!(
            meta.distribution_column("orders"),
            Some("customer_id")
        );
        assert_eq!(
            meta.distribution_column("products"),
            Some("product_id")
        );
        assert_eq!(meta.distribution_column("countries"), None);
    }

    #[test]
    fn metadata_active_workers() {
        let meta = sample_metadata();
        assert_eq!(meta.active_worker_count(), 4);
    }

    #[test]
    fn metadata_inactive_workers_excluded() {
        let mut meta = sample_metadata();
        meta.worker_nodes.push(CitusWorkerNode {
            node_id: NodeId(5),
            hostname: "worker-5".to_owned(),
            port: 5432,
            is_active: false,
        });
        assert_eq!(meta.active_worker_count(), 4);
    }

    #[test]
    fn metadata_columnar_detection() {
        let meta = sample_metadata_with_columnar();
        assert!(meta.is_columnar("events"));
        assert!(!meta.is_columnar("orders"));
    }

    #[test]
    fn metadata_empty_cluster() {
        let meta = CitusMetadata::new(32);
        assert_eq!(meta.active_worker_count(), 0);
        assert!(!meta.is_distributed("any_table"));
        assert!(!meta.is_reference("any_table"));
    }

    // ------ Shard pruning tests ------

    #[test]
    fn shard_pruning_equality() {
        let pred = eq(
            col("customer_id"),
            Expr::Const(Const::Int(42)),
        );
        let result =
            analyze_shard_pruning(&pred, "customer_id", 32);
        assert!(result.is_single_shard);
        assert_eq!(result.shards_remaining, 1);
        assert_eq!(result.total_shards, 32);
    }

    #[test]
    fn shard_pruning_range() {
        let pred = gt(
            col("customer_id"),
            Expr::Const(Const::Int(1000)),
        );
        let result =
            analyze_shard_pruning(&pred, "customer_id", 32);
        assert!(!result.is_single_shard);
        assert_eq!(result.shards_remaining, 16);
    }

    #[test]
    fn shard_pruning_and_with_equality() {
        let pred = and(
            eq(col("customer_id"), Expr::Const(Const::Int(42))),
            gt(col("amount"), Expr::Const(Const::Int(100))),
        );
        let result =
            analyze_shard_pruning(&pred, "customer_id", 32);
        assert!(result.is_single_shard);
        assert_eq!(result.shards_remaining, 1);
    }

    #[test]
    fn shard_pruning_unrelated_column() {
        let pred = eq(
            col("status"),
            Expr::Const(Const::String("active".into())),
        );
        let result =
            analyze_shard_pruning(&pred, "customer_id", 32);
        assert!(!result.is_single_shard);
        assert_eq!(result.shards_remaining, 32);
    }

    #[test]
    fn shard_pruning_selectivity() {
        let pred = eq(
            col("customer_id"),
            Expr::Const(Const::Int(1)),
        );
        let result =
            analyze_shard_pruning(&pred, "customer_id", 32);
        assert!((result.selectivity() - 1.0 / 32.0).abs() < 0.001);
    }

    #[test]
    fn shard_pruning_zero_shards() {
        let pred = eq(
            col("customer_id"),
            Expr::Const(Const::Int(1)),
        );
        let result =
            analyze_shard_pruning(&pred, "customer_id", 0);
        assert!((result.selectivity() - 1.0).abs() < f64::EPSILON);
    }

    // ------ Columnar cost model tests ------

    #[test]
    fn columnar_narrow_projection_cheaper() {
        let info = ColumnarTableInfo {
            total_columns: 50,
            compression_ratio: 10.0,
            ..ColumnarTableInfo::default()
        };
        let params = ColumnarCostParams::default();
        let factor = columnar_scan_cost_factor(2, &info, &params);
        assert!(
            factor < 1.0,
            "narrow projection on columnar should be cheaper: {factor}"
        );
    }

    #[test]
    fn columnar_wide_projection_not_cheaper() {
        let info = ColumnarTableInfo {
            total_columns: 50,
            compression_ratio: 1.0,
            ..ColumnarTableInfo::default()
        };
        let params = ColumnarCostParams::default();
        let factor = columnar_scan_cost_factor(50, &info, &params);
        // With no compression and all columns, not much savings
        assert!(
            factor > 0.5,
            "wide projection on uncompressed columnar should not \
             be dramatically cheaper: {factor}"
        );
    }

    #[test]
    fn columnar_compression_helps() {
        let info_low = ColumnarTableInfo {
            total_columns: 20,
            compression_ratio: 1.0,
            ..ColumnarTableInfo::default()
        };
        let info_high = ColumnarTableInfo {
            total_columns: 20,
            compression_ratio: 10.0,
            ..ColumnarTableInfo::default()
        };
        let params = ColumnarCostParams::default();
        let factor_low =
            columnar_scan_cost_factor(5, &info_low, &params);
        let factor_high =
            columnar_scan_cost_factor(5, &info_high, &params);
        assert!(
            factor_high < factor_low,
            "higher compression should give lower cost: \
             high={factor_high}, low={factor_low}"
        );
    }

    #[test]
    fn columnar_zero_columns_returns_one() {
        let info = ColumnarTableInfo {
            total_columns: 0,
            compression_ratio: 5.0,
            ..ColumnarTableInfo::default()
        };
        let params = ColumnarCostParams::default();
        let factor = columnar_scan_cost_factor(0, &info, &params);
        assert!(
            (factor - 1.0).abs() < f64::EPSILON,
            "zero columns should return factor 1.0"
        );
    }

    // ------ Co-located join tests ------

    #[test]
    fn colocated_join_detected() {
        let opt = make_optimizer();
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("customer_id"), col("customer_id")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("order_items")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::ColocatedJoin);
        assert_eq!(result.execution, ExecutionLocation::Workers);
    }

    #[test]
    fn colocated_join_not_detected_different_groups() {
        let opt = make_optimizer();
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("product_id"), col("product_id")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("products")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_ne!(result.strategy, CitusStrategy::ColocatedJoin);
    }

    #[test]
    fn colocated_join_requires_dist_key_equality() {
        let opt = make_optimizer();
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("order_id"), col("item_id")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("order_items")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_ne!(
            result.strategy,
            CitusStrategy::ColocatedJoin,
            "join on non-distribution columns should not be colocated"
        );
    }

    // ------ Reference table join tests ------

    #[test]
    fn reference_join_right_side() {
        let opt = make_optimizer();
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("country_code"), col("code")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("countries")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::ReferenceJoin);
    }

    #[test]
    fn reference_join_left_side() {
        let opt = make_optimizer();
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("code"), col("country_code")),
            left: Box::new(RelExpr::scan("countries")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::ReferenceJoin);
    }

    #[test]
    fn reference_join_preserves_distributed_side_distribution() {
        let opt = make_optimizer();
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("country_code"), col("code")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("countries")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert!(matches!(
            result.distribution,
            DataDistribution::HashPartitioned { .. }
        ));
    }

    // ------ Distributed aggregation tests ------

    #[test]
    fn distributed_agg_with_dist_key() {
        let opt = make_optimizer();
        let plan = RelExpr::Aggregate {
            group_by: vec![col("customer_id")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("amount")),
                distinct: false,
                alias: Some("total".into()),
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(
            result.strategy,
            CitusStrategy::DistributedAggregation
        );
        assert_eq!(result.execution, ExecutionLocation::Workers);
    }

    #[test]
    fn distributed_agg_without_dist_key() {
        let opt = make_optimizer();
        let plan = RelExpr::Aggregate {
            group_by: vec![col("status")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".into()),
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_ne!(
            result.strategy,
            CitusStrategy::DistributedAggregation,
            "aggregate not on distribution key should not pushdown"
        );
    }

    #[test]
    fn distributed_agg_distinct_not_pushdownable() {
        let opt = make_optimizer();
        let plan = RelExpr::Aggregate {
            group_by: vec![col("customer_id")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(col("product_id")),
                distinct: true,
                alias: Some("uniq".into()),
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_ne!(
            result.strategy,
            CitusStrategy::DistributedAggregation,
            "DISTINCT aggregates should not be pushed down"
        );
    }

    #[test]
    fn distributed_agg_count_star() {
        let opt = make_optimizer();
        let plan = RelExpr::Aggregate {
            group_by: vec![col("customer_id")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".into()),
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(
            result.strategy,
            CitusStrategy::DistributedAggregation,
        );
    }

    #[test]
    fn distributed_agg_multiple_functions() {
        let opt = make_optimizer();
        let plan = RelExpr::Aggregate {
            group_by: vec![col("customer_id")],
            aggregates: vec![
                AggregateExpr {
                    function: AggregateFunction::Sum,
                    arg: Some(col("amount")),
                    distinct: false,
                    alias: Some("total".into()),
                },
                AggregateExpr {
                    function: AggregateFunction::Min,
                    arg: Some(col("created_at")),
                    distinct: false,
                    alias: Some("first".into()),
                },
                AggregateExpr {
                    function: AggregateFunction::Max,
                    arg: Some(col("created_at")),
                    distinct: false,
                    alias: Some("last".into()),
                },
            ],
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(
            result.strategy,
            CitusStrategy::DistributedAggregation,
        );
    }

    // ------ Filter / shard pruning integration tests ------

    #[test]
    fn filter_with_shard_pruning_equality() {
        let opt = make_optimizer();
        let plan = RelExpr::Filter {
            predicate: eq(
                col("customer_id"),
                Expr::Const(Const::Int(42)),
            ),
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::SingleShardQuery);
        assert_eq!(result.execution, ExecutionLocation::SingleWorker);
        assert!(result.shard_pruning.is_some());
        let pruning = result.shard_pruning.as_ref().unwrap();
        assert!(pruning.is_single_shard);
    }

    #[test]
    fn filter_without_dist_key() {
        let opt = make_optimizer();
        let plan = RelExpr::Filter {
            predicate: eq(
                col("status"),
                Expr::Const(Const::String("active".into())),
            ),
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_ne!(result.strategy, CitusStrategy::SingleShardQuery);
        assert!(result.shard_pruning.is_none());
    }

    #[test]
    fn filter_with_range_pruning() {
        let opt = make_optimizer();
        let plan = RelExpr::Filter {
            predicate: gt(
                col("customer_id"),
                Expr::Const(Const::Int(1000)),
            ),
            input: Box::new(RelExpr::scan("orders")),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::ShardPruned);
        assert!(result.shard_pruning.is_some());
        let pruning = result.shard_pruning.as_ref().unwrap();
        assert!(!pruning.is_single_shard);
        assert!(pruning.shards_remaining < pruning.total_shards);
    }

    // ------ Scan annotation tests ------

    #[test]
    fn scan_distributed_table() {
        let opt = make_optimizer();
        let plan = RelExpr::scan("orders");
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::LocalScan);
        assert_eq!(result.execution, ExecutionLocation::Workers);
        assert!(matches!(
            result.distribution,
            DataDistribution::HashPartitioned { .. }
        ));
    }

    #[test]
    fn scan_reference_table() {
        let opt = make_optimizer();
        let plan = RelExpr::scan("countries");
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::LocalScan);
        assert_eq!(result.execution, ExecutionLocation::AllNodes);
        assert_eq!(
            result.distribution,
            DataDistribution::Replicated,
        );
    }

    #[test]
    fn scan_local_table() {
        let opt = make_optimizer();
        let plan = RelExpr::scan("config");
        let result = opt.optimize(&plan).expect("should succeed");
        assert_eq!(result.strategy, CitusStrategy::LocalScan);
        assert_eq!(result.execution, ExecutionLocation::Coordinator);
    }

    // ------ Cost estimation tests ------

    #[test]
    fn cost_colocated_join_is_zero() {
        let opt = make_optimizer();
        let plan = CitusOptimizedPlan {
            plan: RelExpr::scan("orders"),
            distribution: DataDistribution::Arbitrary,
            strategy: CitusStrategy::ColocatedJoin,
            shard_pruning: None,
            columnar_adjustment: None,
            execution: ExecutionLocation::Workers,
        };
        let cost = opt.estimate_cost(&plan);
        assert_eq!(cost, Cost::ZERO);
    }

    #[test]
    fn cost_reference_join_is_zero() {
        let opt = make_optimizer();
        let plan = CitusOptimizedPlan {
            plan: RelExpr::scan("orders"),
            distribution: DataDistribution::Arbitrary,
            strategy: CitusStrategy::ReferenceJoin,
            shard_pruning: None,
            columnar_adjustment: None,
            execution: ExecutionLocation::Workers,
        };
        let cost = opt.estimate_cost(&plan);
        assert_eq!(cost, Cost::ZERO);
    }

    #[test]
    fn cost_distributed_agg_has_network() {
        let opt = make_optimizer();
        let plan = CitusOptimizedPlan {
            plan: RelExpr::scan("orders"),
            distribution: DataDistribution::Arbitrary,
            strategy: CitusStrategy::DistributedAggregation,
            shard_pruning: None,
            columnar_adjustment: None,
            execution: ExecutionLocation::Workers,
        };
        let cost = opt.estimate_cost(&plan);
        assert!(cost.network > 0.0);
        assert!(cost.cpu > 0.0);
    }

    #[test]
    fn cost_shard_pruned_scales_with_selectivity() {
        let opt = make_optimizer();
        let full = CitusOptimizedPlan {
            plan: RelExpr::scan("orders"),
            distribution: DataDistribution::Arbitrary,
            strategy: CitusStrategy::ShardPruned,
            shard_pruning: Some(ShardPruningResult {
                shards_remaining: 32,
                total_shards: 32,
                is_single_shard: false,
            }),
            columnar_adjustment: None,
            execution: ExecutionLocation::Workers,
        };
        let pruned = CitusOptimizedPlan {
            plan: RelExpr::scan("orders"),
            distribution: DataDistribution::Arbitrary,
            strategy: CitusStrategy::ShardPruned,
            shard_pruning: Some(ShardPruningResult {
                shards_remaining: 8,
                total_shards: 32,
                is_single_shard: false,
            }),
            columnar_adjustment: None,
            execution: ExecutionLocation::Workers,
        };
        let full_cost = opt.estimate_cost(&full);
        let pruned_cost = opt.estimate_cost(&pruned);
        assert!(
            pruned_cost.cpu < full_cost.cpu,
            "pruned should be cheaper: pruned={}, full={}",
            pruned_cost.cpu,
            full_cost.cpu,
        );
    }

    #[test]
    fn cost_columnar_adjustment_applied() {
        let opt = make_optimizer();
        let without_col = CitusOptimizedPlan {
            plan: RelExpr::scan("orders"),
            distribution: DataDistribution::Arbitrary,
            strategy: CitusStrategy::LocalScan,
            shard_pruning: None,
            columnar_adjustment: None,
            execution: ExecutionLocation::Workers,
        };
        let with_col = CitusOptimizedPlan {
            plan: RelExpr::scan("orders"),
            distribution: DataDistribution::Arbitrary,
            strategy: CitusStrategy::LocalScan,
            shard_pruning: None,
            columnar_adjustment: Some(0.3),
            execution: ExecutionLocation::Workers,
        };
        let cost_without = opt.estimate_cost(&without_col);
        let cost_with = opt.estimate_cost(&with_col);
        assert!(
            cost_with.cpu < cost_without.cpu,
            "columnar adjustment should reduce CPU cost"
        );
    }

    // ------ Strategy property tests ------

    #[test]
    fn strategy_labels() {
        assert_eq!(
            CitusStrategy::ColocatedJoin.label(),
            "ColocatedJoin"
        );
        assert_eq!(
            CitusStrategy::ReferenceJoin.label(),
            "ReferenceJoin"
        );
        assert_eq!(
            CitusStrategy::DistributedAggregation.label(),
            "DistributedAggregation"
        );
    }

    #[test]
    fn strategy_network_transfer() {
        assert!(
            !CitusStrategy::ColocatedJoin
                .requires_network_transfer()
        );
        assert!(
            !CitusStrategy::ReferenceJoin
                .requires_network_transfer()
        );
        assert!(
            CitusStrategy::DistributedAggregation
                .requires_network_transfer()
        );
        assert!(
            CitusStrategy::GenericDistributed
                .requires_network_transfer()
        );
        assert!(
            !CitusStrategy::SingleShardQuery
                .requires_network_transfer()
        );
    }

    // ------ Error handling tests ------

    #[test]
    fn error_no_workers() {
        let config = CitusOptimizerConfig::default();
        let metadata = CitusMetadata::new(32);
        let opt = CitusOptimizer::new(config, metadata);
        let plan = RelExpr::scan("orders");
        let result = opt.optimize(&plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CitusOptimizerError::NoWorkerNodes,
        ));
    }

    // ------ Helper function tests ------

    #[test]
    fn extract_table_name_from_scan() {
        let name =
            extract_table_name(&RelExpr::scan("users"));
        assert_eq!(name, Some("users".to_owned()));
    }

    #[test]
    fn extract_table_name_through_filter() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(RelExpr::scan("orders")),
        };
        let name = extract_table_name(&plan);
        assert_eq!(name, Some("orders".to_owned()));
    }

    #[test]
    fn condition_equality_check() {
        let cond = eq(col("a"), col("b"));
        assert!(condition_has_equality_on(&cond, "a", "b"));
        assert!(condition_has_equality_on(&cond, "b", "a"));
        assert!(!condition_has_equality_on(&cond, "a", "c"));
    }

    #[test]
    fn condition_equality_in_and() {
        let cond = and(
            eq(col("a"), col("b")),
            gt(col("x"), Expr::Const(Const::Int(10))),
        );
        assert!(condition_has_equality_on(&cond, "a", "b"));
    }

    #[test]
    fn aggregates_pushdownable_basic() {
        let aggs = vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("amount")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            },
        ];
        assert!(aggregates_are_pushdownable(&aggs));
    }

    #[test]
    fn aggregates_not_pushdownable_distinct() {
        let aggs = vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(col("id")),
            distinct: true,
            alias: None,
        }];
        assert!(!aggregates_are_pushdownable(&aggs));
    }

    #[test]
    fn aggregates_not_pushdownable_stddev() {
        let aggs = vec![AggregateExpr {
            function: AggregateFunction::StdDev,
            arg: Some(col("val")),
            distinct: false,
            alias: None,
        }];
        assert!(!aggregates_are_pushdownable(&aggs));
    }

    #[test]
    fn aggregates_empty_is_pushdownable() {
        assert!(aggregates_are_pushdownable(&[]));
    }

    #[test]
    fn expr_references_column_simple() {
        assert!(expr_references_column(
            &col("customer_id"),
            "customer_id"
        ));
        assert!(!expr_references_column(
            &col("customer_id"),
            "order_id"
        ));
    }

    #[test]
    fn expr_references_column_in_binop() {
        let expr = eq(col("customer_id"), Expr::Const(Const::Int(1)));
        assert!(expr_references_column(&expr, "customer_id"));
        assert!(!expr_references_column(&expr, "other"));
    }

    // ------ Integration tests ------

    #[test]
    fn full_query_colocated_join_with_filter() {
        let opt = make_optimizer();
        let plan = RelExpr::Filter {
            predicate: eq(
                col("customer_id"),
                Expr::Const(Const::Int(42)),
            ),
            input: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: eq(
                    col("customer_id"),
                    col("customer_id"),
                ),
                left: Box::new(RelExpr::scan("orders")),
                right: Box::new(RelExpr::scan("order_items")),
            }),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        // The filter should detect shard pruning
        assert!(
            result.strategy == CitusStrategy::SingleShardQuery
                || result.shard_pruning.is_some()
        );
    }

    #[test]
    fn full_query_reference_join_with_agg() {
        let opt = make_optimizer();
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("country_code"), col("code")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("countries")),
        };
        let plan = RelExpr::Aggregate {
            group_by: vec![col("customer_id")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".into()),
            }],
            input: Box::new(join),
        };
        let result = opt.optimize(&plan).expect("should succeed");
        // Should detect distributed aggregation by customer_id
        assert_eq!(
            result.strategy,
            CitusStrategy::DistributedAggregation,
        );
    }
}
