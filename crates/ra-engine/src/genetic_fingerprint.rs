//! Genetic query fingerprinting for plan reuse (RFC 0060).
//!
//! Generates compact "genetic" fingerprints of queries to detect
//! structural similarity. Two queries that differ only in literal
//! constants (parameter variations) produce identical fingerprints,
//! enabling aggressive plan cache hits.
//!
//! A fingerprint captures three orthogonal dimensions:
//! - **Join graph hash**: table set and join topology
//! - **Predicate pattern**: operator structure without literal values
//! - **Aggregation signature**: GROUP BY shape and aggregate functions
//!
//! Similarity scoring between fingerprints uses weighted Hamming-style
//! comparison across these dimensions, allowing fuzzy matching for
//! queries that share most but not all structure.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use ra_core::algebra::{AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{BinOp, Expr, UnaryOp};

/// Compact structural fingerprint of a query.
///
/// Two queries that differ only in literal values produce identical
/// fingerprints. The fingerprint is cheap to compute (<1ms for
/// typical queries) and cheap to compare (three integer comparisons
/// for exact match, a handful of bit ops for similarity).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryFingerprint {
    /// Hash of the join graph: tables involved and join topology.
    pub join_graph_hash: u64,
    /// Hash of the predicate pattern: operator types and column
    /// references, ignoring literal values.
    pub predicate_hash: u64,
    /// Hash of the aggregation signature: GROUP BY column count,
    /// aggregate functions used, HAVING presence.
    pub aggregation_hash: u64,
    /// Number of tables in the query (for quick pre-filtering).
    pub table_count: u16,
    /// Number of join operations.
    pub join_count: u16,
    /// Whether the query uses aggregation.
    pub has_aggregation: bool,
    /// Whether the query uses DISTINCT.
    pub has_distinct: bool,
    /// Whether the query has a LIMIT clause.
    pub has_limit: bool,
    /// Whether the query has a sort (ORDER BY).
    pub has_sort: bool,
}

impl QueryFingerprint {
    /// Compute a fingerprint from a `RelExpr` tree.
    #[must_use]
    pub fn from_rel_expr(expr: &RelExpr) -> Self {
        let mut collector = FingerprintCollector::default();
        collector.visit(expr);
        collector.finish()
    }

    /// Compute similarity score between two fingerprints.
    ///
    /// Returns a value in `[0.0, 1.0]` where 1.0 means identical
    /// fingerprints and 0.0 means completely different.
    ///
    /// The score is a weighted combination of:
    /// - Join graph match (40% weight)
    /// - Predicate pattern match (30% weight)
    /// - Aggregation signature match (20% weight)
    /// - Structural flags match (10% weight)
    #[must_use]
    pub fn similarity(&self, other: &Self) -> f64 {
        let join_match =
            if self.join_graph_hash == other.join_graph_hash {
                1.0
            } else if self.table_count == other.table_count
                && self.join_count == other.join_count
            {
                // Same shape but different tables: partial match
                0.3
            } else {
                0.0
            };

        let pred_match =
            if self.predicate_hash == other.predicate_hash {
                1.0
            } else {
                0.0
            };

        let agg_match =
            if self.aggregation_hash == other.aggregation_hash {
                1.0
            } else if self.has_aggregation == other.has_aggregation {
                0.3
            } else {
                0.0
            };

        let mut flag_matches: u32 = 0;
        let flag_total: u32 = 4;
        if self.has_aggregation == other.has_aggregation {
            flag_matches += 1;
        }
        if self.has_distinct == other.has_distinct {
            flag_matches += 1;
        }
        if self.has_limit == other.has_limit {
            flag_matches += 1;
        }
        if self.has_sort == other.has_sort {
            flag_matches += 1;
        }
        let flag_score = f64::from(flag_matches)
            / f64::from(flag_total);

        0.4 * join_match
            + 0.3 * pred_match
            + 0.2 * agg_match
            + 0.1 * flag_score
    }

    /// Whether two fingerprints are exact matches (identical queries
    /// modulo literal values).
    #[must_use]
    pub fn is_exact_match(&self, other: &Self) -> bool {
        self.join_graph_hash == other.join_graph_hash
            && self.predicate_hash == other.predicate_hash
            && self.aggregation_hash == other.aggregation_hash
    }
}

/// Walks a `RelExpr` tree collecting fingerprint components.
#[derive(Default)]
struct FingerprintCollector {
    /// Sorted table names for deterministic hashing.
    tables: BTreeSet<String>,
    /// Join types encountered, in traversal order.
    join_types: Vec<JoinType>,
    /// Predicate operator shapes (BinOp discriminants, column refs).
    predicate_ops: Vec<u8>,
    /// Aggregate functions used.
    agg_functions: Vec<AggregateFunction>,
    /// Number of GROUP BY expressions.
    group_by_count: u16,
    /// Structural flags.
    has_distinct: bool,
    has_limit: bool,
    has_sort: bool,
}

impl FingerprintCollector {
    fn visit(&mut self, expr: &RelExpr) {
        match expr {
            RelExpr::Scan { table, .. } => {
                self.tables.insert(table.clone());
            }
            RelExpr::Filter { predicate, input } => {
                self.visit_expr(predicate);
                self.visit(input);
            }
            RelExpr::Project { input, .. } => {
                self.visit(input);
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => {
                self.join_types.push(*join_type);
                self.visit_expr(condition);
                self.visit(left);
                self.visit(right);
            }
            RelExpr::Aggregate {
                group_by,
                aggregates,
                input,
            } => {
                {
                    self.group_by_count = group_by.len() as u16;
                }
                for agg in aggregates {
                    self.agg_functions.push(agg.function);
                }
                self.visit(input);
            }
            RelExpr::Sort { input, .. } => {
                self.has_sort = true;
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
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }
            RelExpr::CTE {
                definition, body, ..
            } => {
                self.visit(definition);
                self.visit(body);
            }
            RelExpr::Window { input, .. } => {
                self.visit(input);
            }
            RelExpr::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => {
                self.visit(base_case);
                self.visit(recursive_case);
                self.visit(body);
            }
            RelExpr::Unnest { input, .. } => {
                if let Some(inp) = input {
                    self.visit(inp);
                }
            }
            RelExpr::TableFunction { input, .. } => {
                if let Some(inp) = input {
                    self.visit(inp);
                }
            }
            RelExpr::IncrementalSort { input, .. } => {
                self.has_sort = true;
                self.visit(input);
            }
            RelExpr::IndexScan { table, .. } => {
                self.tables.insert(table.clone());
            }
            RelExpr::IndexOnlyScan { table, .. } => {
                self.tables.insert(table.clone());
            }
            RelExpr::BitmapIndexScan {
                table, predicate, ..
            } => {
                self.tables.insert(table.clone());
                self.visit_expr(predicate);
            }
            RelExpr::BitmapHeapScan {
                table,
                bitmap,
                recheck_cond,
            } => {
                self.tables.insert(table.clone());
                self.visit(bitmap);
                if let Some(cond) = recheck_cond {
                    self.visit_expr(cond);
                }
            }
            RelExpr::BitmapAnd { inputs }
            | RelExpr::BitmapOr { inputs } => {
                for inp in inputs {
                    self.visit(inp);
                }
            }
            RelExpr::ParallelScan { table, .. } => {
                self.tables.insert(table.clone());
            }
            RelExpr::ParallelHashJoin {
                join_type,
                condition,
                left,
                right,
                ..
            } => {
                self.join_types.push(*join_type);
                self.visit_expr(condition);
                self.visit(left);
                self.visit(right);
            }
            RelExpr::ParallelAggregate {
                group_by,
                aggregates,
                input,
                ..
            } => {
                {
                    self.group_by_count = group_by.len() as u16;
                }
                for agg in aggregates {
                    self.agg_functions.push(agg.function);
                }
                self.visit(input);
            }
            RelExpr::Gather { input, .. } => {
                self.visit(input);
            }
            RelExpr::Values { .. }
            | RelExpr::MultiUnnest { .. }
            | RelExpr::RowPattern { .. }
            | RelExpr::MvScan { .. } => {}
            RelExpr::TopK { input, .. } => {
                self.visit(input);
            }
            RelExpr::VectorFilter { input, .. } => {
                self.visit(input);
            }
        }
    }

    /// Visit an expression, recording operator shape but ignoring
    /// literal values.
    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Column(col_ref) => {
                // Hash the column name structure, not the value
                self.predicate_ops.push(0x01);
                if let Some(table) = &col_ref.table {
                    for b in table.as_bytes() {
                        self.predicate_ops.push(*b);
                    }
                }
                self.predicate_ops.push(0x00); // separator
                for b in col_ref.column.as_bytes() {
                    self.predicate_ops.push(*b);
                }
                self.predicate_ops.push(0x00); // separator
            }
            Expr::Const(_) => {
                // Record that there IS a constant, but NOT its value.
                // This is what makes parameterized queries match.
                self.predicate_ops.push(0x02);
            }
            Expr::BinOp { op, left, right } => {
                self.predicate_ops.push(0x03);
                self.predicate_ops.push(binop_discriminant(op));
                self.visit_expr(left);
                self.visit_expr(right);
            }
            Expr::UnaryOp { op, operand } => {
                self.predicate_ops.push(0x04);
                self.predicate_ops
                    .push(unaryop_discriminant(op));
                self.visit_expr(operand);
            }
            Expr::Function { name, args } => {
                self.predicate_ops.push(0x05);
                for b in name.as_bytes() {
                    self.predicate_ops.push(*b);
                }
                self.predicate_ops.push(0x00);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            Expr::Case {
                operand,
                when_clauses,
                else_result,
            } => {
                self.predicate_ops.push(0x06);
                if let Some(op) = operand {
                    self.visit_expr(op);
                }
                for (cond, result) in when_clauses {
                    self.visit_expr(cond);
                    self.visit_expr(result);
                }
                if let Some(el) = else_result {
                    self.visit_expr(el);
                }
            }
            Expr::Cast { expr, target_type } => {
                self.predicate_ops.push(0x07);
                for b in target_type.as_bytes() {
                    self.predicate_ops.push(*b);
                }
                self.predicate_ops.push(0x00);
                self.visit_expr(expr);
            }
            Expr::Array(items) => {
                self.predicate_ops.push(0x08);
                for item in items {
                    self.visit_expr(item);
                }
            }
            Expr::ArrayIndex(arr, idx) => {
                self.predicate_ops.push(0x09);
                self.visit_expr(arr);
                self.visit_expr(idx);
            }
            Expr::ArraySlice {
                array, start, end, ..
            } => {
                self.predicate_ops.push(0x0B);
                self.visit_expr(array);
                if let Some(s) = start {
                    self.visit_expr(s);
                }
                if let Some(e) = end {
                    self.visit_expr(e);
                }
            }
            Expr::FieldAccess { expr, field_name } => {
                self.predicate_ops.push(0x0C);
                for b in field_name.as_bytes() {
                    self.predicate_ops.push(*b);
                }
                self.predicate_ops.push(0x00);
                self.visit_expr(expr);
            }
            Expr::SubQuery { test_expr, .. } => {
                self.predicate_ops.push(0x0D);
                if let Some(test) = test_expr {
                    self.visit_expr(test);
                }
            }
            // Pattern-related expressions: record tag only
            Expr::PatternPrev(..)
            | Expr::PatternNext(..)
            | Expr::PatternFirst(..)
            | Expr::PatternLast(..)
            | Expr::PatternClassifier
            | Expr::PatternMatchNumber => {
                self.predicate_ops.push(0x0A);
            }
            // Vector and FTS expressions: record tag only
            Expr::FullTextMatch { .. } => {
                self.predicate_ops.push(0x0B);
            }
            Expr::VectorDistance { .. } => {
                self.predicate_ops.push(0x0C);
            }
        }
    }

    fn finish(self) -> QueryFingerprint {
        let table_count = self.tables.len() as u16;
        let join_count = self.join_types.len() as u16;
        let has_aggregation = !self.agg_functions.is_empty()
            || self.group_by_count > 0;

        QueryFingerprint {
            join_graph_hash: self.compute_join_hash(),
            predicate_hash: self.compute_predicate_hash(),
            aggregation_hash: self.compute_aggregation_hash(),
            table_count,
            join_count,
            has_aggregation,
            has_distinct: self.has_distinct,
            has_limit: self.has_limit,
            has_sort: self.has_sort,
        }
    }

    fn compute_join_hash(&self) -> u64 {
        let mut hasher = FnvHasher::new();
        // Sorted tables for deterministic order
        for table in &self.tables {
            table.hash(&mut hasher);
        }
        // Join types in traversal order
        for jt in &self.join_types {
            std::mem::discriminant(jt).hash(&mut hasher);
        }
        hasher.finish()
    }

    fn compute_predicate_hash(&self) -> u64 {
        let mut hasher = FnvHasher::new();
        self.predicate_ops.hash(&mut hasher);
        hasher.finish()
    }

    fn compute_aggregation_hash(&self) -> u64 {
        let mut hasher = FnvHasher::new();
        self.group_by_count.hash(&mut hasher);
        for func in &self.agg_functions {
            std::mem::discriminant(func).hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// Minimal FNV-1a 64-bit hasher for deterministic, fast hashing.
struct FnvHasher {
    state: u64,
}

impl FnvHasher {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;

    fn new() -> Self {
        Self {
            state: Self::OFFSET_BASIS,
        }
    }
}

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.state
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state ^= u64::from(byte);
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
    }
}

fn binop_discriminant(op: &BinOp) -> u8 {
    match op {
        BinOp::Eq => 0,
        BinOp::Ne => 1,
        BinOp::Lt => 2,
        BinOp::Le => 3,
        BinOp::Gt => 4,
        BinOp::Ge => 5,
        BinOp::And => 6,
        BinOp::Or => 7,
        BinOp::Add => 8,
        BinOp::Sub => 9,
        BinOp::Mul => 10,
        BinOp::Div => 11,
        BinOp::Mod => 12,
        BinOp::Concat => 13,
        BinOp::JsonAccess => 14,
    }
}

fn unaryop_discriminant(op: &UnaryOp) -> u8 {
    match op {
        UnaryOp::Not => 0,
        UnaryOp::Neg => 1,
        UnaryOp::IsNull => 2,
        UnaryOp::IsNotNull => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{AggregateExpr, JoinType};
    use ra_core::expr::{ColumnRef, Const};

    // ── Fingerprint stability ────────────────────────────────────

    #[test]
    fn same_query_same_fingerprint() {
        let q = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let fp1 = QueryFingerprint::from_rel_expr(&q);
        let fp2 = QueryFingerprint::from_rel_expr(&q);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn parameter_variation_same_fingerprint() {
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let q2 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(65))),
        });
        let fp1 = QueryFingerprint::from_rel_expr(&q1);
        let fp2 = QueryFingerprint::from_rel_expr(&q2);
        assert!(fp1.is_exact_match(&fp2));
        assert!((fp1.similarity(&fp2) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn string_parameter_variation_same_fingerprint() {
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("name"))),
            right: Box::new(Expr::Const(Const::String(
                "Alice".to_owned(),
            ))),
        });
        let q2 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("name"))),
            right: Box::new(Expr::Const(Const::String(
                "Bob".to_owned(),
            ))),
        });
        assert!(QueryFingerprint::from_rel_expr(&q1)
            .is_exact_match(&QueryFingerprint::from_rel_expr(&q2)));
    }

    // ── Different queries produce different fingerprints ──────────

    #[test]
    fn different_tables_different_fingerprint() {
        let q1 = RelExpr::scan("users");
        let q2 = RelExpr::scan("orders");
        let fp1 = QueryFingerprint::from_rel_expr(&q1);
        let fp2 = QueryFingerprint::from_rel_expr(&q2);
        assert!(!fp1.is_exact_match(&fp2));
    }

    #[test]
    fn different_operators_different_fingerprint() {
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let q2 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let fp1 = QueryFingerprint::from_rel_expr(&q1);
        let fp2 = QueryFingerprint::from_rel_expr(&q2);
        assert!(!fp1.is_exact_match(&fp2));
    }

    #[test]
    fn different_columns_different_fingerprint() {
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let q2 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("salary"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let fp1 = QueryFingerprint::from_rel_expr(&q1);
        let fp2 = QueryFingerprint::from_rel_expr(&q2);
        assert!(!fp1.is_exact_match(&fp2));
    }

    // ── Similarity scoring ───────────────────────────────────────

    #[test]
    fn identical_queries_similarity_1() {
        let q = RelExpr::scan("users");
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert!((fp.similarity(&fp) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn completely_different_queries_low_similarity() {
        let q1 = RelExpr::scan("users");
        let q2 = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("dept"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(
                        ColumnRef::qualified("orders", "uid"),
                    )),
                    right: Box::new(Expr::Column(
                        ColumnRef::qualified("items", "oid"),
                    )),
                },
                left: Box::new(RelExpr::scan("orders")),
                right: Box::new(RelExpr::scan("items")),
            }),
        };
        let fp1 = QueryFingerprint::from_rel_expr(&q1);
        let fp2 = QueryFingerprint::from_rel_expr(&q2);
        assert!(fp1.similarity(&fp2) < 0.5);
    }

    // ── Join graph hashing ───────────────────────────────────────

    #[test]
    fn join_counted_correctly() {
        let q = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified(
                    "u", "id",
                ))),
                right: Box::new(Expr::Column(
                    ColumnRef::qualified("o", "user_id"),
                )),
            },
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert_eq!(fp.table_count, 2);
        assert_eq!(fp.join_count, 1);
    }

    #[test]
    fn three_way_join() {
        let inner = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        };
        let q = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("c"))),
                right: Box::new(Expr::Column(ColumnRef::new("d"))),
            },
            left: Box::new(inner),
            right: Box::new(RelExpr::scan("t3")),
        };
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert_eq!(fp.table_count, 3);
        assert_eq!(fp.join_count, 2);
    }

    // ── Aggregation signature ────────────────────────────────────

    #[test]
    fn aggregation_detected() {
        let q = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("dept"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("salary"))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("employees")),
        };
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert!(fp.has_aggregation);
    }

    #[test]
    fn same_agg_different_values_match() {
        let make_query = |threshold: i64| {
            RelExpr::Aggregate {
                group_by: vec![Expr::Column(ColumnRef::new(
                    "dept",
                ))],
                aggregates: vec![AggregateExpr {
                    function: AggregateFunction::Count,
                    arg: None,
                    distinct: false,
                    alias: None,
                }],
                input: Box::new(
                    RelExpr::scan("employees").filter(Expr::BinOp {
                        op: BinOp::Gt,
                        left: Box::new(Expr::Column(
                            ColumnRef::new("salary"),
                        )),
                        right: Box::new(Expr::Const(Const::Int(
                            threshold,
                        ))),
                    }),
                ),
            }
        };
        let fp1 =
            QueryFingerprint::from_rel_expr(&make_query(50000));
        let fp2 =
            QueryFingerprint::from_rel_expr(&make_query(80000));
        assert!(fp1.is_exact_match(&fp2));
    }

    // ── Structural flags ─────────────────────────────────────────

    #[test]
    fn distinct_detected() {
        let q = RelExpr::Distinct {
            input: Box::new(RelExpr::scan("users")),
        };
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert!(fp.has_distinct);
    }

    #[test]
    fn limit_detected() {
        let q = RelExpr::scan("users").limit(10, 0);
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert!(fp.has_limit);
    }

    #[test]
    fn sort_detected() {
        let q = RelExpr::Sort {
            keys: vec![ra_core::algebra::SortKey {
                expr: Expr::Column(ColumnRef::new("name")),
                direction: ra_core::algebra::SortDirection::Asc,
                nulls: ra_core::algebra::NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("users")),
        };
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert!(fp.has_sort);
    }

    #[test]
    fn scan_only_no_flags() {
        let q = RelExpr::scan("users");
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert!(!fp.has_aggregation);
        assert!(!fp.has_distinct);
        assert!(!fp.has_limit);
        assert!(!fp.has_sort);
        assert_eq!(fp.table_count, 1);
        assert_eq!(fp.join_count, 0);
    }
}
