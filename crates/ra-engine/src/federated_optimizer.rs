//! Federated query optimizer that selects execution strategies for
//! queries spanning multiple data sources.
//!
//! The optimizer evaluates three main strategies:
//! - **Ship Query**: send the entire query to a remote database
//! - **Ship Data**: fetch raw data from remote, execute locally
//! - **Hybrid**: push down filters/aggregations, fetch intermediate
//!   results, finish execution locally

use ra_core::algebra::RelExpr;
use ra_core::expr::Expr;
use ra_core::federated::{
    DataSource, ExecutionLocation, FederatedCostBreakdown, FederatedPlan, FederatedQuery,
    QueryCapabilities,
};

use crate::federated_cost::FederatedCostModel;

/// Optimizer for federated (cross-database) queries.
#[derive(Debug, Clone)]
pub struct FederatedOptimizer {
    cost_model: FederatedCostModel,
}

/// Errors that can occur during federated optimization.
#[derive(Debug, thiserror::Error)]
pub enum FederatedError {
    /// No sources found in the query.
    #[error("no data sources defined in the federated query")]
    NoSources,
    /// No viable strategy found.
    #[error("no viable execution strategy found for the query")]
    NoViableStrategy,
}

impl FederatedOptimizer {
    /// Create an optimizer with the default cost model.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cost_model: FederatedCostModel::new(),
        }
    }

    /// Create an optimizer with a custom cost model.
    #[must_use]
    pub fn with_cost_model(cost_model: FederatedCostModel) -> Self {
        Self { cost_model }
    }

    /// Optimize a federated query by selecting the cheapest
    /// execution strategy.
    ///
    /// # Errors
    ///
    /// Returns an error if the query has no sources or no viable
    /// strategy can be found.
    pub fn optimize_federated(
        &self,
        query: &FederatedQuery,
    ) -> Result<FederatedPlan, FederatedError> {
        if query.sources.is_empty() {
            return Err(FederatedError::NoSources);
        }

        // If there are no remote sources, execute locally
        if !query.is_distributed() {
            return Ok(self.plan_local(query));
        }

        let strategies = self.enumerate_strategies(query);
        if strategies.is_empty() {
            return Err(FederatedError::NoViableStrategy);
        }

        // Cost each strategy
        let mut costed: Vec<(ExecutionLocation, FederatedCostBreakdown)> = Vec::new();
        for strategy in &strategies {
            let cost = self.cost_model.estimate_location(strategy, query);
            costed.push((strategy.clone(), cost));
        }

        // Sort by total cost
        costed.sort_by(|a, b| {
            a.1.total_ms
                .partial_cmp(&b.1.total_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let (best_location, best_cost) = costed.remove(0);
        let alternatives: Vec<FederatedCostBreakdown> =
            costed.into_iter().map(|(_, c)| c).collect();

        let steps = self.describe_steps(&best_location, query);

        Ok(FederatedPlan {
            location: best_location,
            cost: best_cost,
            alternatives,
            steps,
        })
    }

    /// Enumerate all viable execution strategies for a query.
    #[must_use]
    pub fn enumerate_strategies(&self, query: &FederatedQuery) -> Vec<ExecutionLocation> {
        let mut strategies = Vec::new();

        // Strategy: execute locally
        strategies.push(ExecutionLocation::Local {
            query: query.plan.clone(),
        });

        // For each remote source, consider shipping options
        for (name, source) in &query.sources {
            if let DataSource::Remote {
                connection,
                capabilities,
                table,
                ..
            } = source
            {
                // Ship Query: if remote supports the full query
                if self.can_ship_query(&query.plan, capabilities) {
                    strategies.push(ExecutionLocation::ShipQuery {
                        target: connection.clone(),
                        query: query.plan.clone(),
                    });
                }

                // Ship Data: always viable (full scan)
                strategies.push(ExecutionLocation::ShipData {
                    source: connection.clone(),
                    table: table.clone(),
                    predicate: None,
                });

                // Ship Data with filter pushdown
                if capabilities.supports_filter_pushdown {
                    if let Some(pred) = self.extract_pushable_filter(&query.plan, name) {
                        strategies.push(ExecutionLocation::ShipData {
                            source: connection.clone(),
                            table: table.clone(),
                            predicate: Some(pred),
                        });
                    }
                }

                // Hybrid: if partial pushdown is possible
                if let Some((remote_sub, local_ops)) =
                    self.plan_hybrid(&query.plan, capabilities, name)
                {
                    strategies.push(ExecutionLocation::Hybrid {
                        remote_subquery: Box::new(remote_sub),
                        local_operations: Box::new(local_ops),
                        target: connection.clone(),
                    });
                }
            }
        }

        strategies
    }

    /// Check whether the entire query can be shipped to a remote
    /// database.
    #[must_use]
    pub fn can_ship_query(&self, plan: &RelExpr, capabilities: &QueryCapabilities) -> bool {
        match plan {
            RelExpr::Scan { .. } | RelExpr::IndexScan { .. } | RelExpr::IndexOnlyScan { .. } => {
                true
            }
            RelExpr::Filter { input, .. } => {
                capabilities.supports_filter_pushdown && self.can_ship_query(input, capabilities)
            }
            RelExpr::Project { input, .. } => {
                capabilities.supports_project_pushdown && self.can_ship_query(input, capabilities)
            }
            RelExpr::Join { left, right, .. } => {
                capabilities.supports_join_pushdown
                    && self.can_ship_query(left, capabilities)
                    && self.can_ship_query(right, capabilities)
            }
            RelExpr::Aggregate { input, .. } => {
                capabilities.supports_aggregate_pushdown && self.can_ship_query(input, capabilities)
            }
            RelExpr::Sort { input, .. } => {
                capabilities.supports_sort_pushdown && self.can_ship_query(input, capabilities)
            }
            RelExpr::Limit { input, .. } => {
                capabilities.supports_limit_pushdown && self.can_ship_query(input, capabilities)
            }
            RelExpr::Window { input, .. } => {
                capabilities.supports_window_pushdown && self.can_ship_query(input, capabilities)
            }
            RelExpr::Distinct { input, .. } => self.can_ship_query(input, capabilities),
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                self.can_ship_query(left, capabilities) && self.can_ship_query(right, capabilities)
            }
            // CTEs, VALUES, table functions, and pattern matching
            // are too complex to ship
            RelExpr::CTE { .. }
            | RelExpr::RecursiveCTE { .. }
            | RelExpr::Values { .. }
            | RelExpr::Unnest { .. }
            | RelExpr::MultiUnnest { .. }
            | RelExpr::TableFunction { .. }
            | RelExpr::RowPattern { .. }
            | RelExpr::IncrementalSort { .. }
            | RelExpr::BitmapIndexScan { .. }
            | RelExpr::BitmapAnd { .. }
            | RelExpr::BitmapOr { .. }
            | RelExpr::BitmapHeapScan { .. }
            | RelExpr::ParallelScan { .. }
            | RelExpr::ParallelHashJoin { .. }
            | RelExpr::ParallelAggregate { .. }
            | RelExpr::Gather { .. }
            | RelExpr::MvScan { .. }
            | RelExpr::TopK { .. }
            | RelExpr::VectorFilter { .. } => false,
        }
    }

    /// Extract a filter predicate that references a specific
    /// remote table and can be pushed down.
    fn extract_pushable_filter(&self, plan: &RelExpr, table_name: &str) -> Option<Expr> {
        match plan {
            RelExpr::Filter { predicate, input } => {
                // Check if the filter references the target table
                if self.filter_references_table(predicate, table_name) {
                    Some(predicate.clone())
                } else {
                    self.extract_pushable_filter(input, table_name)
                }
            }
            RelExpr::Project { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Distinct { input, .. } => self.extract_pushable_filter(input, table_name),
            _ => None,
        }
    }

    /// Check if a filter expression references a specific table.
    fn filter_references_table(&self, expr: &Expr, table_name: &str) -> bool {
        match expr {
            Expr::Column(col) => col.table.as_deref() == Some(table_name) || col.table.is_none(),
            Expr::BinOp { left, right, .. } => {
                self.filter_references_table(left, table_name)
                    || self.filter_references_table(right, table_name)
            }
            Expr::UnaryOp { operand, .. } => self.filter_references_table(operand, table_name),
            Expr::Function { args, .. } => args
                .iter()
                .any(|a| self.filter_references_table(a, table_name)),
            Expr::Const(_) => true,
            Expr::Case {
                operand,
                when_clauses,
                else_result,
            } => {
                operand
                    .as_ref()
                    .map_or(false, |o| self.filter_references_table(o, table_name))
                    || when_clauses.iter().any(|(c, r)| {
                        self.filter_references_table(c, table_name)
                            || self.filter_references_table(r, table_name)
                    })
                    || else_result
                        .as_ref()
                        .map_or(false, |e| self.filter_references_table(e, table_name))
            }
            Expr::Cast { expr, .. } => self.filter_references_table(expr, table_name),
            Expr::Array(elements) => elements
                .iter()
                .any(|e| self.filter_references_table(e, table_name)),
            Expr::ArrayIndex(array, index) => {
                self.filter_references_table(array, table_name)
                    || self.filter_references_table(index, table_name)
            }
            Expr::PatternPrev(inner, _)
            | Expr::PatternNext(inner, _)
            | Expr::PatternFirst(inner, _)
            | Expr::PatternLast(inner, _) => self.filter_references_table(inner, table_name),
            Expr::PatternClassifier | Expr::PatternMatchNumber => false,
            Expr::ArraySlice { array, start, end } => {
                self.filter_references_table(array, table_name)
                    || start
                        .as_ref()
                        .is_some_and(|s| self.filter_references_table(s, table_name))
                    || end
                        .as_ref()
                        .is_some_and(|e| self.filter_references_table(e, table_name))
            }
            Expr::FieldAccess { expr, .. } => self.filter_references_table(expr, table_name),
            Expr::SubQuery { test_expr, .. } => test_expr
                .as_ref()
                .map_or(false, |t| self.filter_references_table(t, table_name)),
            Expr::FullTextMatch { .. } => false,
            Expr::VectorDistance { column, target, .. } => {
                self.filter_references_table(column, table_name)
                    || self.filter_references_table(target, table_name)
            }
        }
    }

    /// Plan a hybrid execution: push down what we can, keep the
    /// rest local.
    fn plan_hybrid(
        &self,
        plan: &RelExpr,
        capabilities: &QueryCapabilities,
        table_name: &str,
    ) -> Option<(RelExpr, RelExpr)> {
        match plan {
            RelExpr::Filter { predicate, input } => {
                if capabilities.supports_filter_pushdown
                    && self.filter_references_table(predicate, table_name)
                {
                    // Push filter to remote, keep rest local
                    let remote = RelExpr::Filter {
                        predicate: predicate.clone(),
                        input: Box::new(
                            self.extract_scan(input, table_name)
                                .unwrap_or_else(|| RelExpr::scan(table_name)),
                        ),
                    };
                    let local = self
                        .remove_filter(plan, predicate)
                        .unwrap_or_else(|| RelExpr::scan(table_name));
                    Some((remote, local))
                } else {
                    None
                }
            }
            RelExpr::Project { columns, input } => {
                if capabilities.supports_project_pushdown {
                    let remote = RelExpr::Project {
                        columns: columns.clone(),
                        input: Box::new(
                            self.extract_scan(input, table_name)
                                .unwrap_or_else(|| RelExpr::scan(table_name)),
                        ),
                    };
                    let local = RelExpr::scan(table_name);
                    Some((remote, local))
                } else {
                    self.plan_hybrid(input, capabilities, table_name)
                }
            }
            RelExpr::Aggregate {
                group_by,
                aggregates,
                input,
            } => {
                if capabilities.supports_aggregate_pushdown {
                    let remote = RelExpr::Aggregate {
                        group_by: group_by.clone(),
                        aggregates: aggregates.clone(),
                        input: Box::new(
                            self.extract_scan(input, table_name)
                                .unwrap_or_else(|| RelExpr::scan(table_name)),
                        ),
                    };
                    let local = RelExpr::scan(table_name);
                    Some((remote, local))
                } else {
                    None
                }
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => {
                // For joins, try to push the remote side's scan
                // with a filter if possible
                if let Some(pred) = self.extract_pushable_filter(plan, table_name) {
                    let remote = RelExpr::Filter {
                        predicate: pred,
                        input: Box::new(RelExpr::scan(table_name)),
                    };
                    let local = RelExpr::Join {
                        join_type: *join_type,
                        condition: condition.clone(),
                        left: left.clone(),
                        right: right.clone(),
                    };
                    Some((remote, local))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract a scan node from a plan tree for a specific table.
    fn extract_scan(&self, plan: &RelExpr, table_name: &str) -> Option<RelExpr> {
        match plan {
            RelExpr::Scan { table, alias } => {
                if table == table_name {
                    Some(RelExpr::Scan {
                        table: table.clone(),
                        alias: alias.clone(),
                    })
                } else {
                    None
                }
            }
            RelExpr::Filter { input, .. }
            | RelExpr::Project { input, .. }
            | RelExpr::Aggregate { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Window { input, .. }
            | RelExpr::Distinct { input, .. } => self.extract_scan(input, table_name),
            RelExpr::Join { left, right, .. }
            | RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => self
                .extract_scan(left, table_name)
                .or_else(|| self.extract_scan(right, table_name)),
            _ => None,
        }
    }

    /// Remove a specific filter from a plan tree.
    fn remove_filter(&self, plan: &RelExpr, target_pred: &Expr) -> Option<RelExpr> {
        match plan {
            RelExpr::Filter { predicate, input } => {
                if predicate == target_pred {
                    Some(input.as_ref().clone())
                } else {
                    let inner = self.remove_filter(input, target_pred)?;
                    Some(RelExpr::Filter {
                        predicate: predicate.clone(),
                        input: Box::new(inner),
                    })
                }
            }
            _ => Some(plan.clone()),
        }
    }

    /// Create a local-only execution plan.
    fn plan_local(&self, query: &FederatedQuery) -> FederatedPlan {
        let cost = self
            .cost_model
            .estimate_local(query.sources.values().find_map(|s| s.statistics()));
        FederatedPlan {
            location: ExecutionLocation::Local {
                query: query.plan.clone(),
            },
            cost,
            alternatives: Vec::new(),
            steps: vec!["Execute entire query locally".into()],
        }
    }

    /// Generate human-readable execution steps.
    fn describe_steps(&self, location: &ExecutionLocation, _query: &FederatedQuery) -> Vec<String> {
        match location {
            ExecutionLocation::ShipQuery { target, .. } => {
                vec![
                    format!(
                        "Ship entire query to {} ({})",
                        target.endpoint, target.database_type
                    ),
                    "Execute query on remote database".into(),
                    "Transfer result set back to local engine".into(),
                ]
            }
            ExecutionLocation::ShipData {
                source,
                table,
                predicate,
            } => {
                let mut steps = Vec::new();
                if let Some(pred) = predicate {
                    steps.push(format!("Push filter to remote: WHERE {pred:?}"));
                    steps.push(format!("Fetch filtered {table} from {}", source.endpoint));
                } else {
                    steps.push(format!("Fetch entire {table} from {}", source.endpoint));
                }
                steps.push("Execute remaining query locally".into());
                steps
            }
            ExecutionLocation::Hybrid { target, .. } => {
                vec![
                    format!(
                        "Push down partial query to {} ({})",
                        target.endpoint, target.database_type
                    ),
                    "Fetch intermediate results".into(),
                    "Execute remaining operations locally".into(),
                ]
            }
            ExecutionLocation::Local { .. } => {
                vec!["Execute entire query locally".into()]
            }
        }
    }

    /// Analyze a federated query and return the optimization report.
    ///
    /// # Errors
    ///
    /// Returns an error if optimization fails.
    pub fn analyze(&self, query: &FederatedQuery) -> Result<FederatedAnalysis, FederatedError> {
        let plan = self.optimize_federated(query)?;

        let best_alternative_cost = plan.best_alternative().map(|a| a.total_ms);
        let savings = best_alternative_cost.map(|alt| plan.cost.savings_percent(alt));

        Ok(FederatedAnalysis {
            plan,
            best_alternative_cost,
            savings_percent: savings,
        })
    }
}

impl Default for FederatedOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Analysis result for a federated query.
#[derive(Debug, Clone)]
pub struct FederatedAnalysis {
    /// The optimized execution plan.
    pub plan: FederatedPlan,
    /// Cost of the best alternative strategy.
    pub best_alternative_cost: Option<f64>,
    /// Savings percentage compared to best alternative (negative
    /// means chosen strategy is cheaper).
    pub savings_percent: Option<f64>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ra_core::expr::{BinOp, ColumnRef, Const};
    use ra_core::federated::{DatabaseType, RemoteConnection};
    use ra_core::statistics::Statistics;

    use super::*;

    fn sample_connection() -> RemoteConnection {
        RemoteConnection::new(DatabaseType::PostgreSQL, "db.example.com:5432", 10, 100)
    }

    fn sample_stats() -> Statistics {
        let mut stats = Statistics::new(10_000_000.0);
        stats.avg_row_size = 200;
        stats.total_size = 2_000_000_000;
        stats
    }

    fn simple_filter_query() -> FederatedQuery {
        let plan = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("remote_table", "status"))),
                right: Box::new(Expr::Const(Const::String("ACTIVE".into()))),
            },
            input: Box::new(RelExpr::scan("remote_table")),
        };

        let mut sources = HashMap::new();
        sources.insert(
            "remote_table".into(),
            DataSource::remote(
                sample_connection(),
                "remote_table",
                Some(sample_stats()),
                QueryCapabilities::full(),
            ),
        );
        FederatedQuery::new(plan, sources)
    }

    fn join_query() -> FederatedQuery {
        let plan = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("local_table", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("remote_table", "id"))),
            },
            left: Box::new(RelExpr::scan("local_table")),
            right: Box::new(RelExpr::scan("remote_table")),
        };

        let mut sources = HashMap::new();
        sources.insert(
            "local_table".into(),
            DataSource::local("local_table", Statistics::new(1000.0)),
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
        FederatedQuery::new(plan, sources)
    }

    #[test]
    fn optimize_local_only() {
        let optimizer = FederatedOptimizer::new();
        let mut sources = HashMap::new();
        sources.insert("t".into(), DataSource::local("t", Statistics::new(100.0)));
        let query = FederatedQuery::new(RelExpr::scan("t"), sources);

        let plan = optimizer
            .optimize_federated(&query)
            .expect("should succeed");
        assert!(matches!(plan.location, ExecutionLocation::Local { .. }));
        assert!(plan.alternatives.is_empty());
    }

    #[test]
    fn optimize_no_sources_error() {
        let optimizer = FederatedOptimizer::new();
        let query = FederatedQuery::new(RelExpr::scan("t"), HashMap::new());
        let result = optimizer.optimize_federated(&query);
        assert!(result.is_err());
    }

    #[test]
    fn enumerate_strategies_includes_all_types() {
        let optimizer = FederatedOptimizer::new();
        let query = simple_filter_query();

        let strategies = optimizer.enumerate_strategies(&query);

        // Should have: local, ship_query, ship_data (full),
        // ship_data (filtered), hybrid
        assert!(strategies.len() >= 4);

        let has_local = strategies
            .iter()
            .any(|s| matches!(s, ExecutionLocation::Local { .. }));
        let has_ship_query = strategies
            .iter()
            .any(|s| matches!(s, ExecutionLocation::ShipQuery { .. }));
        let has_ship_data = strategies
            .iter()
            .any(|s| matches!(s, ExecutionLocation::ShipData { .. }));

        assert!(has_local);
        assert!(has_ship_query);
        assert!(has_ship_data);
    }

    #[test]
    fn can_ship_simple_filter() {
        let optimizer = FederatedOptimizer::new();
        let plan = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(RelExpr::scan("t")),
        };
        let caps = QueryCapabilities::full();
        assert!(optimizer.can_ship_query(&plan, &caps));
    }

    #[test]
    fn cannot_ship_to_minimal_capabilities() {
        let optimizer = FederatedOptimizer::new();
        let plan = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let caps = QueryCapabilities::minimal();
        assert!(!optimizer.can_ship_query(&plan, &caps));
    }

    #[test]
    fn cannot_ship_recursive_cte() {
        let optimizer = FederatedOptimizer::new();
        let plan = RelExpr::RecursiveCTE {
            name: "r".into(),
            base_case: Box::new(RelExpr::scan("t")),
            recursive_case: Box::new(RelExpr::scan("t")),
            body: Box::new(RelExpr::scan("r")),
            cycle_detection: None,
        };
        let caps = QueryCapabilities::full();
        assert!(!optimizer.can_ship_query(&plan, &caps));
    }

    #[test]
    fn optimize_filter_query_picks_strategy() {
        let optimizer = FederatedOptimizer::new();
        let query = simple_filter_query();

        let plan = optimizer
            .optimize_federated(&query)
            .expect("should succeed");

        assert!(!plan.steps.is_empty());
        assert!(plan.cost.total_ms > 0.0);
    }

    #[test]
    fn optimize_join_query() {
        let optimizer = FederatedOptimizer::new();
        let query = join_query();

        let plan = optimizer
            .optimize_federated(&query)
            .expect("should succeed");

        assert!(!plan.steps.is_empty());
        assert!(plan.cost.total_ms > 0.0);
    }

    #[test]
    fn analyze_produces_savings() {
        let optimizer = FederatedOptimizer::new();
        let query = simple_filter_query();

        let analysis = optimizer.analyze(&query).expect("should succeed");

        assert!(!analysis.plan.steps.is_empty());
    }

    #[test]
    fn describe_steps_ship_query() {
        let optimizer = FederatedOptimizer::new();
        let query = simple_filter_query();
        let location = ExecutionLocation::ShipQuery {
            target: sample_connection(),
            query: RelExpr::scan("t"),
        };

        let steps = optimizer.describe_steps(&location, &query);
        assert_eq!(steps.len(), 3);
        assert!(steps[0].contains("Ship entire query"));
    }

    #[test]
    fn describe_steps_ship_data_filtered() {
        let optimizer = FederatedOptimizer::new();
        let query = simple_filter_query();
        let location = ExecutionLocation::ShipData {
            source: sample_connection(),
            table: "orders".into(),
            predicate: Some(Expr::Const(Const::Bool(true))),
        };

        let steps = optimizer.describe_steps(&location, &query);
        assert!(steps.len() >= 2);
        assert!(steps[0].contains("Push filter"));
    }

    #[test]
    fn describe_steps_local() {
        let optimizer = FederatedOptimizer::new();
        let query = simple_filter_query();
        let location = ExecutionLocation::Local {
            query: RelExpr::scan("t"),
        };

        let steps = optimizer.describe_steps(&location, &query);
        assert_eq!(steps.len(), 1);
        assert!(steps[0].contains("locally"));
    }

    #[test]
    fn extract_pushable_filter_found() {
        let optimizer = FederatedOptimizer::new();
        let plan = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("orders", "status"))),
                right: Box::new(Expr::Const(Const::String("ACTIVE".into()))),
            },
            input: Box::new(RelExpr::scan("orders")),
        };

        let pred = optimizer.extract_pushable_filter(&plan, "orders");
        assert!(pred.is_some());
    }

    #[test]
    fn extract_pushable_filter_not_found() {
        let optimizer = FederatedOptimizer::new();
        let plan = RelExpr::scan("orders");

        let pred = optimizer.extract_pushable_filter(&plan, "orders");
        assert!(pred.is_none());
    }

    #[test]
    fn hybrid_plan_generated_for_filter() {
        let optimizer = FederatedOptimizer::new();
        let plan = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("remote", "id"))),
                right: Box::new(Expr::Const(Const::Int(42))),
            },
            input: Box::new(RelExpr::scan("remote")),
        };
        let caps = QueryCapabilities::full();

        let result = optimizer.plan_hybrid(&plan, &caps, "remote");
        assert!(result.is_some());
    }

    #[test]
    fn hybrid_plan_generated_for_aggregate() {
        let optimizer = FederatedOptimizer::new();
        let plan = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("category"))],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("remote")),
        };
        let caps = QueryCapabilities::full();

        let result = optimizer.plan_hybrid(&plan, &caps, "remote");
        assert!(result.is_some());
    }

    #[test]
    fn custom_cost_model() {
        let mut model = FederatedCostModel::new();
        model.remote_execution_overhead = 2.0;

        let optimizer = FederatedOptimizer::with_cost_model(model);
        let query = simple_filter_query();

        let plan = optimizer
            .optimize_federated(&query)
            .expect("should succeed");
        assert!(plan.cost.total_ms > 0.0);
    }
}
