//! Large join graph optimization fallback.
//!
//! Provides heuristic strategies for optimizing queries with many tables
//! (10+) where e-graph equality saturation becomes too expensive.
//! Implements greedy join ordering and simulated annealing as alternatives
//! to exhaustive optimization.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use rand::prelude::*;
use ra_core::{
    algebra::{JoinType, RelExpr},
    cost::{CostModel, StatisticsProvider},
};
use serde::{Deserialize, Serialize};

/// Strategy for optimizing large join graphs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LargeJoinStrategy {
    /// Continue using e-graph (may timeout).
    EGraph,
    /// Greedy join ordering heuristic.
    Greedy,
    /// Simulated annealing optimization.
    SimulatedAnnealing {
        /// Initial temperature for annealing.
        initial_temp: f64,
        /// Rate at which temperature decreases (0 < rate < 1).
        cooling_rate: f64,
        /// Maximum number of iterations.
        max_iterations: usize,
    },
}

impl Default for LargeJoinStrategy {
    fn default() -> Self {
        Self::SimulatedAnnealing {
            initial_temp: 1000.0,
            cooling_rate: 0.95,
            max_iterations: 10000,
        }
    }
}

/// A join node representing a table to be joined.
#[derive(Debug, Clone)]
pub struct JoinNode {
    /// The table name.
    pub table: String,
    /// Optional alias for the table.
    pub alias: Option<String>,
    /// Join condition when joining with other tables.
    pub condition: Option<ra_core::expr::Expr>,
}

impl JoinNode {
    /// Convert to a scan expression.
    pub fn to_scan(&self) -> RelExpr {
        RelExpr::Scan {
            table: self.table.clone(),
            alias: self.alias.clone(),
        }
    }
}

/// Optimizer for large join graphs using heuristic strategies.
pub struct LargeJoinOptimizer {
    strategy: LargeJoinStrategy,
    cost_model: Arc<dyn CostModel>,
    stats_provider: Arc<dyn StatisticsProvider>,
}

impl LargeJoinOptimizer {
    /// Create a new large join optimizer.
    pub fn new(
        strategy: LargeJoinStrategy,
        cost_model: Arc<dyn CostModel>,
        stats_provider: Arc<dyn StatisticsProvider>,
    ) -> Self {
        Self {
            strategy,
            cost_model,
            stats_provider,
        }
    }

    /// Optimize join ordering using the configured heuristic strategy.
    pub fn optimize(&self, joins: Vec<JoinNode>) -> Result<RelExpr> {
        match &self.strategy {
            LargeJoinStrategy::Greedy => self.greedy_join_order(joins),
            LargeJoinStrategy::SimulatedAnnealing { .. } => self.simulated_annealing(joins),
            LargeJoinStrategy::EGraph => {
                // Fall through to standard e-graph optimization
                Err(anyhow!("EGraph strategy should use standard optimizer"))
            }
        }
    }

    /// Greedy join ordering: start with smallest relation, add lowest-cost joins.
    fn greedy_join_order(&self, mut joins: Vec<JoinNode>) -> Result<RelExpr> {
        if joins.is_empty() {
            return Err(anyhow!("No tables to join"));
        }

        // 1. Start with smallest relation (by cardinality)
        let smallest_idx = self.find_smallest_relation(&joins)?;
        let mut current = joins.swap_remove(smallest_idx).to_scan();

        // 2. Greedily add joins with lowest estimated cost
        while !joins.is_empty() {
            let (best_idx, _best_cost) = joins
                .iter()
                .enumerate()
                .map(|(i, join)| {
                    let candidate = self.create_join(&current, &join.to_scan(), join.condition.as_ref())?;
                    let cost = self.cost_model.estimate(&candidate, self.stats_provider.as_ref());
                    Ok((i, cost))
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .min_by(|(_, a), (_, b)| {
                    a.total().partial_cmp(&b.total()).unwrap_or(std::cmp::Ordering::Equal)
                })
                .ok_or_else(|| anyhow!("No valid join found"))?;

            let next_join = joins.swap_remove(best_idx);
            current = self.create_join(&current, &next_join.to_scan(), next_join.condition.as_ref())?;
        }

        Ok(current)
    }

    /// Find the index of the smallest relation by cardinality.
    fn find_smallest_relation(&self, joins: &[JoinNode]) -> Result<usize> {
        joins
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let stats = self.stats_provider
                    .get_statistics(&j.table)
                    .ok_or_else(|| anyhow!("No statistics for table {}", j.table))?;
                Ok((i, stats.row_count))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .ok_or_else(|| anyhow!("No relations found"))
    }

    /// Create a join between two expressions.
    fn create_join(
        &self,
        left: &RelExpr,
        right: &RelExpr,
        condition: Option<&ra_core::expr::Expr>,
    ) -> Result<RelExpr> {
        let join_condition = condition
            .cloned()
            .unwrap_or_else(|| ra_core::expr::Expr::Const(ra_core::expr::Const::Bool(true)));

        Ok(RelExpr::Join {
            join_type: JoinType::Inner,
            condition: join_condition,
            left: Box::new(left.clone()),
            right: Box::new(right.clone()),
        })
    }

    /// Simulated annealing: start with greedy solution, perturb and anneal.
    fn simulated_annealing(&self, joins: Vec<JoinNode>) -> Result<RelExpr> {
        if joins.is_empty() {
            return Err(anyhow!("No tables to join"));
        }

        // 1. Start with greedy initial solution
        let mut current = self.greedy_join_order(joins.clone())?;
        let mut current_cost = self.cost_model.estimate(&current, self.stats_provider.as_ref());
        let mut best = current.clone();
        let mut best_cost = current_cost.clone();

        // 2. Extract annealing parameters
        let LargeJoinStrategy::SimulatedAnnealing {
            initial_temp,
            cooling_rate,
            max_iterations,
        } = &self.strategy
        else {
            unreachable!("simulated_annealing called with non-annealing strategy")
        };

        let mut temp = *initial_temp;
        let mut rng = rand::thread_rng();

        // 3. Annealing loop
        for _iteration in 0..*max_iterations {
            // Perturb: create a neighbor solution
            let neighbor = match self.perturb_join_order(&current, &joins) {
                Ok(n) => n,
                Err(_) => continue, // Skip if perturbation fails
            };

            let neighbor_cost = self.cost_model.estimate(&neighbor, self.stats_provider.as_ref());

            // Accept if better, or probabilistically if worse
            let delta = neighbor_cost.total() - current_cost.total();
            let accept_prob = if delta < 0.0 {
                1.0 // Always accept improvements
            } else {
                (-delta / temp).exp()
            };

            if rng.gen::<f64>() < accept_prob {
                current = neighbor;
                current_cost = neighbor_cost.clone();

                if current_cost.total() < best_cost.total() {
                    best = current.clone();
                    best_cost = current_cost.clone();
                }
            }

            // Cool down
            temp *= cooling_rate;

            // Early termination if temperature is too low
            if temp < 0.001 {
                break;
            }
        }

        Ok(best)
    }

    /// Perturb a join order by swapping two random joins in the tree.
    fn perturb_join_order(&self, _plan: &RelExpr, joins: &[JoinNode]) -> Result<RelExpr> {
        // For simplicity, we'll regenerate the plan with a randomized join order
        // A more sophisticated implementation would swap subtrees in the existing plan

        let mut rng = rand::thread_rng();
        let mut shuffled_joins = joins.to_vec();
        shuffled_joins.shuffle(&mut rng);

        // Build a new join tree with the shuffled order
        if shuffled_joins.is_empty() {
            return Err(anyhow!("No joins to perturb"));
        }

        let mut current = shuffled_joins[0].to_scan();
        for join in &shuffled_joins[1..] {
            current = self.create_join(&current, &join.to_scan(), join.condition.as_ref())?;
        }

        Ok(current)
    }

    /// Count the number of tables in a relational expression.
    pub fn count_tables(expr: &RelExpr) -> usize {
        match expr {
            RelExpr::Scan { .. }
            | RelExpr::IndexScan { .. }
            | RelExpr::IndexOnlyScan { .. } => 1,
            RelExpr::Filter { input, .. } => Self::count_tables(input),
            RelExpr::Project { input, .. } => Self::count_tables(input),
            RelExpr::Join { left, right, .. } => {
                Self::count_tables(left) + Self::count_tables(right)
            }
            RelExpr::Aggregate { input, .. } => Self::count_tables(input),
            RelExpr::Sort { input, .. } => Self::count_tables(input),
            RelExpr::Limit { input, .. } => Self::count_tables(input),
            RelExpr::Union { left, right, .. } => {
                Self::count_tables(left).max(Self::count_tables(right))
            }
            RelExpr::Intersect { left, right, .. } => {
                Self::count_tables(left).max(Self::count_tables(right))
            }
            RelExpr::Except { left, right, .. } => {
                Self::count_tables(left).max(Self::count_tables(right))
            }
            RelExpr::RecursiveCTE { base_case, recursive_case, body, .. } => {
                Self::count_tables(base_case)
                    .max(Self::count_tables(recursive_case))
                    .max(Self::count_tables(body))
            }
            RelExpr::CTE { definition, body, .. } => {
                Self::count_tables(definition).max(Self::count_tables(body))
            }
            RelExpr::Window { input, .. } => Self::count_tables(input),
            RelExpr::Distinct { input } => Self::count_tables(input),
            RelExpr::Values { .. } => 0,
            RelExpr::RowPattern { input, .. } => Self::count_tables(input),
            RelExpr::BitmapIndexScan { .. } => 1,
            RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
                inputs.iter().map(|i| Self::count_tables(i)).sum()
            }
            RelExpr::BitmapHeapScan { bitmap, .. } => Self::count_tables(bitmap),
            RelExpr::Unnest { input, .. } => input.as_ref().map_or(0, |i| Self::count_tables(i)),
            RelExpr::MultiUnnest { .. } => 0,
            RelExpr::TableFunction { input, .. } => input.as_ref().map_or(0, |i| Self::count_tables(i)),
            RelExpr::IncrementalSort { input, .. } => Self::count_tables(input),
            RelExpr::ParallelScan { .. } => 1,
            RelExpr::ParallelHashJoin { left, right, .. } => {
                Self::count_tables(left) + Self::count_tables(right)
            }
            RelExpr::ParallelAggregate { input, .. } | RelExpr::Gather { input, .. } => {
                Self::count_tables(input)
            }
            RelExpr::MvScan { .. } => 1,
        }
    }

    /// Extract join nodes from a relational expression.
    pub fn extract_joins(expr: &RelExpr) -> Vec<JoinNode> {
        let mut joins = Vec::new();
        Self::extract_joins_recursive(expr, &mut joins);
        joins
    }

    fn extract_joins_recursive(expr: &RelExpr, joins: &mut Vec<JoinNode>) {
        match expr {
            RelExpr::Scan { table, alias } => {
                joins.push(JoinNode {
                    table: table.clone(),
                    alias: alias.clone(),
                    condition: None,
                });
            }
            RelExpr::Join {
                left,
                right,
                condition: _,
                ..
            } => {
                // Extract tables from both sides
                Self::extract_joins_recursive(left, joins);
                Self::extract_joins_recursive(right, joins);
                // Note: In a real implementation, we'd associate conditions with the appropriate joins
            }
            RelExpr::Filter { input, .. }
            | RelExpr::Project { input, .. }
            | RelExpr::Aggregate { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Window { input, .. }
            | RelExpr::Distinct { input }
            | RelExpr::RowPattern { input, .. } => {
                Self::extract_joins_recursive(input, joins);
            }
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                Self::extract_joins_recursive(left, joins);
                Self::extract_joins_recursive(right, joins);
            }
            RelExpr::RecursiveCTE {
                base_case, recursive_case, body, ..
            } => {
                Self::extract_joins_recursive(base_case, joins);
                Self::extract_joins_recursive(recursive_case, joins);
                Self::extract_joins_recursive(body, joins);
            }
            RelExpr::CTE {
                definition, body, ..
            } => {
                Self::extract_joins_recursive(definition, joins);
                Self::extract_joins_recursive(body, joins);
            }
            RelExpr::BitmapHeapScan { bitmap, .. } => {
                Self::extract_joins_recursive(bitmap, joins);
            }
            RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
                for input in inputs {
                    Self::extract_joins_recursive(input, joins);
                }
            }
            RelExpr::Values { .. } | RelExpr::BitmapIndexScan { .. } | RelExpr::MultiUnnest { .. } | RelExpr::ParallelScan { .. } | RelExpr::IndexScan { .. } | RelExpr::IndexOnlyScan { .. } | RelExpr::MvScan { .. } => {
                // Leaf nodes, no joins to extract
            }
            RelExpr::Unnest { input, .. } | RelExpr::TableFunction { input, .. } => {
                if let Some(inp) = input {
                    Self::extract_joins_recursive(inp, joins);
                }
            }
            RelExpr::IncrementalSort { input, .. } | RelExpr::ParallelAggregate { input, .. } | RelExpr::Gather { input, .. } => {
                Self::extract_joins_recursive(input, joins);
            }
            RelExpr::ParallelHashJoin { left, right, .. } => {
                Self::extract_joins_recursive(left, joins);
                Self::extract_joins_recursive(right, joins);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ra_core::cost::Cost;
    use ra_core::expr::{Const, Expr};
    use ra_core::statistics::Statistics;
    use std::collections::HashMap;

    #[derive(Debug)]
    struct MockCostModel;

    impl CostModel for MockCostModel {
        fn estimate(
            &self,
            expr: &RelExpr,
            _stats: &dyn StatisticsProvider,
        ) -> Cost {
            let tables =
                LargeJoinOptimizer::count_tables(expr);
            Cost::new(tables as f64 * 10.0, 0.0, 0.0, 0)
        }
    }

    #[derive(Debug)]
    struct MockStats {
        stats: HashMap<String, Statistics>,
    }

    impl MockStats {
        fn new(entries: &[(&str, f64)]) -> Self {
            let mut stats = HashMap::new();
            for &(name, rows) in entries {
                stats.insert(
                    name.to_string(),
                    Statistics::new(rows),
                );
            }
            Self { stats }
        }
    }

    impl StatisticsProvider for MockStats {
        fn get_statistics(
            &self,
            table: &str,
        ) -> Option<&Statistics> {
            self.stats.get(table)
        }
    }

    fn make_optimizer(
        strategy: LargeJoinStrategy,
        table_rows: &[(&str, f64)],
    ) -> LargeJoinOptimizer {
        LargeJoinOptimizer::new(
            strategy,
            Arc::new(MockCostModel),
            Arc::new(MockStats::new(table_rows)),
        )
    }

    fn make_join_node(table: &str) -> JoinNode {
        JoinNode {
            table: table.to_string(),
            alias: None,
            condition: None,
        }
    }

    fn true_expr() -> Expr {
        Expr::Const(Const::Bool(true))
    }

    fn scan(table: &str) -> RelExpr {
        RelExpr::Scan {
            table: table.to_string(),
            alias: None,
        }
    }

    // ---- count_tables ----

    #[test]
    fn count_tables_single_scan() {
        assert_eq!(
            LargeJoinOptimizer::count_tables(&scan("t")),
            1,
        );
    }

    #[test]
    fn count_tables_two_table_join() {
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&join), 2);
    }

    #[test]
    fn count_tables_nested_join() {
        let inner = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let outer = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(inner),
            right: Box::new(scan("c")),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&outer),
            3,
        );
    }

    #[test]
    fn count_tables_through_filter_and_project() {
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let filtered = RelExpr::Filter {
            predicate: true_expr(),
            input: Box::new(join),
        };
        let projected = RelExpr::Project {
            columns: vec![],
            input: Box::new(filtered),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&projected),
            2,
        );
    }

    #[test]
    fn count_tables_aggregate() {
        let agg = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(scan("t")),
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&agg), 1);
    }

    #[test]
    fn count_tables_sort() {
        let sorted = RelExpr::Sort {
            keys: vec![],
            input: Box::new(scan("t")),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&sorted),
            1,
        );
    }

    #[test]
    fn count_tables_limit() {
        let limited = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(scan("t")),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&limited),
            1,
        );
    }

    #[test]
    fn count_tables_union_takes_max() {
        let u = RelExpr::Union {
            left: Box::new(scan("a")),
            right: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: true_expr(),
                left: Box::new(scan("b")),
                right: Box::new(scan("c")),
            }),
            all: true,
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&u), 2);
    }

    #[test]
    fn count_tables_intersect_takes_max() {
        let i = RelExpr::Intersect {
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
            all: false,
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&i), 1);
    }

    #[test]
    fn count_tables_except_takes_max() {
        let e = RelExpr::Except {
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
            all: false,
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&e), 1);
    }

    #[test]
    fn count_tables_values_returns_zero() {
        let v = RelExpr::Values { rows: vec![] };
        assert_eq!(LargeJoinOptimizer::count_tables(&v), 0);
    }

    #[test]
    fn count_tables_window() {
        let w = RelExpr::Window {
            functions: vec![],
            input: Box::new(scan("t")),
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&w), 1);
    }

    #[test]
    fn count_tables_distinct() {
        let d = RelExpr::Distinct {
            input: Box::new(scan("t")),
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&d), 1);
    }

    #[test]
    fn count_tables_index_scan() {
        let is = RelExpr::IndexScan {
            table: "t".to_string(),
            column: "id".to_string(),
        };
        assert_eq!(LargeJoinOptimizer::count_tables(&is), 1);
    }

    // ---- extract_joins ----

    #[test]
    fn extract_joins_single_scan() {
        let joins =
            LargeJoinOptimizer::extract_joins(&scan("t"));
        assert_eq!(joins.len(), 1);
        assert_eq!(joins[0].table, "t");
        assert!(joins[0].alias.is_none());
    }

    #[test]
    fn extract_joins_with_alias() {
        let expr = RelExpr::Scan {
            table: "users".to_string(),
            alias: Some("u".to_string()),
        };
        let joins = LargeJoinOptimizer::extract_joins(&expr);
        assert_eq!(joins.len(), 1);
        assert_eq!(
            joins[0].alias,
            Some("u".to_string()),
        );
    }

    #[test]
    fn extract_joins_two_table_join() {
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let joins = LargeJoinOptimizer::extract_joins(&join);
        assert_eq!(joins.len(), 2);
        assert_eq!(joins[0].table, "a");
        assert_eq!(joins[1].table, "b");
    }

    #[test]
    fn extract_joins_through_filter() {
        let filtered = RelExpr::Filter {
            predicate: true_expr(),
            input: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: true_expr(),
                left: Box::new(scan("x")),
                right: Box::new(scan("y")),
            }),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&filtered);
        assert_eq!(joins.len(), 2);
    }

    #[test]
    fn extract_joins_three_way() {
        let inner = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let outer = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(inner),
            right: Box::new(scan("c")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&outer);
        assert_eq!(joins.len(), 3);
    }

    #[test]
    fn extract_joins_values_empty() {
        let v = RelExpr::Values { rows: vec![] };
        let joins = LargeJoinOptimizer::extract_joins(&v);
        assert!(joins.is_empty());
    }

    // ---- JoinNode::to_scan ----

    #[test]
    fn join_node_to_scan_without_alias() {
        let node = make_join_node("users");
        let expr = node.to_scan();
        match &expr {
            RelExpr::Scan { table, alias } => {
                assert_eq!(table, "users");
                assert!(alias.is_none());
            }
            _ => panic!("Expected Scan"),
        }
    }

    #[test]
    fn join_node_to_scan_with_alias() {
        let node = JoinNode {
            table: "users".to_string(),
            alias: Some("u".to_string()),
            condition: None,
        };
        let expr = node.to_scan();
        match &expr {
            RelExpr::Scan { table, alias } => {
                assert_eq!(table, "users");
                assert_eq!(alias.as_deref(), Some("u"));
            }
            _ => panic!("Expected Scan"),
        }
    }

    // ---- LargeJoinStrategy::default ----

    #[test]
    fn default_strategy_is_simulated_annealing() {
        let strategy = LargeJoinStrategy::default();
        match strategy {
            LargeJoinStrategy::SimulatedAnnealing {
                initial_temp,
                cooling_rate,
                max_iterations,
            } => {
                assert!(
                    (initial_temp - 1000.0).abs()
                        < f64::EPSILON,
                );
                assert!(
                    (cooling_rate - 0.95).abs()
                        < f64::EPSILON,
                );
                assert_eq!(max_iterations, 10000);
            }
            _ => panic!("Expected SimulatedAnnealing"),
        }
    }

    // ---- Greedy optimization ----

    #[test]
    fn greedy_empty_joins_returns_error() {
        let opt =
            make_optimizer(LargeJoinStrategy::Greedy, &[]);
        let result = opt.optimize(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn greedy_single_table_returns_scan() {
        let opt = make_optimizer(
            LargeJoinStrategy::Greedy,
            &[("t", 100.0)],
        );
        let result =
            opt.optimize(vec![make_join_node("t")]);
        assert!(result.is_ok());
        let expr = result.unwrap();
        match &expr {
            RelExpr::Scan { table, .. } => {
                assert_eq!(table, "t");
            }
            _ => panic!("Expected Scan for single table"),
        }
    }

    #[test]
    fn greedy_two_tables_starts_with_smallest() {
        let opt = make_optimizer(
            LargeJoinStrategy::Greedy,
            &[("big", 10000.0), ("small", 10.0)],
        );
        let joins = vec![
            make_join_node("big"),
            make_join_node("small"),
        ];
        let result = opt.optimize(joins).unwrap();
        match &result {
            RelExpr::Join { left, .. } => {
                match left.as_ref() {
                    RelExpr::Scan { table, .. } => {
                        assert_eq!(table, "small");
                    }
                    _ => panic!(
                        "Expected Scan as left child"
                    ),
                }
            }
            _ => panic!("Expected Join"),
        }
    }

    #[test]
    fn greedy_three_tables_produces_valid_plan() {
        let opt = make_optimizer(
            LargeJoinStrategy::Greedy,
            &[
                ("a", 100.0),
                ("b", 200.0),
                ("c", 50.0),
            ],
        );
        let joins = vec![
            make_join_node("a"),
            make_join_node("b"),
            make_join_node("c"),
        ];
        let result = opt.optimize(joins).unwrap();
        assert_eq!(
            LargeJoinOptimizer::count_tables(&result),
            3,
        );
    }

    #[test]
    fn greedy_missing_stats_returns_error() {
        let opt = make_optimizer(
            LargeJoinStrategy::Greedy,
            &[("a", 100.0)],
        );
        let joins = vec![
            make_join_node("a"),
            make_join_node("unknown"),
        ];
        let result = opt.optimize(joins);
        assert!(result.is_err());
    }

    // ---- EGraph strategy ----

    #[test]
    fn egraph_strategy_returns_error() {
        let opt = make_optimizer(
            LargeJoinStrategy::EGraph,
            &[("t", 100.0)],
        );
        let result =
            opt.optimize(vec![make_join_node("t")]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("EGraph"));
    }

    // ---- Simulated annealing ----

    #[test]
    fn annealing_empty_joins_returns_error() {
        let opt = make_optimizer(
            LargeJoinStrategy::default(),
            &[],
        );
        let result = opt.optimize(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn annealing_single_table() {
        let opt = make_optimizer(
            LargeJoinStrategy::SimulatedAnnealing {
                initial_temp: 100.0,
                cooling_rate: 0.9,
                max_iterations: 10,
            },
            &[("t", 100.0)],
        );
        let result =
            opt.optimize(vec![make_join_node("t")]);
        assert!(result.is_ok());
    }

    #[test]
    fn annealing_two_tables_returns_valid_plan() {
        let opt = make_optimizer(
            LargeJoinStrategy::SimulatedAnnealing {
                initial_temp: 100.0,
                cooling_rate: 0.9,
                max_iterations: 50,
            },
            &[("a", 500.0), ("b", 100.0)],
        );
        let joins = vec![
            make_join_node("a"),
            make_join_node("b"),
        ];
        let result = opt.optimize(joins).unwrap();
        assert_eq!(
            LargeJoinOptimizer::count_tables(&result),
            2,
        );
    }

    #[test]
    fn annealing_low_temp_terminates_early() {
        let opt = make_optimizer(
            LargeJoinStrategy::SimulatedAnnealing {
                initial_temp: 0.0001,
                cooling_rate: 0.5,
                max_iterations: 100_000,
            },
            &[("a", 100.0), ("b", 200.0)],
        );
        let joins = vec![
            make_join_node("a"),
            make_join_node("b"),
        ];
        let result = opt.optimize(joins);
        assert!(result.is_ok());
    }

    // ---- find_smallest_relation ----

    #[test]
    fn find_smallest_picks_smallest_cardinality() {
        let opt = make_optimizer(
            LargeJoinStrategy::Greedy,
            &[
                ("big", 10000.0),
                ("mid", 500.0),
                ("small", 10.0),
            ],
        );
        let joins = vec![
            make_join_node("big"),
            make_join_node("mid"),
            make_join_node("small"),
        ];
        let idx =
            opt.find_smallest_relation(&joins).unwrap();
        assert_eq!(joins[idx].table, "small");
    }

    // ---- create_join ----

    #[test]
    fn create_join_with_condition() {
        let opt =
            make_optimizer(LargeJoinStrategy::Greedy, &[]);
        let cond = Expr::Const(Const::Bool(true));
        let result = opt.create_join(
            &scan("a"),
            &scan("b"),
            Some(&cond),
        );
        assert!(result.is_ok());
        match result.unwrap() {
            RelExpr::Join {
                join_type,
                condition,
                ..
            } => {
                assert!(matches!(
                    join_type,
                    JoinType::Inner,
                ));
                assert!(matches!(
                    condition,
                    Expr::Const(Const::Bool(true)),
                ));
            }
            _ => panic!("Expected Join"),
        }
    }

    #[test]
    fn create_join_without_condition_uses_true() {
        let opt =
            make_optimizer(LargeJoinStrategy::Greedy, &[]);
        let result =
            opt.create_join(&scan("a"), &scan("b"), None);
        assert!(result.is_ok());
        match result.unwrap() {
            RelExpr::Join { condition, .. } => {
                assert!(matches!(
                    condition,
                    Expr::Const(Const::Bool(true)),
                ));
            }
            _ => panic!("Expected Join"),
        }
    }

    // ---- Serialization ----

    #[test]
    fn strategy_serialization_roundtrip() {
        let strategy =
            LargeJoinStrategy::SimulatedAnnealing {
                initial_temp: 500.0,
                cooling_rate: 0.99,
                max_iterations: 5000,
            };
        let json =
            serde_json::to_string(&strategy).unwrap();
        let restored: LargeJoinStrategy =
            serde_json::from_str(&json).unwrap();
        match restored {
            LargeJoinStrategy::SimulatedAnnealing {
                initial_temp,
                cooling_rate,
                max_iterations,
            } => {
                assert!(
                    (initial_temp - 500.0).abs()
                        < f64::EPSILON,
                );
                assert!(
                    (cooling_rate - 0.99).abs()
                        < f64::EPSILON,
                );
                assert_eq!(max_iterations, 5000);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn greedy_strategy_serialization() {
        let strategy = LargeJoinStrategy::Greedy;
        let json =
            serde_json::to_string(&strategy).unwrap();
        let restored: LargeJoinStrategy =
            serde_json::from_str(&json).unwrap();
        assert!(matches!(
            restored,
            LargeJoinStrategy::Greedy,
        ));
    }

    #[test]
    fn egraph_strategy_serialization() {
        let strategy = LargeJoinStrategy::EGraph;
        let json =
            serde_json::to_string(&strategy).unwrap();
        let restored: LargeJoinStrategy =
            serde_json::from_str(&json).unwrap();
        assert!(matches!(
            restored,
            LargeJoinStrategy::EGraph,
        ));
    }

    // ---- count_tables: rare variant coverage ----

    #[test]
    fn count_tables_recursive_cte() {
        let rcte = RelExpr::RecursiveCTE {
            name: "r".to_string(),
            base_case: Box::new(scan("a")),
            recursive_case: Box::new(scan("b")),
            body: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: true_expr(),
                left: Box::new(scan("c")),
                right: Box::new(scan("d")),
            }),
            cycle_detection: None,
        };
        // max(1, 1, 2) = 2
        assert_eq!(
            LargeJoinOptimizer::count_tables(&rcte),
            2,
        );
    }

    #[test]
    fn count_tables_cte() {
        let cte = RelExpr::CTE {
            name: "tmp".to_string(),
            definition: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: true_expr(),
                left: Box::new(scan("a")),
                right: Box::new(scan("b")),
            }),
            body: Box::new(scan("c")),
        };
        // max(2, 1) = 2
        assert_eq!(
            LargeJoinOptimizer::count_tables(&cte),
            2,
        );
    }

    #[test]
    fn count_tables_row_pattern() {
        use ra_core::row_pattern::{
            MatchMode, PatternExpr, SkipMode,
        };
        let rp = RelExpr::RowPattern {
            input: Box::new(scan("t")),
            partition_by: vec![],
            order_by: vec![],
            pattern: PatternExpr::Var("A".to_string()),
            defines: vec![],
            measures: vec![],
            mode: MatchMode::OneRowPerMatch,
            skip_mode: SkipMode::PastLastRow,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&rp),
            1,
        );
    }

    #[test]
    fn count_tables_bitmap_index_scan() {
        let bis = RelExpr::BitmapIndexScan {
            table: "t".to_string(),
            index: "idx".to_string(),
            predicate: true_expr(),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&bis),
            1,
        );
    }

    #[test]
    fn count_tables_bitmap_and() {
        let ba = RelExpr::BitmapAnd {
            inputs: vec![
                Box::new(RelExpr::BitmapIndexScan {
                    table: "t".to_string(),
                    index: "idx1".to_string(),
                    predicate: true_expr(),
                }),
                Box::new(RelExpr::BitmapIndexScan {
                    table: "t".to_string(),
                    index: "idx2".to_string(),
                    predicate: true_expr(),
                }),
            ],
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&ba),
            2,
        );
    }

    #[test]
    fn count_tables_bitmap_or() {
        let bo = RelExpr::BitmapOr {
            inputs: vec![Box::new(RelExpr::BitmapIndexScan {
                table: "t".to_string(),
                index: "idx".to_string(),
                predicate: true_expr(),
            })],
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&bo),
            1,
        );
    }

    #[test]
    fn count_tables_bitmap_heap_scan() {
        let bhs = RelExpr::BitmapHeapScan {
            table: "t".to_string(),
            bitmap: Box::new(RelExpr::BitmapIndexScan {
                table: "t".to_string(),
                index: "idx".to_string(),
                predicate: true_expr(),
            }),
            recheck_cond: None,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&bhs),
            1,
        );
    }

    #[test]
    fn count_tables_unnest_with_input() {
        let u = RelExpr::Unnest {
            expr: Expr::Const(Const::Int(1)),
            alias: None,
            input: Some(Box::new(scan("t"))),
            with_ordinality: false,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&u),
            1,
        );
    }

    #[test]
    fn count_tables_unnest_without_input() {
        let u = RelExpr::Unnest {
            expr: Expr::Const(Const::Int(1)),
            alias: None,
            input: None,
            with_ordinality: false,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&u),
            0,
        );
    }

    #[test]
    fn count_tables_multi_unnest() {
        let mu = RelExpr::MultiUnnest {
            exprs: vec![],
            aliases: vec![],
            with_ordinality: false,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&mu),
            0,
        );
    }

    #[test]
    fn count_tables_table_function() {
        let tf = RelExpr::TableFunction {
            name: "generate_series".to_string(),
            args: vec![],
            columns: vec![],
            input: Some(Box::new(scan("t"))),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&tf),
            1,
        );
    }

    #[test]
    fn count_tables_incremental_sort() {
        let isort = RelExpr::IncrementalSort {
            prefix_keys: vec![],
            suffix_keys: vec![],
            input: Box::new(scan("t")),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&isort),
            1,
        );
    }

    #[test]
    fn count_tables_parallel_scan() {
        let ps = RelExpr::ParallelScan {
            table: "t".to_string(),
            workers: 4,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&ps),
            1,
        );
    }

    #[test]
    fn count_tables_parallel_hash_join() {
        let phj = RelExpr::ParallelHashJoin {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
            workers: 4,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&phj),
            2,
        );
    }

    #[test]
    fn count_tables_parallel_aggregate() {
        let pa = RelExpr::ParallelAggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(scan("t")),
            workers: 4,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&pa),
            1,
        );
    }

    #[test]
    fn count_tables_gather() {
        let g = RelExpr::Gather {
            input: Box::new(scan("t")),
            workers: 4,
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&g),
            1,
        );
    }

    #[test]
    fn count_tables_index_only_scan() {
        use ra_core::algebra::ProjectionColumn;
        use ra_core::expr::ColumnRef;
        let ios = RelExpr::IndexOnlyScan {
            table: "t".to_string(),
            index: "idx".to_string(),
            columns: vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("id")),
                alias: None,
            }],
            predicate: true_expr(),
        };
        assert_eq!(
            LargeJoinOptimizer::count_tables(&ios),
            1,
        );
    }

    // ---- extract_joins: rare variant coverage ----

    #[test]
    fn extract_joins_through_project() {
        let projected = RelExpr::Project {
            columns: vec![],
            input: Box::new(scan("t")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&projected);
        assert_eq!(joins.len(), 1);
        assert_eq!(joins[0].table, "t");
    }

    #[test]
    fn extract_joins_through_aggregate() {
        let agg = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(scan("t")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&agg);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_through_sort() {
        let sorted = RelExpr::Sort {
            keys: vec![],
            input: Box::new(scan("t")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&sorted);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_through_limit() {
        let limited = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(scan("t")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&limited);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_through_window() {
        let w = RelExpr::Window {
            functions: vec![],
            input: Box::new(scan("t")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&w);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_through_distinct() {
        let d = RelExpr::Distinct {
            input: Box::new(scan("t")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&d);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_through_row_pattern() {
        use ra_core::row_pattern::{
            MatchMode, PatternExpr, SkipMode,
        };
        let rp = RelExpr::RowPattern {
            input: Box::new(scan("t")),
            partition_by: vec![],
            order_by: vec![],
            pattern: PatternExpr::Var("A".to_string()),
            defines: vec![],
            measures: vec![],
            mode: MatchMode::OneRowPerMatch,
            skip_mode: SkipMode::PastLastRow,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&rp);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_union() {
        let u = RelExpr::Union {
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
            all: true,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&u);
        assert_eq!(joins.len(), 2);
    }

    #[test]
    fn extract_joins_intersect() {
        let i = RelExpr::Intersect {
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
            all: false,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&i);
        assert_eq!(joins.len(), 2);
    }

    #[test]
    fn extract_joins_except() {
        let e = RelExpr::Except {
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
            all: false,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&e);
        assert_eq!(joins.len(), 2);
    }

    #[test]
    fn extract_joins_recursive_cte() {
        let rcte = RelExpr::RecursiveCTE {
            name: "r".to_string(),
            base_case: Box::new(scan("a")),
            recursive_case: Box::new(scan("b")),
            body: Box::new(scan("c")),
            cycle_detection: None,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&rcte);
        assert_eq!(joins.len(), 3);
    }

    #[test]
    fn extract_joins_cte() {
        let cte = RelExpr::CTE {
            name: "tmp".to_string(),
            definition: Box::new(scan("a")),
            body: Box::new(scan("b")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&cte);
        assert_eq!(joins.len(), 2);
    }

    #[test]
    fn extract_joins_bitmap_heap_scan() {
        let bhs = RelExpr::BitmapHeapScan {
            table: "t".to_string(),
            bitmap: Box::new(RelExpr::BitmapIndexScan {
                table: "t".to_string(),
                index: "idx".to_string(),
                predicate: true_expr(),
            }),
            recheck_cond: None,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&bhs);
        // BitmapIndexScan is a leaf, nothing extracted
        assert!(joins.is_empty());
    }

    #[test]
    fn extract_joins_bitmap_and() {
        let ba = RelExpr::BitmapAnd {
            inputs: vec![
                Box::new(RelExpr::BitmapIndexScan {
                    table: "t".to_string(),
                    index: "idx1".to_string(),
                    predicate: true_expr(),
                }),
                Box::new(RelExpr::BitmapIndexScan {
                    table: "t".to_string(),
                    index: "idx2".to_string(),
                    predicate: true_expr(),
                }),
            ],
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&ba);
        assert!(joins.is_empty());
    }

    #[test]
    fn extract_joins_bitmap_or() {
        let bo = RelExpr::BitmapOr {
            inputs: vec![Box::new(
                RelExpr::BitmapIndexScan {
                    table: "t".to_string(),
                    index: "idx".to_string(),
                    predicate: true_expr(),
                },
            )],
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&bo);
        assert!(joins.is_empty());
    }

    #[test]
    fn extract_joins_leaf_nodes_empty() {
        use ra_core::algebra::ProjectionColumn;
        use ra_core::expr::ColumnRef;

        // Values
        assert!(LargeJoinOptimizer::extract_joins(
            &RelExpr::Values { rows: vec![] }
        )
        .is_empty());

        // BitmapIndexScan
        assert!(LargeJoinOptimizer::extract_joins(
            &RelExpr::BitmapIndexScan {
                table: "t".to_string(),
                index: "idx".to_string(),
                predicate: true_expr(),
            }
        )
        .is_empty());

        // MultiUnnest
        assert!(LargeJoinOptimizer::extract_joins(
            &RelExpr::MultiUnnest {
                exprs: vec![],
                aliases: vec![],
                with_ordinality: false,
            }
        )
        .is_empty());

        // ParallelScan
        assert!(LargeJoinOptimizer::extract_joins(
            &RelExpr::ParallelScan {
                table: "t".to_string(),
                workers: 4,
            }
        )
        .is_empty());

        // IndexScan
        assert!(LargeJoinOptimizer::extract_joins(
            &RelExpr::IndexScan {
                table: "t".to_string(),
                column: "id".to_string(),
            }
        )
        .is_empty());

        // IndexOnlyScan
        assert!(LargeJoinOptimizer::extract_joins(
            &RelExpr::IndexOnlyScan {
                table: "t".to_string(),
                index: "idx".to_string(),
                columns: vec![ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("id")),
                    alias: None,
                }],
                predicate: true_expr(),
            }
        )
        .is_empty());
    }

    #[test]
    fn extract_joins_unnest_with_input() {
        let u = RelExpr::Unnest {
            expr: Expr::Const(Const::Int(1)),
            alias: None,
            input: Some(Box::new(scan("t"))),
            with_ordinality: false,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&u);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_unnest_without_input() {
        let u = RelExpr::Unnest {
            expr: Expr::Const(Const::Int(1)),
            alias: None,
            input: None,
            with_ordinality: false,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&u);
        assert!(joins.is_empty());
    }

    #[test]
    fn extract_joins_table_function_with_input() {
        let tf = RelExpr::TableFunction {
            name: "generate_series".to_string(),
            args: vec![],
            columns: vec![],
            input: Some(Box::new(scan("t"))),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&tf);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_incremental_sort() {
        let isort = RelExpr::IncrementalSort {
            prefix_keys: vec![],
            suffix_keys: vec![],
            input: Box::new(scan("t")),
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&isort);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_parallel_aggregate() {
        let pa = RelExpr::ParallelAggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(scan("t")),
            workers: 4,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&pa);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_gather() {
        let g = RelExpr::Gather {
            input: Box::new(scan("t")),
            workers: 4,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&g);
        assert_eq!(joins.len(), 1);
    }

    #[test]
    fn extract_joins_parallel_hash_join() {
        let phj = RelExpr::ParallelHashJoin {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
            workers: 4,
        };
        let joins =
            LargeJoinOptimizer::extract_joins(&phj);
        assert_eq!(joins.len(), 2);
    }

    // ---- Annealing: multi-table to exercise inner loop ----

    #[test]
    fn annealing_four_tables_exercises_inner_loop() {
        let opt = make_optimizer(
            LargeJoinStrategy::SimulatedAnnealing {
                initial_temp: 1000.0,
                cooling_rate: 0.95,
                max_iterations: 200,
            },
            &[
                ("a", 100.0),
                ("b", 200.0),
                ("c", 50.0),
                ("d", 300.0),
            ],
        );
        let joins = vec![
            make_join_node("a"),
            make_join_node("b"),
            make_join_node("c"),
            make_join_node("d"),
        ];
        let result = opt.optimize(joins).unwrap();
        assert_eq!(
            LargeJoinOptimizer::count_tables(&result),
            4,
        );
    }
}