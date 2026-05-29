//! Partition pruning (RFC 0019 MVP).
//!
//! Given partition metadata for a table and a filter predicate,
//! determines which partitions can be eliminated at planning
//! time because the filter provably excludes every row they
//! could contain.
//!
//! # Status
//!
//! This is the **pruning** half of RFC 0019. The full RFC also
//! covers partition-wise joins and partition-wise aggregation;
//! those rewrite the plan tree and are a larger, separate piece
//! of work. Pruning is the highest-leverage part — it's what
//! turns "scan all 24 monthly partitions" into "scan the 2 that
//! overlap the WHERE clause" — and it's self-contained: it
//! takes metadata + a predicate and returns a surviving-
//! partition list without touching the plan structure.
//!
//! # Integration
//!
//! Partition metadata isn't in Ra's `RelExpr` (the algebra is
//! partition-agnostic). The PG extension supplies
//! [`PartitionInfo`] from the catalog (`pg_partitioned_table`,
//! `pg_class.relpartbound`); the CLI supplies it from schema
//! JSON. The pruner is pure: `prune(info, predicate) ->
//! Vec<surviving partition indices>`.

use ra_core::expr::{BinOp, Const, Expr};
use serde::{Deserialize, Serialize};

/// Partition metadata for one partitioned table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionInfo {
    /// Table (or alias) the partitions belong to.
    pub table: String,
    /// Partition-key column. Multi-column keys are a future
    /// extension; the MVP handles single-column keys.
    pub partition_key: String,
    /// The partitions, in catalog order. The index into this
    /// vec is the partition identifier returned by [`prune`].
    pub partitions: Vec<PartitionBounds>,
}

/// Bounds describing which key values a partition holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PartitionBounds {
    /// `RANGE` partition: `[min, max)` (max-exclusive, matching
    /// PG's range-partition semantics). `None` means unbounded
    /// on that side.
    Range {
        min: Option<Const>,
        max: Option<Const>,
    },
    /// `LIST` partition: the set of values it accepts.
    List { values: Vec<Const> },
    /// `HASH` partition: `key mod modulus == remainder`.
    Hash { modulus: u32, remainder: u32 },
}

/// Prune partitions of `info` that the `predicate` excludes.
///
/// Returns the indices of partitions that *might* contain
/// matching rows — i.e. partitions that survive pruning. A
/// partition is pruned only when the filter is provably
/// incompatible with its bounds; when in doubt the partition is
/// kept (conservative: never prune a partition that could hold a
/// matching row).
///
/// Returns all partition indices when the predicate doesn't
/// reference the partition key (nothing can be pruned).
#[must_use]
pub fn prune(info: &PartitionInfo, predicate: &Expr) -> Vec<usize> {
    // Extract the set of simple constraints the predicate puts
    // on the partition key. If there are none, no pruning.
    let constraints = extract_key_constraints(predicate, &info.partition_key);
    if constraints.is_empty() {
        return (0..info.partitions.len()).collect();
    }
    info.partitions
        .iter()
        .enumerate()
        .filter_map(|(idx, bounds)| {
            if constraints
                .iter()
                .all(|c| constraint_allows_partition(c, bounds))
            {
                Some(idx)
            } else {
                None
            }
        })
        .collect()
}

/// A single constraint on the partition key, e.g. `key < 100`.
#[derive(Debug, Clone, PartialEq)]
struct KeyConstraint {
    op: BinOp,
    value: Const,
}

/// Walk `predicate` collecting constraints on `key`. Only the
/// AND-conjunction of comparisons is considered — an OR can't
/// prune (a partition matching either disjunct must be kept).
/// Comparisons handled: `=`, `<`, `<=`, `>`, `>=`.
fn extract_key_constraints(predicate: &Expr, key: &str) -> Vec<KeyConstraint> {
    let mut out = Vec::new();
    walk_constraints(predicate, key, &mut out);
    out
}

fn walk_constraints(predicate: &Expr, key: &str, out: &mut Vec<KeyConstraint>) {
    match predicate {
        Expr::BinOp { op: BinOp::And, left, right } => {
            walk_constraints(left, key, out);
            walk_constraints(right, key, out);
        }
        Expr::BinOp { op, left, right }
            if matches!(
                op,
                BinOp::Eq | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
            ) =>
        {
            // Accept `key OP const` and the flipped `const OP key`.
            if let (Expr::Column(c), Expr::Const(v)) = (left.as_ref(), right.as_ref()) {
                if c.column.eq_ignore_ascii_case(key) {
                    out.push(KeyConstraint { op: *op, value: v.clone() });
                }
            } else if let (Expr::Const(v), Expr::Column(c)) = (left.as_ref(), right.as_ref()) {
                if c.column.eq_ignore_ascii_case(key) {
                    out.push(KeyConstraint {
                        op: flip_op(*op),
                        value: v.clone(),
                    });
                }
            }
        }
        _ => {}
    }
}

/// Flip a comparison operator for `const OP key` -> `key OP' const`.
fn flip_op(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other, // Eq / Ne are symmetric
    }
}

/// True if `constraint` is compatible with `bounds` — i.e. the
/// partition could contain a row satisfying the constraint.
/// Conservative: returns `true` (keep the partition) whenever
/// compatibility can't be decided (unsupported value type,
/// hash partition, etc.).
fn constraint_allows_partition(constraint: &KeyConstraint, bounds: &PartitionBounds) -> bool {
    match bounds {
        PartitionBounds::Range { min, max } => {
            range_allows(constraint, min.as_ref(), max.as_ref())
        }
        PartitionBounds::List { values } => {
            list_allows(constraint, values)
        }
        // Hash partitioning can't be statically pruned by a
        // comparison (would need to evaluate the hash). Keep.
        PartitionBounds::Hash { .. } => true,
    }
}

/// Range-partition compatibility. Range is `[min, max)`.
fn range_allows(c: &KeyConstraint, min: Option<&Const>, max: Option<&Const>) -> bool {
    let Some(v) = const_as_f64(&c.value) else {
        return true; // unsupported type → keep
    };
    let min_f = min.and_then(const_as_f64);
    let max_f = max.and_then(const_as_f64);
    match c.op {
        // key = v : v must be in [min, max)
        BinOp::Eq => {
            min_f.is_none_or(|m| v >= m) && max_f.is_none_or(|m| v < m)
        }
        // key < v : partition overlaps if min < v
        BinOp::Lt => min_f.is_none_or(|m| m < v),
        // key <= v : partition overlaps if min <= v
        BinOp::Le => min_f.is_none_or(|m| m <= v),
        // key > v : partition overlaps if max-1 > v, i.e. max > v
        // (max-exclusive, so a value > v exists when max > v+epsilon;
        // conservatively use max > v)
        BinOp::Gt => max_f.is_none_or(|m| m > v),
        // key >= v : partition overlaps if max > v
        BinOp::Ge => max_f.is_none_or(|m| m > v),
        _ => true,
    }
}

/// List-partition compatibility. For `key = v`, the partition is
/// allowed iff `v` is in the list. Inequalities keep the
/// partition if any listed value satisfies them.
fn list_allows(c: &KeyConstraint, values: &[Const]) -> bool {
    let Some(v) = const_as_f64(&c.value) else {
        return true;
    };
    match c.op {
        BinOp::Eq => values
            .iter()
            .filter_map(const_as_f64)
            .any(|lv| (lv - v).abs() < f64::EPSILON),
        BinOp::Lt => values.iter().filter_map(const_as_f64).any(|lv| lv < v),
        BinOp::Le => values.iter().filter_map(const_as_f64).any(|lv| lv <= v),
        BinOp::Gt => values.iter().filter_map(const_as_f64).any(|lv| lv > v),
        BinOp::Ge => values.iter().filter_map(const_as_f64).any(|lv| lv >= v),
        _ => true,
    }
}

/// Coerce a `Const` to f64 for numeric comparison. Returns
/// `None` for non-numeric values (string/bool/null), which the
/// callers treat as "can't decide → keep partition".
fn const_as_f64(c: &Const) -> Option<f64> {
    match c {
        Const::Int(i) => Some(*i as f64),
        Const::Float(f) => Some(*f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min: Option<i64>, max: Option<i64>) -> PartitionBounds {
        PartitionBounds::Range {
            min: min.map(Const::Int),
            max: max.map(Const::Int),
        }
    }

    /// Monthly-style range partitions: [0,100), [100,200), [200,300).
    fn three_range_partitions() -> PartitionInfo {
        PartitionInfo {
            table: "events".into(),
            partition_key: "ts".into(),
            partitions: vec![
                range(Some(0), Some(100)),
                range(Some(100), Some(200)),
                range(Some(200), Some(300)),
            ],
        }
    }

    fn col_cmp(op: BinOp, key: &str, v: i64) -> Expr {
        Expr::BinOp {
            op,
            left: Box::new(Expr::Column(ra_core::expr::ColumnRef::new(key))),
            right: Box::new(Expr::Const(Const::Int(v))),
        }
    }

    #[test]
    fn no_key_predicate_keeps_all() {
        let info = three_range_partitions();
        let pred = col_cmp(BinOp::Eq, "other_col", 50);
        assert_eq!(prune(&info, &pred), vec![0, 1, 2]);
    }

    #[test]
    fn equality_prunes_to_single_partition() {
        let info = three_range_partitions();
        // ts = 150 → only partition [100,200)
        let pred = col_cmp(BinOp::Eq, "ts", 150);
        assert_eq!(prune(&info, &pred), vec![1]);
    }

    #[test]
    fn greater_than_prunes_lower_partitions() {
        let info = three_range_partitions();
        // ts > 150 → partitions [100,200) and [200,300)
        let pred = col_cmp(BinOp::Gt, "ts", 150);
        assert_eq!(prune(&info, &pred), vec![1, 2]);
    }

    #[test]
    fn less_than_prunes_upper_partitions() {
        let info = three_range_partitions();
        // ts < 150 → partitions [0,100) and [100,200)
        let pred = col_cmp(BinOp::Lt, "ts", 150);
        assert_eq!(prune(&info, &pred), vec![0, 1]);
    }

    #[test]
    fn range_band_via_and() {
        let info = three_range_partitions();
        // ts >= 100 AND ts < 200 → only partition [100,200)
        let pred = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(col_cmp(BinOp::Ge, "ts", 100)),
            right: Box::new(col_cmp(BinOp::Lt, "ts", 200)),
        };
        assert_eq!(prune(&info, &pred), vec![1]);
    }

    #[test]
    fn flipped_operands_handled() {
        let info = three_range_partitions();
        // 150 < ts  ==  ts > 150 → partitions 1, 2
        let pred = Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(Expr::Const(Const::Int(150))),
            right: Box::new(Expr::Column(ra_core::expr::ColumnRef::new("ts"))),
        };
        assert_eq!(prune(&info, &pred), vec![1, 2]);
    }

    #[test]
    fn list_partition_equality() {
        let info = PartitionInfo {
            table: "t".into(),
            partition_key: "region".into(),
            partitions: vec![
                PartitionBounds::List {
                    values: vec![Const::Int(1), Const::Int(2)],
                },
                PartitionBounds::List {
                    values: vec![Const::Int(3), Const::Int(4)],
                },
            ],
        };
        // region = 3 → only second partition
        let pred = col_cmp(BinOp::Eq, "region", 3);
        assert_eq!(prune(&info, &pred), vec![1]);
    }

    #[test]
    fn hash_partition_never_pruned() {
        let info = PartitionInfo {
            table: "t".into(),
            partition_key: "id".into(),
            partitions: vec![
                PartitionBounds::Hash { modulus: 4, remainder: 0 },
                PartitionBounds::Hash { modulus: 4, remainder: 1 },
            ],
        };
        let pred = col_cmp(BinOp::Eq, "id", 7);
        // Can't statically prune hash partitions by comparison.
        assert_eq!(prune(&info, &pred), vec![0, 1]);
    }

    #[test]
    fn unbounded_partitions_handled() {
        let info = PartitionInfo {
            table: "t".into(),
            partition_key: "k".into(),
            partitions: vec![
                range(None, Some(0)),    // (-inf, 0)
                range(Some(0), None),    // [0, +inf)
            ],
        };
        // k = -5 → only the first (unbounded-below) partition
        let pred = col_cmp(BinOp::Eq, "k", -5);
        assert_eq!(prune(&info, &pred), vec![0]);
    }

    #[test]
    fn string_key_keeps_all_conservatively() {
        let info = three_range_partitions();
        // Non-numeric comparison value → can't decide → keep all.
        let pred = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ra_core::expr::ColumnRef::new("ts"))),
            right: Box::new(Expr::Const(Const::String("x".into()))),
        };
        assert_eq!(prune(&info, &pred), vec![0, 1, 2]);
    }
}
