//! Cardinality estimators for query optimization.
//!
//! Provides both heuristic and ML-based cardinality estimation.
//! The [`CardinalityEstimator`] trait abstracts over estimation
//! strategies, letting the optimizer swap implementations.

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::cost::{Cost, CostModel, StatisticsProvider};
use ra_core::statistics::Statistics;

use crate::features::FeatureSchema;
use crate::nn::FeedForwardNet;

/// A cardinality estimate for a relational expression.
#[derive(Debug, Clone, Copy)]
pub struct CardinalityEstimate {
    /// Estimated number of rows produced.
    pub rows: f64,
    /// Confidence in the estimate
    /// (0.0 = no confidence, 1.0 = certain).
    pub confidence: f64,
}

/// Trait for cardinality estimation strategies.
pub trait CardinalityEstimator: std::fmt::Debug + Send + Sync {
    /// Estimate the output cardinality of the given expression.
    fn estimate(
        &self,
        expr: &RelExpr,
        stats_provider: &dyn StatisticsProvider,
    ) -> CardinalityEstimate;
}

/// Traditional heuristic cardinality estimator.
///
/// Uses selectivity factors and independence assumptions typical
/// of classical database optimizers. Serves as a baseline.
#[derive(Debug)]
pub struct HeuristicEstimator;

impl CardinalityEstimator for HeuristicEstimator {
    fn estimate(
        &self,
        expr: &RelExpr,
        stats_provider: &dyn StatisticsProvider,
    ) -> CardinalityEstimate {
        let rows = estimate_heuristic(expr, stats_provider);
        CardinalityEstimate {
            rows,
            confidence: 0.3,
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Heuristic estimation requires handling many RelExpr variants"
)]
fn estimate_heuristic(expr: &RelExpr, stats: &dyn StatisticsProvider) -> f64 {
    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::ParallelScan { table, .. } => {
            stats.get_statistics(table).map_or(1000.0, |s| s.row_count)
        }
        RelExpr::Filter { input, .. } => {
            let input_rows = estimate_heuristic(input, stats);
            input_rows * Statistics::default_selectivity()
        }
        RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Gather { input, .. } => estimate_heuristic(input, stats),
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            let left_rows = estimate_heuristic(left, stats);
            let right_rows = estimate_heuristic(right, stats);
            estimate_join_cardinality(*join_type, left_rows, right_rows)
        }
        RelExpr::Aggregate {
            group_by, input, ..
        }
        | RelExpr::ParallelAggregate {
            group_by, input, ..
        } => {
            let input_rows = estimate_heuristic(input, stats);
            if group_by.is_empty() {
                1.0
            } else {
                (input_rows / 10.0).max(1.0)
            }
        }
        RelExpr::Limit {
            count,
            offset,
            input,
            ..
        } => {
            let input_rows = estimate_heuristic(input, stats);
            let available = (input_rows - *offset as f64).max(0.0);
            available.min(*count as f64)
        }
        RelExpr::Union { left, right, .. } => {
            let left_rows = estimate_heuristic(left, stats);
            let right_rows = estimate_heuristic(right, stats);
            left_rows + right_rows
        }
        RelExpr::Intersect { left, right, .. } => {
            let left_rows = estimate_heuristic(left, stats);
            let right_rows = estimate_heuristic(right, stats);
            left_rows.min(right_rows)
        }
        RelExpr::Except { left, right, .. } => {
            let left_rows = estimate_heuristic(left, stats);
            let right_rows = estimate_heuristic(right, stats);
            (left_rows - right_rows * 0.5).max(0.0)
        }
        RelExpr::CTE { body, .. } => estimate_heuristic(body, stats),
        RelExpr::Distinct { input, .. } => {
            let input_rows = estimate_heuristic(input, stats);
            (input_rows * 0.75).max(1.0)
        }
        RelExpr::RecursiveCTE {
            base_case, body, ..
        } => {
            let base_rows = estimate_heuristic(base_case, stats);
            let body_rows = estimate_heuristic(body, stats);
            // Rough heuristic: assume ~10 iterations
            (base_rows * 10.0).max(body_rows)
        }
        RelExpr::Values { rows, .. } => rows.len() as f64,
        RelExpr::Unnest { input, .. } => match input {
            Some(inp) => {
                let input_rows = estimate_heuristic(inp, stats);
                input_rows * 10.0 // assume avg array length of 10
            }
            None => 10.0,
        },
        RelExpr::MultiUnnest { exprs, .. } => exprs.len().max(1) as f64 * 10.0,
        RelExpr::TableFunction { input, .. } => match input {
            Some(inp) => estimate_heuristic(inp, stats),
            None => 100.0,
        },
        RelExpr::RowPattern { input, .. } => {
            let input_rows = estimate_heuristic(input, stats);
            // Pattern matching typically reduces output rows
            (input_rows * 0.1).max(1.0)
        }
        RelExpr::BitmapIndexScan { table, .. } => stats
            .get_statistics(table)
            .map_or(1000.0, |s| s.row_count * 0.1),
        RelExpr::BitmapAnd { inputs } => {
            let min_rows = inputs
                .iter()
                .map(|b| estimate_heuristic(b, stats))
                .min_by(f64::total_cmp)
                .unwrap_or(1.0);
            min_rows
        }
        RelExpr::BitmapOr { inputs } => {
            let total_rows: f64 = inputs.iter().map(|b| estimate_heuristic(b, stats)).sum();
            total_rows * 0.8 // account for overlap
        }
        RelExpr::BitmapHeapScan { bitmap, .. } => estimate_heuristic(bitmap, stats),
        RelExpr::ParallelHashJoin { left, right, .. } => {
            let left_rows = estimate_heuristic(left, stats);
            let right_rows = estimate_heuristic(right, stats);
            left_rows * right_rows * 0.1
        }
        RelExpr::IndexScan { table, .. } => {
            stats.get_statistics(table).map_or(1.0, |s| s.row_count)
        }
        RelExpr::MvScan { view_name, .. } => {
            stats.get_statistics(view_name).map_or(1.0, |s| s.row_count)
        }
        RelExpr::TopK { k, input, .. } => (*k as f64).min(estimate_heuristic(input, stats)),
        RelExpr::VectorFilter { input, .. } => {
            // Assume vector distance filter is selective (10% by default)
            estimate_heuristic(input, stats) * 0.1
        }
        // DML operators: estimate based on their source/affected rows
        RelExpr::Insert { source, .. } | RelExpr::Merge { source, .. } => {
            estimate_heuristic(source, stats)
        }
        RelExpr::Update { filter, .. } => {
            // Without a source table scan, use a default
            if filter.is_some() {
                100.0
            } else {
                1000.0
            }
        }
        RelExpr::Delete { filter, .. } => {
            if filter.is_some() {
                100.0
            } else {
                1000.0
            }
        }
        RelExpr::GraphTable { .. } => 1000.0,
    }
}

fn estimate_join_cardinality(join_type: JoinType, left_rows: f64, right_rows: f64) -> f64 {
    match join_type {
        JoinType::Inner => {
            let larger = left_rows.max(right_rows);
            let sel = if larger > 0.0 { 1.0 / larger } else { 0.1 };
            left_rows * right_rows * sel
        }
        JoinType::LeftOuter => left_rows.max(left_rows * 0.8),
        JoinType::RightOuter => right_rows.max(right_rows * 0.8),
        JoinType::FullOuter => left_rows + right_rows * 0.2,
        JoinType::Cross => left_rows * right_rows,
        JoinType::Semi | JoinType::Anti => left_rows * 0.5,
    }
}

/// ML-based cardinality estimator using a neural network.
///
/// Encodes query plans as feature vectors and feeds them through
/// a trained MLP to predict log-scaled row counts. This approach
/// learns correlations that heuristic estimators miss.
#[derive(Debug)]
pub struct MlEstimator {
    model: FeedForwardNet,
    schema: FeatureSchema,
    fallback: HeuristicEstimator,
}

impl MlEstimator {
    /// Create a new ML estimator with the given model and schema.
    #[must_use]
    pub fn new(model: FeedForwardNet, schema: FeatureSchema) -> Self {
        Self {
            model,
            schema,
            fallback: HeuristicEstimator,
        }
    }

    /// Build an ML estimator with a default (untrained) model
    /// for the given tables and columns. The model uses small
    /// deterministic weights; real accuracy requires loading
    /// trained weights.
    #[must_use]
    pub fn with_default_model(tables: &[&str], columns: &[&str]) -> Self {
        let schema = FeatureSchema::new(tables, columns);
        let model = crate::nn::build_default_mlp(&[schema.total_features, 64, 32, 1]);
        Self::new(model, schema)
    }
}

impl CardinalityEstimator for MlEstimator {
    fn estimate(
        &self,
        expr: &RelExpr,
        stats_provider: &dyn StatisticsProvider,
    ) -> CardinalityEstimate {
        let stats_map = collect_table_stats(expr, stats_provider);
        let features = self.schema.extract(expr, &stats_map);

        match self.model.forward(&features) {
            Ok(output) if !output.is_empty() => {
                let log_rows = output[0];
                let rows = 2.0_f64.powf(log_rows).max(1.0);
                CardinalityEstimate {
                    rows,
                    confidence: 0.7,
                }
            }
            _ => {
                tracing::warn!(
                    "ML model inference failed, \
                     using heuristic fallback"
                );
                self.fallback.estimate(expr, stats_provider)
            }
        }
    }
}

/// Walk the expression tree and collect statistics for all
/// referenced tables into a `HashMap`.
fn collect_table_stats(
    expr: &RelExpr,
    provider: &dyn StatisticsProvider,
) -> HashMap<String, Statistics> {
    let mut map = HashMap::new();
    collect_tables_recursive(expr, provider, &mut map);
    map
}

fn collect_tables_recursive(
    expr: &RelExpr,
    provider: &dyn StatisticsProvider,
    map: &mut HashMap<String, Statistics>,
) {
    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::IndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::BitmapIndexScan { table, .. }
        | RelExpr::ParallelScan { table, .. } => {
            if let Some(s) = provider.get_statistics(table) {
                map.insert(table.clone(), s.clone());
            }
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::RowPattern { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. }
        | RelExpr::TopK { input, .. }
        | RelExpr::VectorFilter { input, .. } => {
            collect_tables_recursive(input, provider, map);
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. }
        | RelExpr::ParallelHashJoin { left, right, .. } => {
            collect_tables_recursive(left, provider, map);
            collect_tables_recursive(right, provider, map);
        }
        RelExpr::CTE {
            definition, body, ..
        } => {
            collect_tables_recursive(definition, provider, map);
            collect_tables_recursive(body, provider, map);
        }
        RelExpr::RecursiveCTE {
            base_case,
            recursive_case,
            body,
            ..
        } => {
            collect_tables_recursive(base_case, provider, map);
            collect_tables_recursive(recursive_case, provider, map);
            collect_tables_recursive(body, provider, map);
        }
        RelExpr::Values { .. } | RelExpr::MultiUnnest { .. } | RelExpr::GraphTable { .. } => {}
        RelExpr::Unnest { input, .. } | RelExpr::TableFunction { input, .. } => {
            if let Some(inp) = input {
                collect_tables_recursive(inp, provider, map);
            }
        }
        RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
            for bitmap in inputs {
                collect_tables_recursive(bitmap, provider, map);
            }
        }
        RelExpr::BitmapHeapScan { bitmap, table, .. } => {
            if let Some(s) = provider.get_statistics(table) {
                map.insert(table.clone(), s.clone());
            }
            collect_tables_recursive(bitmap, provider, map);
        }
        RelExpr::MvScan { view_name, .. } => {
            if let Some(s) = provider.get_statistics(view_name) {
                map.insert(view_name.clone(), s.clone());
            }
        }
        RelExpr::Insert { source, .. } | RelExpr::Merge { source, .. } => {
            collect_tables_recursive(source, provider, map);
        }
        // GRAPH_TABLE references a property graph, not base tables
        // (folded into the Values/MultiUnnest no-op arm above).
        RelExpr::Update { from, .. } => {
            if let Some(f) = from {
                collect_tables_recursive(f, provider, map);
            }
        }
        RelExpr::Delete { using, .. } => {
            if let Some(u) = using {
                collect_tables_recursive(u, provider, map);
            }
        }
    }
}

/// Cost model that uses a [`CardinalityEstimator`] for row count
/// estimates instead of raw statistics lookups.
#[derive(Debug)]
pub struct MlCostModel<E: CardinalityEstimator> {
    estimator: E,
    cpu_per_row: f64,
    io_per_row: f64,
}

impl<E: CardinalityEstimator> MlCostModel<E> {
    /// Create a cost model backed by the given estimator.
    #[must_use]
    pub fn new(estimator: E) -> Self {
        Self {
            estimator,
            cpu_per_row: 0.01,
            io_per_row: 0.1,
        }
    }

    /// Set the CPU cost per row processed.
    #[must_use]
    pub fn with_cpu_per_row(mut self, cost: f64) -> Self {
        self.cpu_per_row = cost;
        self
    }

    /// Set the I/O cost per row processed.
    #[must_use]
    pub fn with_io_per_row(mut self, cost: f64) -> Self {
        self.io_per_row = cost;
        self
    }
}

impl<E: CardinalityEstimator> CostModel for MlCostModel<E> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "Cost calculation requires numeric conversions for model compatibility"
    )]
    fn estimate(&self, expr: &RelExpr, statistics: &dyn StatisticsProvider) -> Cost {
        let card = self.estimator.estimate(expr, statistics);
        let rows = card.rows;

        let cpu = rows * self.cpu_per_row;
        let io = rows * self.io_per_row;
        let memory = (rows * 64.0).min(u64::MAX as f64) as u64;

        Cost::new(cpu, io, 0.0, memory)
    }
}

/// Q-error metric for evaluating cardinality estimation accuracy.
///
/// Q-error = max(estimated/actual, actual/estimated).
/// A q-error of 1.0 means a perfect estimate.
#[must_use]
pub fn q_error(estimated: f64, actual: f64) -> f64 {
    let est = estimated.max(1.0);
    let act = actual.max(1.0);
    (est / act).max(act / est)
}

/// Compute summary statistics for a set of q-errors.
#[must_use]
pub fn q_error_summary(errors: &[f64]) -> QErrorSummary {
    if errors.is_empty() {
        return QErrorSummary {
            median: 0.0,
            p90: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
            mean: 0.0,
        };
    }

    let mut sorted = errors.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let n = sorted.len();
    let mean = sorted.iter().sum::<f64>() / n as f64;

    QErrorSummary {
        median: percentile(&sorted, 50.0),
        p90: percentile(&sorted, 90.0),
        p95: percentile(&sorted, 95.0),
        p99: percentile(&sorted, 99.0),
        max: sorted[n - 1],
        mean,
    }
}

/// Summary statistics for q-error measurements.
#[derive(Debug, Clone, Copy)]
pub struct QErrorSummary {
    /// Median q-error.
    pub median: f64,
    /// 90th percentile q-error.
    pub p90: f64,
    /// 95th percentile q-error.
    pub p95: f64,
    /// 99th percentile q-error.
    pub p99: f64,
    /// Maximum q-error.
    pub max: f64,
    /// Mean q-error.
    pub mean: f64,
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "Percentile calculation requires index conversion from f64"
)]
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// A simple in-memory statistics provider for testing.
#[derive(Debug)]
pub struct SimpleStatsProvider {
    stats: HashMap<String, Statistics>,
}

impl SimpleStatsProvider {
    /// Create an empty provider.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stats: HashMap::new(),
        }
    }

    /// Add statistics for a table.
    pub fn add(&mut self, table: &str, statistics: Statistics) {
        self.stats.insert(table.to_string(), statistics);
    }
}

impl Default for SimpleStatsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl StatisticsProvider for SimpleStatsProvider {
    fn get_statistics(&self, table: &str) -> Option<&Statistics> {
        self.stats.get(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp as ExprBinOp, ColumnRef, Const, Expr};

    fn setup_provider() -> SimpleStatsProvider {
        let mut provider = SimpleStatsProvider::new();
        provider.add("users", Statistics::new(1000.0));
        provider.add("orders", Statistics::new(5000.0));
        provider
    }

    #[test]
    fn heuristic_scan() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::scan("users");
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_filter() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: ExprBinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(42))),
        });
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_join_inner() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: ExprBinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("users", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("orders", "user_id"))),
            },
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
        assert!(card.rows <= 1000.0 * 5000.0);
    }

    #[test]
    fn heuristic_limit() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::scan("users").limit(10, 0);
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_aggregate_no_group() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_unknown_table() {
        let est = HeuristicEstimator;
        let provider = SimpleStatsProvider::new();
        let expr = RelExpr::scan("unknown");
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ml_estimator_produces_positive() {
        let ml = MlEstimator::with_default_model(
            &["users", "orders"],
            &["id", "name", "amount", "user_id"],
        );
        let provider = setup_provider();
        let expr = RelExpr::scan("users");
        let card = ml.estimate(&expr, &provider);
        assert!(card.rows >= 1.0);
    }

    #[test]
    fn ml_cost_model() {
        let ml = MlEstimator::with_default_model(&["users"], &["id"]);
        let cost_model = MlCostModel::new(ml);
        let provider = setup_provider();
        let expr = RelExpr::scan("users");
        let cost = cost_model.estimate(&expr, &provider);
        assert!(cost.cpu > 0.0);
        assert!(cost.io > 0.0);
    }

    #[test]
    fn q_error_perfect() {
        let err = q_error(100.0, 100.0);
        assert!((err - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_overestimate() {
        let err = q_error(200.0, 100.0);
        assert!((err - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_underestimate() {
        let err = q_error(50.0, 100.0);
        assert!((err - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_summary_single() {
        let summary = q_error_summary(&[2.0]);
        assert!((summary.median - 2.0).abs() < f64::EPSILON);
        assert!((summary.mean - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_summary_empty() {
        let summary = q_error_summary(&[]);
        assert!((summary.median - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_summary_multiple_basic() {
        let errors = vec![1.0, 1.5, 2.0, 3.0, 10.0];
        let summary = q_error_summary(&errors);
        assert!((summary.median - 2.0).abs() < f64::EPSILON);
        assert!((summary.max - 10.0).abs() < f64::EPSILON);
        assert!(summary.p90 >= summary.median);
        assert!(summary.p99 >= summary.p90);
    }

    #[test]
    fn simple_stats_provider_default() {
        let provider = SimpleStatsProvider::default();
        assert!(provider.get_statistics("foo").is_none());
    }

    #[test]
    fn heuristic_sort_preserves_cardinality() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_project_preserves_cardinality() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Project {
            columns: vec![],
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ml_cost_model_custom_rates() {
        let ml = MlEstimator::with_default_model(&["users"], &["id"]);
        let cost_model = MlCostModel::new(ml)
            .with_cpu_per_row(0.5)
            .with_io_per_row(1.0);
        let provider = setup_provider();
        let expr = RelExpr::scan("users");
        let cost = cost_model.estimate(&expr, &provider);
        assert!(cost.cpu > 0.0);
        assert!(cost.io >= cost.cpu);
    }

    #[test]
    fn collect_table_stats_from_join() {
        let provider = setup_provider();
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let map = collect_table_stats(&expr, &provider);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("users"));
        assert!(map.contains_key("orders"));
    }

    // === Additional comprehensive tests for missing coverage ===

    #[test]
    fn heuristic_join_left() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows >= 1000.0); // At least left table size
    }

    #[test]
    fn heuristic_join_right() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Join {
            join_type: JoinType::RightOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows >= 5000.0); // At least right table size
    }

    #[test]
    fn heuristic_join_full() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Join {
            join_type: JoinType::FullOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_join_cross() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 5_000_000.0).abs() < 1.0); // 1000 * 5000
    }

    #[test]
    fn heuristic_aggregate_with_group() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("status"))],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 1.0);
        assert!(card.rows < 5000.0);
    }

    #[test]
    fn heuristic_project() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Project {
            columns: vec![],
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_sort() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_incremental_sort() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::IncrementalSort {
            prefix_keys: vec![],
            suffix_keys: vec![],
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_union() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 6000.0).abs() < f64::EPSILON); // 1000 + 5000
    }

    #[test]
    fn heuristic_intersect() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Intersect {
            all: false,
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON); // min of the two
    }

    #[test]
    fn heuristic_except() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Except {
            all: false,
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
        assert!(card.rows <= 5000.0);
    }

    #[test]
    fn heuristic_cte() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::CTE {
            name: "temp".into(),
            definition: Box::new(RelExpr::scan("users")),
            body: Box::new(RelExpr::scan("temp")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_window() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Window {
            functions: vec![],
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_distinct() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Distinct {
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
        assert!(card.rows <= 1000.0);
    }

    #[test]
    fn heuristic_recursive_cte() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::RecursiveCTE {
            name: "tree".into(),
            base_case: Box::new(RelExpr::scan("users")),
            recursive_case: Box::new(RelExpr::scan("users")),
            body: Box::new(RelExpr::scan("tree")),
            cycle_detection: None,
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows >= 1000.0);
    }

    #[test]
    fn heuristic_values() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Values {
            rows: vec![
                vec![Expr::Const(Const::Int(1))],
                vec![Expr::Const(Const::Int(2))],
                vec![Expr::Const(Const::Int(3))],
            ],
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_unnest_with_input() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("items")),
            alias: None,
            input: Some(Box::new(RelExpr::scan("users"))),
            with_ordinality: false,
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows >= 1000.0);
    }

    #[test]
    fn heuristic_unnest_no_input() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("items")),
            alias: None,
            input: None,
            with_ordinality: false,
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_multi_unnest() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::MultiUnnest {
            exprs: vec![
                Expr::Column(ColumnRef::new("arr1")),
                Expr::Column(ColumnRef::new("arr2")),
            ],
            aliases: vec![None, None],
            with_ordinality: false,
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_table_function_with_input() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::TableFunction {
            name: "generate_series".into(),
            args: vec![],
            columns: vec![],
            input: Some(Box::new(RelExpr::scan("users"))),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_table_function_no_input() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::TableFunction {
            name: "generate_series".into(),
            args: vec![],
            columns: vec![],
            input: None,
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_row_pattern() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::RowPattern {
            input: Box::new(RelExpr::scan("users")),
            partition_by: vec![],
            order_by: vec![],
            pattern: ra_core::row_pattern::PatternExpr::Var("A".to_string()),
            defines: vec![],
            measures: vec![],
            mode: ra_core::row_pattern::MatchMode::OneRowPerMatch,
            skip_mode: ra_core::row_pattern::SkipMode::PastLastRow,
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
        assert!(card.rows < 1000.0);
    }

    #[test]
    fn heuristic_bitmap_index_scan() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::BitmapIndexScan {
            table: "users".into(),
            index: "idx".into(),
            predicate: Expr::Const(Const::Bool(true)),
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
        assert!(card.rows <= 1000.0);
    }

    #[test]
    fn heuristic_bitmap_and() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::BitmapAnd {
            inputs: vec![
                Box::new(RelExpr::scan("users")),
                Box::new(RelExpr::scan("orders")),
            ],
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_bitmap_or() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::BitmapOr {
            inputs: vec![
                Box::new(RelExpr::scan("users")),
                Box::new(RelExpr::scan("orders")),
            ],
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_bitmap_heap_scan() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::BitmapHeapScan {
            bitmap: Box::new(RelExpr::scan("users")),
            table: "users".into(),
            recheck_cond: None,
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_parallel_scan() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::ParallelScan {
            table: "users".into(),
            workers: 4,
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_parallel_hash_join() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::ParallelHashJoin {
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
            join_type: ra_core::JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            workers: 4,
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_parallel_aggregate() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::ParallelAggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("users")),
            workers: 4,
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_gather() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Gather {
            input: Box::new(RelExpr::scan("users")),
            workers: 4,
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_mv_scan() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::MvScan {
            view_name: "mv_users".into(),
            alias: None,
        };
        let card = est.estimate(&expr, &provider);
        assert!(card.rows > 0.0);
    }

    #[test]
    fn heuristic_index_only_scan() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::IndexOnlyScan {
            table: "users".into(),
            index: "idx_email".into(),
            predicate: Expr::Const(Const::Bool(true)),
            columns: vec![],
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_limit_with_offset() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Limit {
            count: 50,
            offset: 100,
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_limit_exceeds_available() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Limit {
            count: 2000,
            offset: 0,
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn heuristic_offset_exceeds_input() {
        let est = HeuristicEstimator;
        let provider = setup_provider();
        let expr = RelExpr::Limit {
            count: 10,
            offset: 2000,
            input: Box::new(RelExpr::scan("users")),
        };
        let card = est.estimate(&expr, &provider);
        assert!((card.rows - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_handles_zeros() {
        let err = q_error(0.0, 100.0);
        assert!(err > 0.0);
    }

    #[test]
    fn q_error_summary_multiple() {
        let errors = vec![1.5, 2.0, 3.0, 4.0, 10.0];
        let summary = q_error_summary(&errors);
        assert!(summary.median > 0.0);
        assert!(summary.mean > 0.0);
        assert!(summary.p90 >= summary.median);
        assert!(summary.max >= summary.p90);
    }
}
