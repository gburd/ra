//! Query complexity classification for adaptive optimization strategies.
//!
//! Classifies queries by table count, join complexity, and other factors
//! to determine appropriate iteration limits and optimization strategies.

use ra_core::algebra::{RelExpr, JoinType};

/// Query complexity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueryComplexity {
    /// Single table (no joins).
    Trivial,
    /// 2-4 tables with simple joins.
    Simple,
    /// 5-7 tables, may have complex predicates.
    Medium,
    /// 8-9 tables with complex join conditions.
    Complex,
    /// 10+ tables, should use heuristic optimizer.
    VeryComplex,
}

impl QueryComplexity {
    /// Classify a query by analyzing its structure.
    pub fn from_expr(expr: &RelExpr) -> Self {
        let table_count = count_tables(expr);
        let join_count = count_joins(expr);
        let has_subqueries = contains_subqueries(expr);
        let outer_joins = count_outer_joins(expr);

        match table_count {
            0..=1 => Self::Trivial,
            2..=4 => {
                // Upgrade to Medium if complex features
                if join_count > 3 || has_subqueries || outer_joins > 0 {
                    Self::Medium
                } else {
                    Self::Simple
                }
            }
            5..=7 => Self::Medium,
            8..=9 => Self::Complex,
            _ => Self::VeryComplex,
        }
    }

    /// Get recommended iteration limit for this complexity level.
    pub fn default_iter_limit(self) -> usize {
        match self {
            Self::Trivial => 3,
            Self::Simple => 5,
            Self::Medium => 10,
            Self::Complex => 15,
            Self::VeryComplex => 20, // Or signal to use heuristic optimizer
        }
    }

    /// Get recommended timeout in milliseconds.
    pub fn default_timeout_ms(self) -> u64 {
        match self {
            Self::Trivial => 10,
            Self::Simple => 50,
            Self::Medium => 100,
            Self::Complex => 300,
            Self::VeryComplex => 500,
        }
    }
}

/// Count number of base tables (scans) in the query.
fn count_tables(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Scan { .. } => 1,
        RelExpr::Filter { input, .. } => count_tables(input),
        RelExpr::Project { input, .. } => count_tables(input),
        RelExpr::Join { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::Aggregate { input, .. } => count_tables(input),
        RelExpr::Sort { input, .. } => count_tables(input),
        RelExpr::Limit { input, .. } => count_tables(input),
        RelExpr::Union { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::Intersect { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::Except { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::Window { input, .. } => count_tables(input),
        RelExpr::Distinct { input } => count_tables(input),
        RelExpr::Values { .. } => 0, // Not a base table
        RelExpr::CTE { definition, body, .. } => count_tables(definition) + count_tables(body),
        RelExpr::RecursiveCTE { base_case, recursive_case, body, .. } => {
            count_tables(base_case) + count_tables(recursive_case) + count_tables(body)
        }
        // Other variants don't directly contain tables
        _ => 0,
    }
}

/// Count number of join operations in the query.
fn count_joins(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Join { left, right, .. } => 1 + count_joins(left) + count_joins(right),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => count_joins(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => count_joins(left) + count_joins(right),
        RelExpr::CTE { definition, body, .. } => count_joins(definition) + count_joins(body),
        RelExpr::RecursiveCTE { base_case, recursive_case, body, .. } => {
            count_joins(base_case) + count_joins(recursive_case) + count_joins(body)
        }
        _ => 0,
    }
}

/// Check if query contains subqueries (CTEs).
fn contains_subqueries(expr: &RelExpr) -> bool {
    matches!(
        expr,
        RelExpr::CTE { .. } | RelExpr::RecursiveCTE { .. }
    ) || match expr {
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => contains_subqueries(input),
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            contains_subqueries(left) || contains_subqueries(right)
        }
        _ => false,
    }
}

/// Count number of outer joins (more expensive than inner joins).
fn count_outer_joins(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            let is_outer = matches!(
                join_type,
                JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter
            );
            let count = if is_outer { 1 } else { 0 };
            count + count_outer_joins(left) + count_outer_joins(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => count_outer_joins(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => count_outer_joins(left) + count_outer_joins(right),
        RelExpr::CTE { definition, body, .. } => count_outer_joins(definition) + count_outer_joins(body),
        RelExpr::RecursiveCTE { base_case, recursive_case, body, .. } => {
            count_outer_joins(base_case) + count_outer_joins(recursive_case) + count_outer_joins(body)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_string(),
            alias: None,
        }
    }

    fn join(left: RelExpr, right: RelExpr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn test_trivial_complexity() {
        let expr = scan("users");
        assert_eq!(QueryComplexity::from_expr(&expr), QueryComplexity::Trivial);
        assert_eq!(QueryComplexity::Trivial.default_iter_limit(), 3);
    }

    #[test]
    fn test_simple_complexity() {
        let expr = join(scan("users"), scan("orders"));
        assert_eq!(QueryComplexity::from_expr(&expr), QueryComplexity::Simple);
        assert_eq!(QueryComplexity::Simple.default_iter_limit(), 5);
    }

    #[test]
    fn test_medium_complexity() {
        let expr = join(
            join(scan("users"), scan("orders")),
            join(scan("products"), scan("categories")),
        );
        // 4 tables with 3 joins
        assert_eq!(QueryComplexity::from_expr(&expr), QueryComplexity::Medium);
        assert_eq!(QueryComplexity::Medium.default_iter_limit(), 10);
    }

    #[test]
    fn test_complex_complexity() {
        let mut expr = join(scan("t1"), scan("t2"));
        for i in 3..=8 {
            expr = join(expr, scan(&format!("t{}", i)));
        }
        assert_eq!(QueryComplexity::from_expr(&expr), QueryComplexity::Complex);
        assert_eq!(QueryComplexity::Complex.default_iter_limit(), 15);
    }

    #[test]
    fn test_very_complex() {
        let mut expr = join(scan("t1"), scan("t2"));
        for i in 3..=11 {
            expr = join(expr, scan(&format!("t{}", i)));
        }
        assert_eq!(
            QueryComplexity::from_expr(&expr),
            QueryComplexity::VeryComplex
        );
        assert_eq!(QueryComplexity::VeryComplex.default_iter_limit(), 20);
    }

    #[test]
    fn test_outer_join_upgrade() {
        let expr = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(scan("users")),
            right: Box::new(scan("orders")),
        };
        // 2 tables but with outer join -> Medium
        assert_eq!(QueryComplexity::from_expr(&expr), QueryComplexity::Medium);
    }

    #[test]
    fn test_timeout_values() {
        assert_eq!(QueryComplexity::Trivial.default_timeout_ms(), 10);
        assert_eq!(QueryComplexity::Simple.default_timeout_ms(), 50);
        assert_eq!(QueryComplexity::Medium.default_timeout_ms(), 100);
        assert_eq!(QueryComplexity::Complex.default_timeout_ms(), 300);
        assert_eq!(QueryComplexity::VeryComplex.default_timeout_ms(), 500);
    }
}
