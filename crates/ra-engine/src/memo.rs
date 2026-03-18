//! Memo table for caching optimization results.
//!
//! Provides a thin wrapper around the e-graph that acts as a
//! memoization cache for previously-optimized expressions. This
//! avoids re-running equality saturation for queries (or subqueries)
//! that have already been explored.

use std::collections::HashMap;

use ra_core::algebra::RelExpr;

/// A memo table that caches optimization results.
///
/// Maps a structural hash of the input expression to the optimized
/// output. This avoids redundant optimization of repeated subqueries.
#[derive(Debug, Default)]
pub struct MemoTable {
    cache: HashMap<u64, RelExpr>,
}

impl MemoTable {
    /// Create an empty memo table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a cached result for the given expression hash.
    #[must_use]
    pub fn get(&self, hash: u64) -> Option<&RelExpr> {
        self.cache.get(&hash)
    }

    /// Store an optimization result.
    pub fn insert(&mut self, hash: u64, result: RelExpr) {
        self.cache.insert(hash, result);
    }

    /// Return the number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Return whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clear all cached entries.
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

/// Compute a structural hash for a relational expression.
///
/// This is used as the memo table key. It hashes the structure
/// and leaf values of the expression tree.
#[must_use]
pub fn structural_hash(expr: &RelExpr) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut hasher = DefaultHasher::new();
    hash_rel_expr(expr, &mut hasher);
    hasher.finish()
}

fn hash_rel_expr(expr: &RelExpr, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    std::mem::discriminant(expr).hash(hasher);
    match expr {
        RelExpr::Scan { table, alias } => {
            table.hash(hasher);
            alias.hash(hasher);
        }
        RelExpr::Filter { predicate, input } => {
            hash_scalar_expr(predicate, hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::Project { columns, input } => {
            columns.len().hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            join_type.hash(hasher);
            hash_scalar_expr(condition, hasher);
            hash_rel_expr(left, hasher);
            hash_rel_expr(right, hasher);
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            group_by.len().hash(hasher);
            aggregates.len().hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::Sort { keys, input } => {
            keys.len().hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            count.hash(hasher);
            offset.hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::Union { all, left, right }
        | RelExpr::Intersect { all, left, right }
        | RelExpr::Except { all, left, right } => {
            all.hash(hasher);
            hash_rel_expr(left, hasher);
            hash_rel_expr(right, hasher);
        }
        RelExpr::Cte {
            name,
            definition,
            body,
        } => {
            name.hash(hasher);
            hash_rel_expr(definition, hasher);
            hash_rel_expr(body, hasher);
        }
        RelExpr::Window { functions, input } => {
            functions.len().hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::Distinct { input } => {
            hash_rel_expr(input, hasher);
        }
        RelExpr::Values { rows } => {
            rows.len().hash(hasher);
        }
    }
}

fn hash_scalar_expr(expr: &ra_core::expr::Expr, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    std::mem::discriminant(expr).hash(hasher);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn memo_table_insert_and_get() {
        let mut memo = MemoTable::new();
        let expr = RelExpr::scan("users");
        let hash = structural_hash(&expr);
        memo.insert(hash, expr.clone());
        assert_eq!(memo.get(hash), Some(&expr));
    }

    #[test]
    fn memo_table_empty() {
        let memo = MemoTable::new();
        assert!(memo.is_empty());
        assert_eq!(memo.len(), 0);
    }

    #[test]
    fn memo_table_clear() {
        let mut memo = MemoTable::new();
        let expr = RelExpr::scan("t");
        memo.insert(structural_hash(&expr), expr);
        assert_eq!(memo.len(), 1);
        memo.clear();
        assert!(memo.is_empty());
    }

    #[test]
    fn structural_hash_differs_for_different_exprs() {
        let scan_a = RelExpr::scan("a");
        let scan_b = RelExpr::scan("b");
        assert_ne!(structural_hash(&scan_a), structural_hash(&scan_b));
    }

    #[test]
    fn structural_hash_same_for_same_exprs() {
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });
        assert_eq!(structural_hash(&expr), structural_hash(&expr));
    }
}
