//! Automatic index advisor for workload analysis and optimization
//!
//! This module provides automated index recommendations based on query workload analysis.
//! It analyzes SQL queries, generates index candidates, estimates their benefits and costs,
//! and recommends the most beneficial indexes to create.

pub mod candidate;
pub mod cost;
pub mod workload;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use ra_core::{Expr, RelExpr};
use ra_engine::Optimizer;
use ra_metadata::schema::SchemaInfo;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::{debug, info};

pub use candidate::{IndexCandidate, IndexType};
pub use cost::{IndexBenefit, IndexCost};
pub use workload::{Query, QueryId, Workload};

/// Main index advisor that coordinates workload analysis and recommendation generation
#[derive(Debug)]
pub struct IndexAdvisor {
    /// Schema information including tables and existing indexes
    schema: SchemaInfo,
    /// Query optimizer for cost estimation
    optimizer: Optimizer,
    /// Statistics for accurate cost estimation
    stats: ra_stats::Statistics,
}

/// A recommended index with its benefit/cost analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexRecommendation {
    /// The index candidate
    pub candidate: IndexCandidate,
    /// Estimated benefit from creating this index
    pub benefit: IndexBenefit,
    /// Estimated cost of creating and maintaining this index
    pub cost: IndexCost,
    /// Benefit/cost ratio (higher is better)
    pub score: f64,
    /// Priority level based on score
    pub priority: Priority,
}

/// Priority level for index recommendations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    /// Score > 10000: Very high benefit relative to cost
    High,
    /// Score 1000-10000: Good benefit/cost ratio
    Medium,
    /// Score < 1000: Lower priority but still beneficial
    Low,
}

impl IndexAdvisor {
    /// Create a new index advisor with the given schema and statistics
    pub fn new(schema: SchemaInfo, stats: ra_stats::Statistics) -> Self {
        let optimizer = Optimizer::new();
        Self {
            schema,
            optimizer,
            stats,
        }
    }

    /// Analyze a workload and generate index recommendations
    pub fn analyze(&self, workload: &Workload) -> Result<Vec<IndexRecommendation>> {
        info!("Analyzing workload with {} queries", workload.queries.len());

        // Generate index candidates
        let candidates = self.generate_candidates(workload)?;
        info!("Generated {} index candidates", candidates.len());

        // Evaluate each candidate
        let mut recommendations = Vec::new();
        for candidate in candidates {
            debug!("Evaluating candidate: {:?}", candidate);

            // Estimate benefit across all queries
            let benefit = self.estimate_benefit(&candidate, workload)?;

            // Estimate cost
            let cost = self.estimate_cost(&candidate)?;

            // Calculate score
            let score = if cost.total() > 0.0 {
                benefit.total_cost_saved / cost.total()
            } else {
                f64::INFINITY
            };

            // Determine priority
            let priority = match score {
                s if s > 10000.0 => Priority::High,
                s if s > 1000.0 => Priority::Medium,
                _ => Priority::Low,
            };

            recommendations.push(IndexRecommendation {
                candidate,
                benefit,
                cost,
                score,
                priority,
            });
        }

        // Sort by score (highest first) and remove redundant indexes
        recommendations.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        let recommendations = self.remove_redundant_indexes(recommendations);

        info!("Generated {} recommendations", recommendations.len());
        Ok(recommendations)
    }

    /// Generate index candidates from workload analysis
    pub fn generate_candidates(&self, workload: &Workload) -> Result<Vec<IndexCandidate>> {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        for query in &workload.queries {
            // Extract columns used in different contexts
            let filter_columns = self.extract_filter_columns(&query.parsed_query)?;
            let join_columns = self.extract_join_columns(&query.parsed_query)?;
            let sort_columns = self.extract_sort_columns(&query.parsed_query)?;

            // Single-column indexes for filters
            for col in filter_columns {
                let candidate = IndexCandidate {
                    table: col.table.clone(),
                    columns: vec![col.column.clone()],
                    index_type: IndexType::BTree,
                    unique: false,
                    partial_predicate: None,
                };

                let key = format!("{}.{}", candidate.table, candidate.columns.join(","));
                if seen.insert(key) {
                    candidates.push(candidate);
                }
            }

            // Single-column indexes for joins
            for (left, right) in join_columns {
                // Index on left side
                let left_candidate = IndexCandidate {
                    table: left.table.clone(),
                    columns: vec![left.column.clone()],
                    index_type: IndexType::BTree,
                    unique: false,
                    partial_predicate: None,
                };

                let key = format!("{}.{}", left_candidate.table, left_candidate.columns.join(","));
                if seen.insert(key.clone()) {
                    candidates.push(left_candidate);
                }

                // Index on right side
                let right_candidate = IndexCandidate {
                    table: right.table.clone(),
                    columns: vec![right.column.clone()],
                    index_type: IndexType::BTree,
                    unique: false,
                    partial_predicate: None,
                };

                let key = format!("{}.{}", right_candidate.table, right_candidate.columns.join(","));
                if seen.insert(key) {
                    candidates.push(right_candidate);
                }
            }

            // Composite indexes (two-column combinations)
            let composite_candidates = self.generate_composite_candidates(&query.parsed_query)?;
            for candidate in composite_candidates {
                let key = format!("{}.{}", candidate.table, candidate.columns.join(","));
                if seen.insert(key) {
                    candidates.push(candidate);
                }
            }
        }

        Ok(candidates)
    }

    /// Extract columns used in WHERE clauses
    fn extract_filter_columns(&self, plan: &RelExpr) -> Result<Vec<ColumnRef>> {
        let mut columns = Vec::new();
        self.walk_plan_for_filters(plan, &mut columns)?;
        Ok(columns)
    }

    /// Recursively walk the plan to find filter predicates
    fn walk_plan_for_filters(&self, plan: &RelExpr, columns: &mut Vec<ColumnRef>) -> Result<()> {
        match plan {
            RelExpr::Filter { predicate, input } => {
                self.extract_columns_from_expr(predicate, columns)?;
                self.walk_plan_for_filters(input, columns)?;
            }
            RelExpr::Join { left, right, on, .. } => {
                if let Some(on_expr) = on {
                    self.extract_columns_from_expr(on_expr, columns)?;
                }
                self.walk_plan_for_filters(left, columns)?;
                self.walk_plan_for_filters(right, columns)?;
            }
            RelExpr::Scan { .. } => {
                // Base case
            }
            _ => {
                // For other plan types, recurse into children
                for child in plan.children() {
                    self.walk_plan_for_filters(child, columns)?;
                }
            }
        }
        Ok(())
    }

    /// Extract column references from an expression
    fn extract_columns_from_expr(&self, expr: &Expr, columns: &mut Vec<ColumnRef>) -> Result<()> {
        match expr {
            Expr::Column { table, name, .. } => {
                if let Some(table) = table {
                    columns.push(ColumnRef {
                        table: table.clone(),
                        column: name.clone(),
                    });
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.extract_columns_from_expr(left, columns)?;
                self.extract_columns_from_expr(right, columns)?;
            }
            Expr::UnaryOp { operand, .. } => {
                self.extract_columns_from_expr(operand, columns)?;
            }
            Expr::InList { expr, list, .. } => {
                self.extract_columns_from_expr(expr, columns)?;
                for item in list {
                    self.extract_columns_from_expr(item, columns)?;
                }
            }
            _ => {
                // Other expression types don't contain column references we care about
            }
        }
        Ok(())
    }

    /// Extract columns used in JOIN conditions
    fn extract_join_columns(&self, plan: &LogicalPlan) -> Result<Vec<(ColumnRef, ColumnRef)>> {
        let mut join_pairs = Vec::new();
        self.walk_plan_for_joins(plan, &mut join_pairs)?;
        Ok(join_pairs)
    }

    /// Recursively walk the plan to find join conditions
    fn walk_plan_for_joins(&self, plan: &LogicalPlan, pairs: &mut Vec<(ColumnRef, ColumnRef)>) -> Result<()> {
        match plan {
            LogicalPlan::Join { left, right, on, .. } => {
                if let Some(on_expr) = on {
                    self.extract_join_pairs_from_expr(on_expr, pairs)?;
                }
                self.walk_plan_for_joins(left, pairs)?;
                self.walk_plan_for_joins(right, pairs)?;
            }
            _ => {
                // For other plan types, recurse into children
                for child in plan.children() {
                    self.walk_plan_for_joins(child, pairs)?;
                }
            }
        }
        Ok(())
    }

    /// Extract join column pairs from a join condition expression
    fn extract_join_pairs_from_expr(&self, expr: &Expr, pairs: &mut Vec<(ColumnRef, ColumnRef)>) -> Result<()> {
        match expr {
            Expr::BinaryOp { left, op, right } if op.is_comparison() => {
                if let (Expr::Column { table: Some(t1), name: n1, .. },
                        Expr::Column { table: Some(t2), name: n2, .. }) = (left.as_ref(), right.as_ref()) {
                    pairs.push((
                        ColumnRef { table: t1.clone(), column: n1.clone() },
                        ColumnRef { table: t2.clone(), column: n2.clone() },
                    ));
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.extract_join_pairs_from_expr(left, pairs)?;
                self.extract_join_pairs_from_expr(right, pairs)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Extract columns used in ORDER BY/GROUP BY
    fn extract_sort_columns(&self, plan: &LogicalPlan) -> Result<Vec<ColumnRef>> {
        let mut columns = Vec::new();
        self.walk_plan_for_sorts(plan, &mut columns)?;
        Ok(columns)
    }

    /// Recursively walk the plan to find sort columns
    fn walk_plan_for_sorts(&self, plan: &LogicalPlan, columns: &mut Vec<ColumnRef>) -> Result<()> {
        match plan {
            LogicalPlan::Sort { exprs, input } => {
                for expr in exprs {
                    self.extract_columns_from_expr(expr, columns)?;
                }
                self.walk_plan_for_sorts(input, columns)?;
            }
            LogicalPlan::Aggregate { group_exprs, input, .. } => {
                for expr in group_exprs {
                    self.extract_columns_from_expr(expr, columns)?;
                }
                self.walk_plan_for_sorts(input, columns)?;
            }
            _ => {
                // For other plan types, recurse into children
                for child in plan.children() {
                    self.walk_plan_for_sorts(child, columns)?;
                }
            }
        }
        Ok(())
    }

    /// Generate composite index candidates (two-column combinations)
    fn generate_composite_candidates(&self, plan: &LogicalPlan) -> Result<Vec<IndexCandidate>> {
        let mut candidates = Vec::new();

        // Find filters and sorts in the same query
        let filter_cols = self.extract_filter_columns(plan)?;
        let sort_cols = self.extract_sort_columns(plan)?;

        // Group columns by table
        let mut by_table: HashMap<String, Vec<String>> = HashMap::new();

        for col in &filter_cols {
            by_table.entry(col.table.clone())
                .or_default()
                .push(col.column.clone());
        }

        for col in &sort_cols {
            by_table.entry(col.table.clone())
                .or_default()
                .push(col.column.clone());
        }

        // Create two-column combinations for each table
        for (table, columns) in by_table {
            let unique_cols: Vec<_> = columns.into_iter().collect::<HashSet<_>>().into_iter().collect();

            if unique_cols.len() >= 2 {
                // Take first two most frequently used columns
                candidates.push(IndexCandidate {
                    table: table.clone(),
                    columns: vec![unique_cols[0].clone(), unique_cols[1].clone()],
                    index_type: IndexType::BTree,
                    unique: false,
                    partial_predicate: None,
                });
            }
        }

        Ok(candidates)
    }

    /// Estimate the benefit of creating an index
    pub fn estimate_benefit(&self, candidate: &IndexCandidate, workload: &Workload) -> Result<IndexBenefit> {
        let mut affected_queries = Vec::new();
        let mut total_cost_saved = 0.0;
        let mut speedups = Vec::new();

        for query in &workload.queries {
            // Optimize query without index
            let plan_without = self.optimizer.optimize(&query.parsed_query)?;
            let cost_without = self.estimate_plan_cost(&plan_without)?;

            // Add hypothetical index and re-optimize
            let mut schema_with_index = self.schema.clone();
            schema_with_index.add_hypothetical_index(candidate);

            let optimizer_with_index = Optimizer::new(schema_with_index);
            let plan_with = optimizer_with_index.optimize(&query.parsed_query)?;
            let cost_with = self.estimate_plan_cost(&plan_with)?;

            // Calculate benefit for this query
            let cost_saved = cost_without - cost_with;
            if cost_saved > 0.0 {
                affected_queries.push(query.id.clone());
                let speedup = cost_without / cost_with;
                speedups.push(speedup);

                // Weight by query frequency
                total_cost_saved += cost_saved * query.frequency as f64;
            }
        }

        let avg_speedup = if !speedups.is_empty() {
            speedups.iter().sum::<f64>() / speedups.len() as f64
        } else {
            1.0
        };

        Ok(IndexBenefit {
            affected_queries,
            avg_speedup,
            total_cost_saved,
        })
    }

    /// Estimate the cost of a logical plan
    fn estimate_plan_cost(&self, plan: &LogicalPlan) -> Result<f64> {
        // Simplified cost estimation
        // In a real implementation, this would use the statistics module
        match plan {
            LogicalPlan::Scan { table, .. } => {
                // Base cost for scanning a table
                let rows = self.stats.get_table_rows(table).unwrap_or(1000.0);
                Ok(rows * 1.0) // 1 cost unit per row
            }
            LogicalPlan::IndexScan { table, .. } => {
                // Lower cost for index scan
                let rows = self.stats.get_table_rows(table).unwrap_or(1000.0);
                Ok(rows * 0.1) // 0.1 cost unit per row (10x faster than table scan)
            }
            LogicalPlan::Filter { input, predicate } => {
                let input_cost = self.estimate_plan_cost(input)?;
                let selectivity = self.estimate_selectivity(predicate)?;
                Ok(input_cost * selectivity)
            }
            LogicalPlan::Join { left, right, .. } => {
                let left_cost = self.estimate_plan_cost(left)?;
                let right_cost = self.estimate_plan_cost(right)?;
                // Nested loop join cost approximation
                Ok(left_cost + (left_cost * right_cost * 0.01))
            }
            LogicalPlan::Sort { input, .. } => {
                let input_cost = self.estimate_plan_cost(input)?;
                // n log n cost for sorting
                Ok(input_cost * (input_cost.log2() + 1.0))
            }
            _ => {
                // For other operators, just sum child costs
                let mut total = 0.0;
                for child in plan.children() {
                    total += self.estimate_plan_cost(child)?;
                }
                Ok(total)
            }
        }
    }

    /// Estimate selectivity of a predicate (fraction of rows that pass)
    fn estimate_selectivity(&self, _predicate: &Expr) -> Result<f64> {
        // Simplified selectivity estimation
        // Real implementation would analyze the predicate type and use statistics
        Ok(0.3) // Assume 30% selectivity for now
    }

    /// Estimate the cost of creating and maintaining an index
    pub fn estimate_cost(&self, candidate: &IndexCandidate) -> Result<IndexCost> {
        // Get table statistics
        let table_rows = self.stats.get_table_rows(&candidate.table).unwrap_or(1000.0);

        // Storage cost estimation
        let bytes_per_entry = 8 + (candidate.columns.len() * 20); // Simplified
        let storage_bytes = (table_rows * bytes_per_entry as f64) as u64;

        // Write overhead (more columns = more overhead)
        let write_overhead = 0.05 * candidate.columns.len() as f64;

        // Build time estimation (simplified)
        let build_time_secs = table_rows / 100000.0; // 100k rows per second

        Ok(IndexCost {
            storage_bytes,
            write_overhead,
            build_time_secs,
        })
    }

    /// Remove redundant indexes from recommendations
    fn remove_redundant_indexes(&self, mut recommendations: Vec<IndexRecommendation>) -> Vec<IndexRecommendation> {
        let mut kept = Vec::new();
        let mut covered_patterns: HashSet<String> = HashSet::new();

        for rec in recommendations {
            let pattern = format!("{}.{}", rec.candidate.table, rec.candidate.columns[0]);

            // Check if this index is redundant
            let mut is_redundant = false;

            // A single-column index is redundant if a composite index starts with the same column
            if rec.candidate.columns.len() == 1 {
                for existing in &kept {
                    if existing.candidate.table == rec.candidate.table
                        && existing.candidate.columns.len() > 1
                        && existing.candidate.columns[0] == rec.candidate.columns[0] {
                        is_redundant = true;
                        break;
                    }
                }
            }

            if !is_redundant && covered_patterns.insert(pattern) {
                kept.push(rec);
            }
        }

        kept
    }
}

/// Column reference in a query
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ColumnRef {
    table: String,
    column: String,
}