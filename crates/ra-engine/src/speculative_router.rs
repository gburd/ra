//! Speculative optimization routing using BitNet prediction.
//!
//! Replaces the hard-coded `QueryComplexity` heuristic with a trained
//! predictor that routes queries to the optimal optimization path
//! (skip / left-deep / e-graph) and dynamically caps e-graph budgets.
//!
//! Inspired by Google's DFlash speculative decoding architecture: a
//! lightweight model (~87ns BitNet forward pass) makes an O(1) prediction
//! about the optimization strategy, then verification gates confirm
//! whether continued computation is worthwhile.

use std::sync::Arc;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, Expr};

use crate::cost_model::BitNetCostModel;
use crate::join_graph::JoinGraph;

/// Optimization route predicted by the speculative model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptRoute {
    /// Query is trivial — return unchanged (no optimization needed).
    Skip,
    /// Use heuristic left-deep join ordering (no e-graph).
    LeftDeep,
    /// E-graph with low iteration budget (3 iters, ~1ms).
    EGraphLow,
    /// E-graph with medium budget (8 iters, ~5ms).
    EGraphMedium,
    /// E-graph with high budget (15 iters, ~15ms).
    EGraphHigh,
}

impl OptRoute {
    /// Iteration limit for this route.
    #[must_use]
    pub fn iter_limit(self) -> usize {
        match self {
            Self::Skip | Self::LeftDeep => 0,
            Self::EGraphLow => 3,
            Self::EGraphMedium => 8,
            Self::EGraphHigh => 15,
        }
    }

    /// Timeout in milliseconds for this route.
    #[must_use]
    pub fn timeout_ms(self) -> u64 {
        match self {
            Self::Skip | Self::LeftDeep => 0,
            Self::EGraphLow => 5,
            Self::EGraphMedium => 15,
            Self::EGraphHigh => 50,
        }
    }
}

/// Prediction from the speculative routing model.
#[derive(Debug, Clone)]
pub struct RoutePrediction {
    /// Predicted optimal route.
    pub route: OptRoute,
    /// Confidence in prediction (0.0 - 1.0).
    pub confidence: f32,
    /// Predicted number of e-graph iterations needed.
    pub predicted_iterations_needed: u8,
    /// Predicted cost improvement percentage from optimization.
    pub predicted_cost_improvement_pct: f32,
}

/// Extended features for optimization difficulty prediction.
///
/// 16-dimensional input vector that replaces 12D `QueryFeatures` for
/// routing decisions. The extra 4 dimensions capture join graph topology,
/// selectivity signals, and scale information that are highly predictive
/// of optimization difficulty.
#[derive(Debug, Clone)]
pub struct OptimizationFeatures {
    // ─── Structural (from existing QueryFeatures, dims 0-5) ───
    pub table_count: f32,
    pub join_count: f32,
    pub filter_count: f32,
    pub aggregate_count: f32,
    pub subquery_count: f32,
    pub window_count: f32,

    // ─── Join graph topology (dims 6-9) ───
    /// Fraction of possible edges that exist: edges / (n*(n-1)/2).
    pub join_graph_density: f32,
    /// Maximum degree of any table node in the join graph.
    pub max_join_fan_out: f32,
    /// Fraction of joins that are equi-joins (= predicates).
    pub equi_join_fraction: f32,
    /// 1.0 if any cross/theta joins exist, 0.0 otherwise.
    pub cross_join_present: f32,

    // ─── Selectivity signals (dims 10-12) ───
    /// Geometric mean selectivity of filter predicates.
    pub avg_predicate_selectivity: f32,
    /// 1.0 if query has LIMIT clause, 0.0 otherwise.
    pub has_limit: f32,
    /// 1.0 if query has DISTINCT or GROUP BY.
    pub has_distinct_or_group: f32,

    // ─── Scale (dims 13-15) ───
    /// log10 of estimated output rows.
    pub log_estimated_rows: f32,
    /// Sum of table sizes (in pages) for all referenced tables.
    pub total_table_pages: f32,
    /// Fraction of joins that have usable indexes.
    pub index_coverage: f32,
}

impl OptimizationFeatures {
    /// Number of feature dimensions.
    pub const DIM: usize = 16;

    /// Convert to array for model input.
    #[must_use]
    pub fn as_array(&self) -> [f32; Self::DIM] {
        [
            self.table_count,
            self.join_count,
            self.filter_count,
            self.aggregate_count,
            self.subquery_count,
            self.window_count,
            self.join_graph_density,
            self.max_join_fan_out,
            self.equi_join_fraction,
            self.cross_join_present,
            self.avg_predicate_selectivity,
            self.has_limit,
            self.has_distinct_or_group,
            self.log_estimated_rows,
            self.total_table_pages,
            self.index_coverage,
        ]
    }

    /// Extract optimization features from a relational expression.
    #[must_use]
    pub fn from_expr(expr: &RelExpr) -> Self {
        let join_graph = JoinGraph::from_expr(expr);
        Self::from_expr_with_graph(expr, &join_graph)
    }

    /// Extract features using a pre-computed join graph.
    #[must_use]
    pub fn from_expr_with_graph(expr: &RelExpr, join_graph: &JoinGraph) -> Self {
        let stats = join_graph.stats();

        // Structural counts
        let mut counter = StructuralCounter::default();
        counter.visit(expr);

        // Join topology
        let join_graph_density = stats.density() as f32;
        let max_join_fan_out = compute_max_fan_out(join_graph);
        let equi_join_fraction = compute_equi_join_fraction(expr, &counter);
        let cross_join_present = if counter.cross_join_count > 0 {
            1.0
        } else {
            0.0
        };

        // Selectivity signals
        let avg_predicate_selectivity =
            estimate_avg_selectivity(counter.filter_count, counter.join_count);

        OptimizationFeatures {
            table_count: counter.table_count as f32,
            join_count: counter.join_count as f32,
            filter_count: counter.filter_count as f32,
            aggregate_count: counter.aggregate_count as f32,
            subquery_count: counter.subquery_count as f32,
            window_count: counter.window_count as f32,
            join_graph_density,
            max_join_fan_out,
            equi_join_fraction,
            cross_join_present,
            avg_predicate_selectivity,
            has_limit: if counter.has_limit { 1.0 } else { 0.0 },
            has_distinct_or_group: if counter.has_distinct || counter.has_group_by {
                1.0
            } else {
                0.0
            },
            log_estimated_rows: estimate_log_rows(&counter),
            total_table_pages: 0.0, // Requires table stats; filled by caller
            index_coverage: 0.0,    // Requires index metadata; filled by caller
        }
    }

    /// Update scale features with table statistics.
    pub fn with_table_stats(
        mut self,
        table_stats: &std::collections::HashMap<String, ra_core::statistics::Statistics>,
    ) -> Self {
        if table_stats.is_empty() {
            return self;
        }

        let mut total_pages: f64 = 0.0;
        let mut total_rows_log: f64 = 0.0;
        let mut table_count = 0u32;

        // Estimate pages from total_size / 8192 (PostgreSQL page size)
        const PAGE_SIZE: f64 = 8192.0;
        for stats in table_stats.values() {
            total_pages += stats.total_size as f64 / PAGE_SIZE;
            if stats.row_count > 0.0 {
                total_rows_log += stats.row_count.log10();
                table_count += 1;
            }
        }

        self.total_table_pages = (total_pages as f32).min(1e6);

        if table_count > 0 {
            let avg_log_rows = total_rows_log / f64::from(table_count);
            // Scale by join count: each join multiplies (but predicates reduce)
            let join_factor = (1.0 + self.join_count as f64 * 0.3).min(4.0);
            self.log_estimated_rows = (avg_log_rows * join_factor).min(12.0) as f32;
        }

        self
    }
}

/// The speculative router uses a dedicated BitNet head to predict
/// the optimal optimization route in O(1) time.
pub struct SpeculativeRouter {
    model: Arc<BitNetCostModel>,
}

impl std::fmt::Debug for SpeculativeRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeculativeRouter")
            .field("model_samples", &self.model.samples_trained)
            .finish()
    }
}

impl SpeculativeRouter {
    /// Create a router with a trained model.
    #[must_use]
    pub fn new(model: Arc<BitNetCostModel>) -> Self {
        Self { model }
    }

    /// Predict route from optimization features using the BitNet model.
    ///
    /// Uses output dimensions 12-15 of the 16D prediction:
    /// - dim 12: difficulty score (0 = trivial, 1 = hard)
    /// - dim 13: predicted iterations needed (scaled)
    /// - dim 14: predicted improvement potential
    /// - dim 15: confidence
    #[must_use]
    pub fn predict(&self, features: &OptimizationFeatures) -> RoutePrediction {
        // The model expects 12D input; we pack the first 12 dims
        // and use the topology signals to post-adjust.
        let input: [f32; 12] = [
            features.table_count,
            features.join_count,
            features.filter_count,
            features.aggregate_count,
            features.subquery_count,
            features.window_count,
            features.join_graph_density,
            features.max_join_fan_out,
            features.equi_join_fraction,
            features.cross_join_present,
            features.avg_predicate_selectivity,
            features.has_limit,
        ];

        let output = self.model.predict_all(&input);

        // Interpret output dims for routing
        let difficulty = output[12].clamp(0.0, 1.0);
        let predicted_iters = (output[13] * 20.0).clamp(0.0, 20.0);
        let improvement_pct = output[14].clamp(0.0, 100.0);
        let confidence = output[15].clamp(0.0, 1.0);

        // Route based on difficulty and topology signals
        let route = self.classify_route(features, difficulty, predicted_iters);

        RoutePrediction {
            route,
            confidence,
            predicted_iterations_needed: predicted_iters as u8,
            predicted_cost_improvement_pct: improvement_pct,
        }
    }

    /// Classify route using model output + topology heuristics.
    fn classify_route(
        &self,
        features: &OptimizationFeatures,
        difficulty: f32,
        predicted_iters: f32,
    ) -> OptRoute {
        // Strong signal: all equi-joins with good density → left-deep
        if features.equi_join_fraction >= 0.95
            && features.cross_join_present < 0.5
            && features.table_count >= 2.0
            && features.table_count <= 7.0
            && difficulty < 0.4
        {
            return OptRoute::LeftDeep;
        }

        // Single table or trivial
        if features.table_count <= 1.0 || difficulty < 0.1 {
            return OptRoute::Skip;
        }

        // Route by predicted iterations
        match predicted_iters as u8 {
            0 => OptRoute::Skip,
            1 => OptRoute::LeftDeep,
            2..=3 => OptRoute::EGraphLow,
            4..=8 => OptRoute::EGraphMedium,
            _ => OptRoute::EGraphHigh,
        }
    }

    /// Heuristic fallback when no trained model is available.
    ///
    /// Reproduces the current `QueryComplexity` logic but enhanced with
    /// topology signals for better left-deep routing.
    #[must_use]
    pub fn heuristic_fallback(features: &OptimizationFeatures) -> RoutePrediction {
        let table_count = features.table_count as usize;
        let join_count = features.join_count as usize;

        // Skip: single table or no meaningful optimization possible
        if table_count <= 1 {
            return RoutePrediction {
                route: OptRoute::Skip,
                confidence: 0.95,
                predicted_iterations_needed: 0,
                predicted_cost_improvement_pct: 0.0,
            };
        }

        // Left-deep: equi-join chains with 2-7 tables and good density
        // This is the key insight from the plan — equi-join chains are
        // trivially solved by left-deep ordering in <1ms.
        if table_count >= 2
            && table_count <= 7
            && features.equi_join_fraction >= 0.8
            && features.cross_join_present < 0.5
            && features.subquery_count < 1.0
            && features.window_count < 1.0
        {
            return RoutePrediction {
                route: OptRoute::LeftDeep,
                confidence: 0.85,
                predicted_iterations_needed: 0,
                predicted_cost_improvement_pct: 5.0,
            };
        }

        // E-graph routing by complexity (matching QueryComplexity logic)
        let (route, iters, improvement) = match table_count {
            2..=4 => {
                if join_count > 3
                    || features.subquery_count > 0.0
                    || features.cross_join_present > 0.5
                {
                    (OptRoute::EGraphMedium, 8, 15.0)
                } else {
                    (OptRoute::EGraphLow, 3, 8.0)
                }
            }
            5..=7 => (OptRoute::EGraphMedium, 8, 20.0),
            8..=9 => (OptRoute::EGraphHigh, 15, 25.0),
            _ => (OptRoute::EGraphHigh, 15, 30.0),
        };

        RoutePrediction {
            route,
            confidence: 0.6,
            predicted_iterations_needed: iters,
            predicted_cost_improvement_pct: improvement,
        }
    }
}

// ─── Feature extraction helpers ───

/// Counts structural elements during expression tree traversal.
#[derive(Debug, Default)]
struct StructuralCounter {
    table_count: u32,
    join_count: u32,
    filter_count: u32,
    aggregate_count: u32,
    subquery_count: u32,
    window_count: u32,
    cross_join_count: u32,
    equi_join_count: u32,
    non_equi_join_count: u32,
    has_limit: bool,
    has_distinct: bool,
    has_group_by: bool,
}

impl StructuralCounter {
    fn visit(&mut self, expr: &RelExpr) {
        match expr {
            RelExpr::Scan { .. }
            | RelExpr::IndexScan { .. }
            | RelExpr::IndexOnlyScan { .. }
            | RelExpr::BitmapHeapScan { .. }
            | RelExpr::ParallelScan { .. }
            | RelExpr::MvScan { .. } => {
                self.table_count += 1;
            }

            RelExpr::Filter { input, predicate } => {
                self.filter_count += count_predicates(predicate);
                self.visit(input);
            }

            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
                ..
            } => {
                self.join_count += 1;
                self.classify_join(join_type, condition);
                self.visit(left);
                self.visit(right);
            }

            RelExpr::Aggregate {
                input, group_by, ..
            } => {
                self.aggregate_count += 1;
                if !group_by.is_empty() {
                    self.has_group_by = true;
                }
                self.visit(input);
            }

            RelExpr::Window { input, .. } => {
                self.window_count += 1;
                self.visit(input);
            }

            RelExpr::Limit { input, .. } => {
                self.has_limit = true;
                self.visit(input);
            }

            RelExpr::Distinct { input } => {
                self.has_distinct = true;
                self.visit(input);
            }

            RelExpr::CTE {
                definition, body, ..
            } => {
                self.subquery_count += 1;
                self.visit(definition);
                self.visit(body);
            }

            RelExpr::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => {
                self.subquery_count += 1;
                self.visit(base_case);
                self.visit(recursive_case);
                self.visit(body);
            }

            RelExpr::Project { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Gather { input, .. }
            | RelExpr::TopK { input, .. }
            | RelExpr::VectorFilter { input, .. }
            | RelExpr::IncrementalSort { input, .. }
            | RelExpr::ParallelAggregate { input, .. }
            | RelExpr::RowPattern { input, .. } => {
                self.visit(input);
            }

            RelExpr::ParallelHashJoin {
                left, right, ..
            }
            | RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }

            RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
                for inp in inputs {
                    self.visit(inp);
                }
            }

            RelExpr::Unnest { input, .. } | RelExpr::TableFunction { input, .. } => {
                if let Some(inp) = input {
                    self.visit(inp);
                }
            }

            RelExpr::BitmapIndexScan { .. } => {
                self.table_count += 1;
            }

            RelExpr::Values { .. } | RelExpr::MultiUnnest { .. } => {}
        }
    }

    fn classify_join(&mut self, join_type: &JoinType, condition: &Expr) {
        // Cross join detection
        if matches!(join_type, JoinType::Cross) || is_trivial_condition(condition) {
            self.cross_join_count += 1;
            return;
        }

        // Equi-join detection
        if is_equi_join_condition(condition) {
            self.equi_join_count += 1;
        } else {
            self.non_equi_join_count += 1;
        }
    }
}

/// Count predicate conjuncts.
fn count_predicates(expr: &Expr) -> u32 {
    match expr {
        Expr::BinOp {
            op: BinOp::And,
            left,
            right,
        } => count_predicates(left) + count_predicates(right),
        Expr::Const(_) => 0,
        _ => 1,
    }
}

/// Check if condition is trivially true (cross join).
fn is_trivial_condition(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Const(ra_core::expr::Const::Bool(true))
    )
}

/// Check if condition is an equi-join (column = column).
fn is_equi_join_condition(expr: &Expr) -> bool {
    match expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left,
            right,
        } => matches!(left.as_ref(), Expr::Column(_)) && matches!(right.as_ref(), Expr::Column(_)),
        Expr::BinOp {
            op: BinOp::And,
            left,
            right,
        } => is_equi_join_condition(left) && is_equi_join_condition(right),
        _ => false,
    }
}

/// Compute max fan-out (degree) of any node in the join graph.
fn compute_max_fan_out(join_graph: &JoinGraph) -> f32 {
    let tables = join_graph.tables();
    if tables.len() <= 1 {
        return 0.0;
    }

    let mut max_degree: u32 = 0;
    let table_vec: Vec<&String> = tables.iter().collect();

    for &table in &table_vec {
        let mut degree: u32 = 0;
        for &other in &table_vec {
            if table != other && join_graph.can_join(table, other) {
                degree += 1;
            }
        }
        max_degree = max_degree.max(degree);
    }

    max_degree as f32
}

/// Compute the fraction of joins that are equi-joins.
fn compute_equi_join_fraction(expr: &RelExpr, counter: &StructuralCounter) -> f32 {
    let _ = expr; // Used indirectly via counter
    let total = counter.equi_join_count + counter.non_equi_join_count + counter.cross_join_count;
    if total == 0 {
        return 1.0; // No joins = all "equi" (vacuously true)
    }
    counter.equi_join_count as f32 / total as f32
}

/// Estimate average predicate selectivity using a simple heuristic.
///
/// Without actual statistics, we use the "rule of 1/10" heuristic:
/// - Equality predicates: ~0.1
/// - Range predicates: ~0.3
/// - Each additional predicate reduces by its selectivity (AND)
fn estimate_avg_selectivity(filter_count: u32, join_count: u32) -> f32 {
    if filter_count == 0 && join_count == 0 {
        return 1.0; // No predicates = no selectivity
    }

    // Geometric mean of per-predicate selectivities
    let total_predicates = filter_count + join_count;
    let per_predicate_selectivity = 0.1_f32; // equi-join heuristic
    per_predicate_selectivity.powi(total_predicates.min(8) as i32).max(1e-6)
}

/// Estimate log10(output rows) from structural features.
fn estimate_log_rows(counter: &StructuralCounter) -> f32 {
    // Rough heuristic: base cardinality grows with tables, reduced by predicates
    let base = (counter.table_count as f32) * 3.0; // ~1000 rows per table
    let reduction = (counter.filter_count as f32) * 0.5;
    let limit_cap = if counter.has_limit { 2.0 } else { 0.0 };

    if counter.has_limit {
        limit_cap
    } else {
        (base - reduction).max(1.0).min(9.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::JoinType;
    use ra_core::expr::{ColumnRef, Const};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_string(),
            alias: None,
        }
    }

    fn equi_join(left: RelExpr, right: RelExpr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "id"))),
            },
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn heuristic_routes_single_table_to_skip() {
        let expr = scan("users");
        let features = OptimizationFeatures::from_expr(&expr);
        let prediction = SpeculativeRouter::heuristic_fallback(&features);
        assert_eq!(prediction.route, OptRoute::Skip);
        assert!(prediction.confidence > 0.9);
    }

    #[test]
    fn heuristic_routes_equi_join_chain_to_left_deep() {
        // 6-table equi-join chain (like TPC-H Q5)
        let expr = equi_join(
            equi_join(
                equi_join(
                    equi_join(equi_join(scan("t1"), scan("t2")), scan("t3")),
                    scan("t4"),
                ),
                scan("t5"),
            ),
            scan("t6"),
        );
        let features = OptimizationFeatures::from_expr(&expr);
        let prediction = SpeculativeRouter::heuristic_fallback(&features);
        assert_eq!(
            prediction.route,
            OptRoute::LeftDeep,
            "6-table equi-join chain should route to LeftDeep, got {:?}",
            prediction.route
        );
    }

    #[test]
    fn heuristic_routes_complex_query_to_egraph() {
        // 3 tables with cross join → needs e-graph
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(equi_join(scan("t1"), scan("t2"))),
            right: Box::new(scan("t3")),
        };
        let features = OptimizationFeatures::from_expr(&expr);
        let prediction = SpeculativeRouter::heuristic_fallback(&features);
        assert!(
            matches!(
                prediction.route,
                OptRoute::EGraphLow | OptRoute::EGraphMedium | OptRoute::EGraphHigh
            ),
            "Cross-join query should route to e-graph, got {:?}",
            prediction.route
        );
    }

    #[test]
    fn optimization_features_dimension() {
        let expr = equi_join(scan("a"), scan("b"));
        let features = OptimizationFeatures::from_expr(&expr);
        let array = features.as_array();
        assert_eq!(array.len(), OptimizationFeatures::DIM);
    }

    #[test]
    fn equi_join_fraction_correct() {
        let expr = equi_join(equi_join(scan("a"), scan("b")), scan("c"));
        let features = OptimizationFeatures::from_expr(&expr);
        assert!(
            features.equi_join_fraction >= 0.99,
            "All equi-joins should give fraction ~1.0, got {}",
            features.equi_join_fraction
        );
    }

    #[test]
    fn cross_join_detected() {
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let features = OptimizationFeatures::from_expr(&expr);
        assert!(
            features.cross_join_present > 0.5,
            "Cross join should be detected"
        );
    }

    #[test]
    fn route_iter_limits() {
        assert_eq!(OptRoute::Skip.iter_limit(), 0);
        assert_eq!(OptRoute::LeftDeep.iter_limit(), 0);
        assert_eq!(OptRoute::EGraphLow.iter_limit(), 3);
        assert_eq!(OptRoute::EGraphMedium.iter_limit(), 8);
        assert_eq!(OptRoute::EGraphHigh.iter_limit(), 15);
    }
}
