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
        RelExpr::IndexScan { table, column } => {
            table.hash(hasher);
            column.hash(hasher);
        }
        RelExpr::IndexOnlyScan { table, index, columns, predicate } => {
            table.hash(hasher);
            index.hash(hasher);
            columns.len().hash(hasher);
            hash_scalar_expr(predicate, hasher);
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
        RelExpr::CTE {
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
        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            cycle_detection,
        } => {
            name.hash(hasher);
            hash_rel_expr(base_case, hasher);
            hash_rel_expr(recursive_case, hasher);
            hash_rel_expr(body, hasher);
            if let Some(cd) = cycle_detection {
                cd.track_columns.hash(hasher);
                cd.max_depth.hash(hasher);
            }
        }
        RelExpr::Unnest {
            expr, alias, input, with_ordinality,
        } => {
            hash_scalar_expr(expr, hasher);
            alias.hash(hasher);
            with_ordinality.hash(hasher);
            if let Some(inp) = input {
                hash_rel_expr(inp, hasher);
            }
        }
        RelExpr::MultiUnnest {
            exprs, aliases, with_ordinality,
        } => {
            exprs.len().hash(hasher);
            for expr in exprs {
                hash_scalar_expr(expr, hasher);
            }
            aliases.hash(hasher);
            with_ordinality.hash(hasher);
        }
        RelExpr::TableFunction {
            name, args, input, ..
        } => {
            name.hash(hasher);
            args.len().hash(hasher);
            if let Some(inp) = input {
                hash_rel_expr(inp, hasher);
            }
        }
        RelExpr::RowPattern {
            input,
            partition_by,
            order_by,
            defines,
            measures,
            ..
        } => {
            partition_by.len().hash(hasher);
            order_by.len().hash(hasher);
            defines.len().hash(hasher);
            measures.len().hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::IncrementalSort {
            prefix_keys,
            suffix_keys,
            input,
        } => {
            prefix_keys.len().hash(hasher);
            suffix_keys.len().hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::BitmapIndexScan {
            table, index, predicate,
        } => {
            table.hash(hasher);
            index.hash(hasher);
            hash_scalar_expr(predicate, hasher);
        }
        RelExpr::BitmapAnd { inputs }
        | RelExpr::BitmapOr { inputs } => {
            inputs.len().hash(hasher);
            for inp in inputs {
                hash_rel_expr(inp, hasher);
            }
        }
        RelExpr::BitmapHeapScan {
            table, bitmap, recheck_cond,
        } => {
            table.hash(hasher);
            hash_rel_expr(bitmap, hasher);
            if let Some(cond) = recheck_cond {
                hash_scalar_expr(cond, hasher);
            }
        }
        RelExpr::ParallelScan { table, workers } => {
            table.hash(hasher);
            workers.hash(hasher);
        }
        RelExpr::ParallelHashJoin {
            join_type,
            condition,
            left,
            right,
            workers,
        } => {
            join_type.hash(hasher);
            hash_scalar_expr(condition, hasher);
            hash_rel_expr(left, hasher);
            hash_rel_expr(right, hasher);
            workers.hash(hasher);
        }
        RelExpr::ParallelAggregate {
            group_by,
            aggregates,
            input,
            workers,
        } => {
            group_by.len().hash(hasher);
            aggregates.len().hash(hasher);
            hash_rel_expr(input, hasher);
            workers.hash(hasher);
        }
        RelExpr::Gather { input, workers } => {
            hash_rel_expr(input, hasher);
            workers.hash(hasher);
        }
        RelExpr::MvScan { view_name, alias } => {
            view_name.hash(hasher);
            alias.hash(hasher);
        }
        RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k,
            input,
        } => {
            hash_scalar_expr(vector_expr, hasher);
            hash_scalar_expr(query_vector, hasher);
            std::mem::discriminant(metric).hash(hasher);
            k.hash(hasher);
            hash_rel_expr(input, hasher);
        }
        RelExpr::VectorFilter {
            vector_expr,
            query_vector,
            metric,
            threshold,
            input,
        } => {
            hash_scalar_expr(vector_expr, hasher);
            hash_scalar_expr(query_vector, hasher);
            std::mem::discriminant(metric).hash(hasher);
            // Float hashing is tricky, use bits
            threshold.to_bits().hash(hasher);
            hash_rel_expr(input, hasher);
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

    #[test]
    fn structural_hash_filter_different_expr_kinds() {
        // hash_scalar_expr only hashes discriminant, so two BinOps
        // with different operators hash the same. But a BinOp vs a
        // Column should differ.
        let e1 = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });
        let e2 = RelExpr::scan("t").filter(Expr::Column(
            ColumnRef::new("flag"),
        ));
        assert_ne!(structural_hash(&e1), structural_hash(&e2));
    }

    #[test]
    fn structural_hash_join() {
        let cond = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Column(ColumnRef::new("id"))),
        };
        let plan = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: cond,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let h1 = structural_hash(&plan);
        let h2 = structural_hash(&plan);
        assert_eq!(h1, h2);
    }

    #[test]
    fn structural_hash_aggregate() {
        let plan = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        let h = structural_hash(&plan);
        assert_ne!(h, structural_hash(&RelExpr::scan("t")));
    }

    #[test]
    fn structural_hash_sort() {
        let plan = RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        let h = structural_hash(&plan);
        assert_ne!(h, structural_hash(&RelExpr::scan("t")));
    }

    #[test]
    fn structural_hash_limit() {
        let plan1 = RelExpr::scan("t").limit(10, 0);
        let plan2 = RelExpr::scan("t").limit(20, 0);
        assert_ne!(structural_hash(&plan1), structural_hash(&plan2));
    }

    #[test]
    fn structural_hash_distinct() {
        let plan = RelExpr::scan("t").distinct();
        let h = structural_hash(&plan);
        assert_ne!(h, structural_hash(&RelExpr::scan("t")));
    }

    #[test]
    fn structural_hash_union() {
        let plan = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let h = structural_hash(&plan);
        assert_ne!(h, structural_hash(&RelExpr::scan("a")));
    }

    #[test]
    fn structural_hash_cte() {
        let plan = RelExpr::CTE {
            name: "x".into(),
            definition: Box::new(RelExpr::scan("t")),
            body: Box::new(RelExpr::scan("x")),
        };
        let h = structural_hash(&plan);
        assert_ne!(h, 0);
    }

    #[test]
    fn structural_hash_values() {
        let plan = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        };
        let h = structural_hash(&plan);
        assert_ne!(h, 0);
    }

    #[test]
    fn structural_hash_index_scan() {
        let plan = RelExpr::IndexScan {
            table: "t".into(),
            column: "id".into(),
        };
        let h = structural_hash(&plan);
        assert_ne!(h, structural_hash(&RelExpr::scan("t")));
    }

    #[test]
    fn structural_hash_recursive_cte() {
        let plan = RelExpr::RecursiveCTE {
            name: "r".into(),
            base_case: Box::new(RelExpr::scan("t")),
            recursive_case: Box::new(RelExpr::scan("r")),
            body: Box::new(RelExpr::scan("r")),
            cycle_detection: None,
        };
        let h = structural_hash(&plan);
        assert_ne!(h, 0);
    }

    #[test]
    fn structural_hash_window() {
        let plan = RelExpr::Window {
            functions: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        let h = structural_hash(&plan);
        assert_ne!(h, structural_hash(&RelExpr::scan("t")));
    }

    #[test]
    fn memo_table_overwrite() {
        let mut memo = MemoTable::new();
        let e1 = RelExpr::scan("a");
        let e2 = RelExpr::scan("b");
        memo.insert(42, e1);
        memo.insert(42, e2.clone());
        assert_eq!(memo.get(42), Some(&e2));
        assert_eq!(memo.len(), 1);
    }

    #[test]
    fn memo_table_get_missing() {
        let memo = MemoTable::new();
        assert!(memo.get(999).is_none());
    }
}
