//! Automatic index advisor for workload analysis and optimization.
//!
//! This module provides automated index recommendations based on
//! query workload analysis and table statistics. The key feature is
//! BRIN index recommendation for columns with high physical
//! correlation (e.g., timestamps, auto-increment IDs), where BRIN
//! indexes are 100-1000x smaller than B-tree alternatives.

pub mod candidate;
pub mod cost;

use std::collections::HashMap;

use ra_core::expr::{BinOp, ColumnRef};
use ra_core::statistics::{ColumnStats, Statistics};
use ra_core::{Expr, RelExpr};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

pub use candidate::{IndexCandidate, IndexType};
pub use cost::{IndexBenefit, IndexCost};

/// Minimum absolute correlation to recommend BRIN over B-tree.
/// From RFC 0066: |correlation| > 0.9 means the column is highly
/// correlated with physical row order.
const BRIN_CORRELATION_THRESHOLD: f64 = 0.9;

/// Minimum table size in bytes to recommend BRIN.
/// BRIN overhead is per-range, so small tables get no benefit.
/// RFC 0066 suggests 100 MB.
const BRIN_MIN_TABLE_SIZE: u64 = 100 * 1024 * 1024;

/// Default pages-per-range for BRIN recommendations.
const BRIN_DEFAULT_PAGES_PER_RANGE: u32 = 128;

/// Main index advisor that analyzes table statistics and generates
/// index recommendations, including BRIN for correlated columns.
#[derive(Debug)]
pub struct IndexAdvisor {
    /// Per-table statistics, keyed by table name.
    table_stats: HashMap<String, Statistics>,
}

/// A recommended index with its benefit/cost analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexRecommendation {
    /// The index candidate.
    pub candidate: IndexCandidate,
    /// Estimated benefit from creating this index.
    pub benefit: IndexBenefit,
    /// Estimated cost of creating and maintaining this index.
    pub cost: IndexCost,
    /// Benefit/cost ratio (higher is better).
    pub score: f64,
    /// Priority level based on score.
    pub priority: Priority,
}

/// Priority level for index recommendations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    /// Score > 10000: Very high benefit relative to cost.
    High,
    /// Score 1000-10000: Good benefit/cost ratio.
    Medium,
    /// Score < 1000: Lower priority but still beneficial.
    Low,
}

impl IndexAdvisor {
    /// Create a new index advisor with the given table statistics.
    #[must_use]
    pub fn new(table_stats: HashMap<String, Statistics>) -> Self {
        Self { table_stats }
    }

    /// Recommend BRIN indexes for all tables by examining column
    /// correlation statistics. This is the primary entry point
    /// for RFC 0066 BRIN recommendations.
    ///
    /// For each table, checks every column's correlation with
    /// physical row order. When |correlation| > 0.9 and the table
    /// is large enough, recommends BRIN instead of B-tree.
    pub fn recommend_brin_indexes(&self) -> Vec<IndexRecommendation> {
        let mut recommendations = Vec::new();

        for (table, stats) in &self.table_stats {
            let table_size = stats.total_size;

            for (col_name, col_stats) in &stats.columns {
                if let Some(rec) = self.evaluate_brin_candidate(
                    table,
                    col_name,
                    col_stats,
                    table_size,
                    stats.row_count,
                ) {
                    recommendations.push(rec);
                }
            }
        }

        recommendations.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        info!("Generated {} BRIN recommendations", recommendations.len());
        recommendations
    }

    /// Evaluate whether a column is a good BRIN index candidate.
    fn evaluate_brin_candidate(
        &self,
        table: &str,
        column: &str,
        col_stats: &ColumnStats,
        table_size: u64,
        row_count: f64,
    ) -> Option<IndexRecommendation> {
        let correlation = col_stats.correlation?;
        let abs_correlation = correlation.abs();

        // Check correlation threshold
        if abs_correlation < BRIN_CORRELATION_THRESHOLD {
            debug!(
                "{table}.{column}: correlation {correlation:.3} \
                 below threshold {BRIN_CORRELATION_THRESHOLD}"
            );
            return None;
        }

        // Check minimum table size
        if table_size < BRIN_MIN_TABLE_SIZE {
            debug!(
                "{table}: size {} below BRIN minimum {}",
                table_size, BRIN_MIN_TABLE_SIZE
            );
            return None;
        }

        // Check if a BRIN index already exists on this column
        if self.has_brin_index(table, column) {
            debug!("{table}.{column}: BRIN index already exists");
            return None;
        }

        let candidate = IndexCandidate::brin(table, column, correlation);

        // Estimate benefit: BRIN saves storage and scan cost
        let table_pages = table_size / 8192; // 8 KB pages
        let effectiveness = cost::estimate_brin_effectiveness(
            correlation,
            table_pages,
            BRIN_DEFAULT_PAGES_PER_RANGE,
            0.1, // Assume 10% average selectivity
        );

        let storage_savings = cost::brin_storage_savings(table_size);

        // Benefit from scan cost reduction + storage savings
        let total_cost_saved = row_count * effectiveness * 0.01 // scan savings
            + storage_savings * 10.0; // storage savings

        let benefit = IndexBenefit::new(
            vec![format!("{table}_range_scans")],
            storage_savings.min(1000.0),
            total_cost_saved,
        );

        let cost = IndexCost::estimate(IndexType::BRIN, table_size, row_count, 1);

        let score = if cost.total() > 0.0 {
            benefit.total_cost_saved / cost.total()
        } else {
            f64::MAX
        };

        let priority = match score {
            s if s > 10000.0 => Priority::High,
            s if s > 1000.0 => Priority::Medium,
            _ => Priority::Low,
        };

        info!(
            "Recommending BRIN on {table}.{column}: \
             correlation={correlation:.3}, \
             effectiveness={effectiveness:.2}, \
             storage_savings={storage_savings:.0}x, \
             score={score:.0}"
        );

        Some(IndexRecommendation {
            candidate,
            benefit,
            cost,
            score,
            priority,
        })
    }

    /// Check if a BRIN index already exists on a table column.
    fn has_brin_index(&self, table: &str, column: &str) -> bool {
        let Some(stats) = self.table_stats.get(table) else {
            return false;
        };

        stats.indexes.values().any(|idx| {
            idx.index_type == ra_core::facts::IndexType::Brin
                && idx.columns.contains(&column.to_string())
        })
    }

    /// Analyze a query plan and generate index candidates for
    /// columns used in predicates. Applies BRIN recommendations
    /// when column correlation is high.
    #[must_use]
    pub fn analyze_plan(&self, plan: &RelExpr) -> Vec<IndexRecommendation> {
        let mut columns = Vec::new();
        collect_filter_columns(plan, &mut columns);

        let mut recommendations = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for col_ref in &columns {
            let Some(table) = &col_ref.table else {
                continue;
            };
            let key = format!("{}.{}", table, col_ref.column);
            if !seen.insert(key) {
                continue;
            }

            let Some(stats) = self.table_stats.get(table.as_str()) else {
                continue;
            };

            let table_size = stats.total_size;
            let row_count = stats.row_count;

            if let Some(col_stats) = stats.columns.get(&col_ref.column) {
                // Try BRIN first for correlated columns
                if let Some(brin_rec) = self.evaluate_brin_candidate(
                    table,
                    &col_ref.column,
                    col_stats,
                    table_size,
                    row_count,
                ) {
                    recommendations.push(brin_rec);
                    continue;
                }
            }

            // Fall back to B-tree recommendation
            let candidate = IndexCandidate::btree(table.clone(), col_ref.column.clone());
            let cost = IndexCost::estimate(IndexType::BTree, stats.total_size, row_count, 1);
            let benefit = IndexBenefit::new(vec!["filter_scan".to_string()], 2.0, row_count * 0.1);

            let score = if cost.total() > 0.0 {
                benefit.total_cost_saved / cost.total()
            } else {
                0.0
            };

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

        recommendations.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        recommendations
    }

    /// Check if a column has range predicates in the given
    /// expression, which makes it a better BRIN candidate.
    #[must_use]
    pub fn has_range_predicate(expr: &Expr, column: &str) -> bool {
        match expr {
            Expr::BinOp { op, left, right } => {
                let is_range_op = matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge);

                if is_range_op {
                    let left_matches = matches!(
                        left.as_ref(),
                        Expr::Column(c) if c.column == column
                    );
                    let right_matches = matches!(
                        right.as_ref(),
                        Expr::Column(c) if c.column == column
                    );
                    if left_matches || right_matches {
                        return true;
                    }
                }

                // Recurse through AND/OR
                Self::has_range_predicate(left, column) || Self::has_range_predicate(right, column)
            }
            _ => false,
        }
    }
}

/// Collect column references used in filter predicates.
fn collect_filter_columns(plan: &RelExpr, columns: &mut Vec<ColumnRef>) {
    match plan {
        RelExpr::Filter { predicate, input } => {
            collect_columns_from_expr(predicate, columns);
            collect_filter_columns(input, columns);
        }
        RelExpr::Join {
            condition,
            left,
            right,
            ..
        } => {
            collect_columns_from_expr(condition, columns);
            collect_filter_columns(left, columns);
            collect_filter_columns(right, columns);
        }
        other => {
            for child in other.children() {
                collect_filter_columns(child, columns);
            }
        }
    }
}

/// Extract column references from a scalar expression.
fn collect_columns_from_expr(expr: &Expr, columns: &mut Vec<ColumnRef>) {
    match expr {
        Expr::Column(col_ref) => {
            columns.push(col_ref.clone());
        }
        Expr::BinOp { left, right, .. } => {
            collect_columns_from_expr(left, columns);
            collect_columns_from_expr(right, columns);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_columns_from_expr(operand, columns);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_columns_from_expr(arg, columns);
            }
        }
        _ => {}
    }
}

/// Determine whether BRIN is preferable to B-tree for a given column
/// based on correlation, table size, and access pattern.
#[must_use]
pub fn should_recommend_brin(correlation: f64, table_size_bytes: u64) -> bool {
    correlation.abs() > BRIN_CORRELATION_THRESHOLD && table_size_bytes >= BRIN_MIN_TABLE_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::expr::Const;
    use ra_core::statistics::{ColumnStats, Statistics};

    fn make_stats(
        row_count: f64,
        total_size: u64,
        columns: Vec<(&str, Option<f64>)>,
    ) -> Statistics {
        let mut stats = Statistics::new(row_count);
        stats.total_size = total_size;
        for (name, correlation) in columns {
            let mut col = ColumnStats::new(row_count);
            col.correlation = correlation;
            stats.columns.insert(name.to_string(), col);
        }
        stats
    }

    #[test]
    fn recommend_brin_for_highly_correlated_column() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "events".to_string(),
            make_stats(
                10_000_000.0,
                1_000_000_000, // 1 GB
                vec![
                    ("created_at", Some(0.98)),
                    ("user_id", Some(0.1)),
                    ("event_type", None),
                ],
            ),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();

        // Should recommend BRIN only for created_at (high
        // correlation)
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].candidate.columns, vec!["created_at"]);
        assert_eq!(recs[0].candidate.index_type, IndexType::BRIN);
        assert!(recs[0].candidate.reason.is_some());
    }

    #[test]
    fn no_brin_for_low_correlation() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "users".to_string(),
            make_stats(
                1_000_000.0,
                500_000_000,
                vec![("email", Some(0.1)), ("name", Some(0.3))],
            ),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();
        assert!(recs.is_empty());
    }

    #[test]
    fn no_brin_for_small_table() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "config".to_string(),
            make_stats(
                100.0,
                8192, // 8 KB -- too small
                vec![("id", Some(0.99))],
            ),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();
        assert!(recs.is_empty());
    }

    #[test]
    fn brin_for_negative_correlation() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "logs".to_string(),
            make_stats(5_000_000.0, 500_000_000, vec![("reverse_ts", Some(-0.95))]),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();

        // Negative correlation is equally useful
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].candidate.index_type, IndexType::BRIN);
    }

    #[test]
    fn skip_column_without_correlation_stats() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "data".to_string(),
            make_stats(10_000_000.0, 1_000_000_000, vec![("payload", None)]),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();
        assert!(recs.is_empty());
    }

    #[test]
    fn multiple_tables_multiple_brin() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "events".to_string(),
            make_stats(10_000_000.0, 1_000_000_000, vec![("ts", Some(0.99))]),
        );
        table_stats.insert(
            "metrics".to_string(),
            make_stats(
                50_000_000.0,
                5_000_000_000,
                vec![("recorded_at", Some(0.97))],
            ),
        );
        table_stats.insert(
            "users".to_string(),
            make_stats(
                100_000.0,
                10_000_000, // 10 MB, too small
                vec![("id", Some(0.99))],
            ),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();

        // events.ts and metrics.recorded_at, not users.id (too
        // small)
        assert_eq!(recs.len(), 2);
        let tables: Vec<&str> = recs.iter().map(|r| r.candidate.table.as_str()).collect();
        assert!(tables.contains(&"events"));
        assert!(tables.contains(&"metrics"));
    }

    #[test]
    fn brin_recommendation_has_sql() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "orders".to_string(),
            make_stats(
                10_000_000.0,
                2_000_000_000,
                vec![("order_date", Some(0.95))],
            ),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();

        assert_eq!(recs.len(), 1);
        let sql = recs[0].candidate.to_sql();
        assert!(sql.contains("USING BRIN"));
        assert!(sql.contains("order_date"));
        assert!(sql.contains("orders"));
    }

    #[test]
    fn brin_has_positive_score_for_large_tables() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "huge_table".to_string(),
            make_stats(
                100_000_000.0,
                10_000_000_000, // 10 GB
                vec![("auto_id", Some(0.999))],
            ),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();

        assert_eq!(recs.len(), 1);
        // BRIN should have meaningful positive score
        assert!(recs[0].score > 1.0);
        // Storage should be tiny compared to B-tree
        assert!(recs[0].cost.storage_bytes < 100_000_000);
        // Benefit should reflect scan savings
        assert!(recs[0].benefit.total_cost_saved > 0.0);
    }

    #[test]
    fn analyze_plan_recommends_brin_for_correlated_filter() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "events".to_string(),
            make_stats(
                10_000_000.0,
                1_000_000_000,
                vec![("created_at", Some(0.98)), ("user_id", Some(0.05))],
            ),
        );

        let advisor = IndexAdvisor::new(table_stats);

        // SELECT * FROM events WHERE created_at > '2024-01-01'
        let plan = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::qualified("events", "created_at"))),
                right: Box::new(Expr::Const(Const::String("2024-01-01".to_string()))),
            },
            input: Box::new(RelExpr::Scan {
                table: "events".to_string(),
                alias: None,
            }),
        };

        let recs = advisor.analyze_plan(&plan);

        // Should recommend BRIN for created_at
        assert!(!recs.is_empty());
        let brin_recs: Vec<_> = recs
            .iter()
            .filter(|r| r.candidate.index_type == IndexType::BRIN)
            .collect();
        assert_eq!(brin_recs.len(), 1);
        assert_eq!(brin_recs[0].candidate.columns, vec!["created_at"]);
    }

    #[test]
    fn analyze_plan_falls_back_to_btree() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "users".to_string(),
            make_stats(1_000_000.0, 500_000_000, vec![("email", Some(0.05))]),
        );

        let advisor = IndexAdvisor::new(table_stats);

        // SELECT * FROM users WHERE email = 'test@example.com'
        let plan = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("users", "email"))),
                right: Box::new(Expr::Const(Const::String("test@example.com".to_string()))),
            },
            input: Box::new(RelExpr::Scan {
                table: "users".to_string(),
                alias: None,
            }),
        };

        let recs = advisor.analyze_plan(&plan);

        // Should fall back to B-tree for uncorrelated column
        assert!(!recs.is_empty());
        assert_eq!(recs[0].candidate.index_type, IndexType::BTree);
    }

    #[test]
    fn has_range_predicate_detects_gt() {
        let expr = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("ts"))),
            right: Box::new(Expr::Const(Const::Int(100))),
        };
        assert!(IndexAdvisor::has_range_predicate(&expr, "ts"));
        assert!(!IndexAdvisor::has_range_predicate(&expr, "other"));
    }

    #[test]
    fn has_range_predicate_in_and() {
        let expr = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Ge,
                left: Box::new(Expr::Column(ColumnRef::new("ts"))),
                right: Box::new(Expr::Const(Const::Int(100))),
            }),
            right: Box::new(Expr::BinOp {
                op: BinOp::Le,
                left: Box::new(Expr::Column(ColumnRef::new("ts"))),
                right: Box::new(Expr::Const(Const::Int(200))),
            }),
        };
        assert!(IndexAdvisor::has_range_predicate(&expr, "ts"));
    }

    #[test]
    fn has_range_predicate_equality_is_false() {
        let expr = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(42))),
        };
        assert!(!IndexAdvisor::has_range_predicate(&expr, "id"));
    }

    #[test]
    fn should_recommend_brin_threshold() {
        // At the boundary
        assert!(!should_recommend_brin(0.9, BRIN_MIN_TABLE_SIZE));
        assert!(should_recommend_brin(0.91, BRIN_MIN_TABLE_SIZE));
        // Below min size
        assert!(!should_recommend_brin(0.99, 1024));
        // Both conditions met
        assert!(should_recommend_brin(0.95, 200_000_000));
        // Negative correlation
        assert!(should_recommend_brin(-0.95, 200_000_000));
    }

    #[test]
    fn skip_existing_brin_index() {
        let mut stats = make_stats(10_000_000.0, 1_000_000_000, vec![("ts", Some(0.99))]);

        // Add an existing BRIN index
        stats.indexes.insert(
            "idx_events_ts_brin".to_string(),
            ra_core::statistics::IndexStats::new(
                vec!["ts".to_string()],
                ra_core::facts::IndexType::Brin,
            ),
        );

        let mut table_stats = HashMap::new();
        table_stats.insert("events".to_string(), stats);

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();
        assert!(recs.is_empty());
    }

    #[test]
    fn recommend_brin_score_and_cost() {
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "timeseries".to_string(),
            make_stats(10_000_000.0, 1_000_000_000, vec![("ts", Some(0.99))]),
        );

        let advisor = IndexAdvisor::new(table_stats);
        let recs = advisor.recommend_brin_indexes();

        assert_eq!(recs.len(), 1);
        let rec = &recs[0];

        // Score should be positive
        assert!(rec.score > 0.0);

        // Cost should reflect tiny BRIN storage
        assert!(rec.cost.storage_bytes < 10_000_000);

        // Benefit should be meaningful
        assert!(rec.benefit.total_cost_saved > 0.0);
    }
}
