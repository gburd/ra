//! Self-join elimination and outer-to-inner join conversion.
//!
//! Two key preprocessing transformations that run before join
//! reordering:
//!
//! **Self-join elimination**: removes redundant joins to the same
//! table when uniqueness guarantees make the join a no-op. Common
//! in generated SQL and ORM queries.
//!
//! **Outer-to-inner conversion**: converts LEFT/RIGHT/FULL OUTER
//! JOINs to INNER JOINs when filters on the nullable side make
//! null-extended rows impossible. This is important because inner
//! joins can be freely reordered while outer joins cannot.

use std::collections::HashSet;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr, UnaryOp};
use serde::{Deserialize, Serialize};

/// A known unique constraint on a table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UniqueConstraint {
    /// Table this constraint applies to.
    pub table: String,
    /// Columns forming the unique key (may be composite).
    pub columns: Vec<String>,
}

/// Schema information needed for join transformations.
#[derive(Debug, Clone, Default)]
pub struct SchemaInfo {
    /// Known unique constraints (primary keys, unique indexes).
    unique_constraints: Vec<UniqueConstraint>,
}

impl SchemaInfo {
    /// Create a new empty schema info.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a unique constraint.
    pub fn add_unique(&mut self, table: &str, columns: &[&str]) {
        self.unique_constraints.push(UniqueConstraint {
            table: table.to_string(),
            columns: columns.iter().map(|c| (*c).to_string()).collect(),
        });
    }

    /// Check if columns form a unique key for a table.
    #[must_use]
    pub fn is_unique_key(&self, table: &str, columns: &[&str]) -> bool {
        self.unique_constraints.iter().any(|uc| {
            uc.table == table
                && uc.columns.len() <= columns.len()
                && uc.columns.iter().all(|c| columns.contains(&c.as_str()))
        })
    }
}

/// Result of a self-join detection check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfJoinMatch {
    /// The table being joined to itself.
    pub table: String,
    /// Columns used in the equi-join condition.
    pub join_columns: Vec<(String, String)>,
}

/// Detect whether a join is a self-join (both sides scan the same table).
#[must_use]
pub fn detect_self_join(expr: &RelExpr) -> Option<SelfJoinMatch> {
    let RelExpr::Join {
        condition,
        left,
        right,
        join_type,
        ..
    } = expr
    else {
        return None;
    };

    if *join_type != JoinType::Inner {
        return None;
    }

    let left_table = scan_table_name(left)?;
    let right_table = scan_table_name(right)?;

    if left_table != right_table {
        return None;
    }

    let join_cols = extract_equi_join_columns(condition);
    if join_cols.is_empty() {
        return None;
    }

    Some(SelfJoinMatch {
        table: left_table,
        join_columns: join_cols,
    })
}

/// Check if a detected self-join can be eliminated because the
/// join key is unique (the join is guaranteed to produce at most
/// one match per row, making it a no-op).
#[must_use]
pub fn can_eliminate_self_join(self_join: &SelfJoinMatch, schema: &SchemaInfo) -> bool {
    let left_cols: Vec<&str> = self_join
        .join_columns
        .iter()
        .map(|(l, _)| l.as_str())
        .collect();
    let right_cols: Vec<&str> = self_join
        .join_columns
        .iter()
        .map(|(_, r)| r.as_str())
        .collect();

    schema.is_unique_key(&self_join.table, &left_cols)
        || schema.is_unique_key(&self_join.table, &right_cols)
}

/// Attempt to eliminate a self-join, returning the simplified
/// expression if possible.
///
/// When a self-join on a unique key is detected, the join is
/// replaced with a single scan of the table.
#[must_use]
pub fn try_eliminate_self_join(expr: &RelExpr, schema: &SchemaInfo) -> Option<RelExpr> {
    let self_join = detect_self_join(expr)?;
    if !can_eliminate_self_join(&self_join, schema) {
        return None;
    }

    let RelExpr::Join { left, .. } = expr else {
        return None;
    };

    Some(*left.clone())
}

/// Check if an expression rejects null values (evaluates to
/// FALSE or NULL when its column inputs are NULL).
///
/// An expression is null-rejecting if applying it as a filter
/// would discard any null-extended row from an outer join.
#[must_use]
pub fn is_null_rejecting(expr: &Expr) -> bool {
    match expr {
        // Comparisons with a non-null constant reject nulls
        Expr::BinOp { op, left, right } => match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                has_column_ref(left) || has_column_ref(right)
            }
            BinOp::And => is_null_rejecting(left) || is_null_rejecting(right),
            BinOp::Or => is_null_rejecting(left) && is_null_rejecting(right),
            _ => false,
        },
        // IS NOT NULL explicitly rejects nulls
        Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            ..
        } => true,
        // NOT(expr) rejects nulls if expr would accept nulls
        // (three-valued logic: NOT(NULL) = NULL, which is filtered out)
        Expr::UnaryOp {
            op: UnaryOp::Not,
            operand,
        } => has_column_ref(operand),
        // Functions generally return NULL on NULL input (strict)
        Expr::Function { args, .. } => args.iter().any(has_column_ref),
        _ => false,
    }
}

/// Collect all table qualifiers from column references in an expression.
#[must_use]
pub fn referenced_tables(expr: &Expr) -> HashSet<String> {
    let mut tables = HashSet::new();
    collect_table_refs(expr, &mut tables);
    tables
}

/// Check if a filter expression references columns from a
/// specific table (by qualifier or alias).
#[must_use]
pub fn references_table(expr: &Expr, table: &str) -> bool {
    referenced_tables(expr).contains(table)
}

/// Determine if an outer join can be converted to an inner join
/// based on a filter predicate applied above the join.
///
/// The conversion is valid when:
/// - LEFT OUTER: filter is null-rejecting and references the right
///   (nullable) side
/// - RIGHT OUTER: filter is null-rejecting and references the left
///   (nullable) side
/// - FULL OUTER: filter is null-rejecting and references both sides
///   (converts to INNER), or one side (converts to LEFT/RIGHT)
#[must_use]
#[expect(clippy::implicit_hasher, reason = "always uses default hasher")]
pub fn outer_to_inner_conversion(
    join_type: JoinType,
    filter: &Expr,
    left_tables: &HashSet<String>,
    right_tables: &HashSet<String>,
) -> Option<JoinType> {
    if !is_null_rejecting(filter) {
        return None;
    }

    let filter_tables = referenced_tables(filter);

    let refs_left = filter_tables.iter().any(|t| left_tables.contains(t));
    let refs_right = filter_tables.iter().any(|t| right_tables.contains(t));

    match join_type {
        JoinType::LeftOuter => {
            if refs_right {
                Some(JoinType::Inner)
            } else {
                None
            }
        }
        JoinType::RightOuter => {
            if refs_left {
                Some(JoinType::Inner)
            } else {
                None
            }
        }
        JoinType::FullOuter => {
            if refs_left && refs_right {
                Some(JoinType::Inner)
            } else if refs_right {
                Some(JoinType::LeftOuter)
            } else if refs_left {
                Some(JoinType::RightOuter)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Apply outer-to-inner conversion on a Filter(Join(...)) pattern.
///
/// If the expression is a filter over an outer join and the filter
/// rejects nulls on the outer side, convert the join type.
#[must_use]
pub fn try_convert_outer_to_inner(expr: &RelExpr) -> Option<RelExpr> {
    let RelExpr::Filter { predicate, input } = expr else {
        return None;
    };

    let RelExpr::Join {
        join_type,
        condition,
        left,
        right,
    } = input.as_ref()
    else {
        return None;
    };

    if *join_type != JoinType::LeftOuter
        && *join_type != JoinType::RightOuter
        && *join_type != JoinType::FullOuter
    {
        return None;
    }

    let left_tables = collect_scan_tables(left);
    let right_tables = collect_scan_tables(right);

    let new_join_type =
        outer_to_inner_conversion(*join_type, predicate, &left_tables, &right_tables)?;

    Some(RelExpr::Filter {
        predicate: predicate.clone(),
        input: Box::new(RelExpr::Join {
            join_type: new_join_type,
            condition: condition.clone(),
            left: left.clone(),
            right: right.clone(),
        }),
    })
}

/// Apply all join transformations to an expression tree
/// (recursive, bottom-up).
#[must_use]
pub fn apply_join_transformations(expr: &RelExpr, schema: &SchemaInfo) -> RelExpr {
    let transformed = transform_children(expr, schema);

    if let Some(simplified) = try_eliminate_self_join(&transformed, schema) {
        return simplified;
    }

    if let Some(converted) = try_convert_outer_to_inner(&transformed) {
        return converted;
    }

    transformed
}

// -- Internal helpers --

fn scan_table_name(expr: &RelExpr) -> Option<String> {
    match expr {
        RelExpr::Scan { table, .. } => Some(table.clone()),
        _ => None,
    }
}

fn extract_equi_join_columns(expr: &Expr) -> Vec<(String, String)> {
    let mut result = Vec::new();
    extract_equi_join_columns_inner(expr, &mut result);
    result
}

fn extract_equi_join_columns_inner(expr: &Expr, out: &mut Vec<(String, String)>) {
    match expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left,
            right,
        } => {
            if let (Expr::Column(left_col), Expr::Column(right_col)) =
                (left.as_ref(), right.as_ref())
            {
                out.push((left_col.column.clone(), right_col.column.clone()));
            }
        }
        Expr::BinOp {
            op: BinOp::And,
            left,
            right,
        } => {
            extract_equi_join_columns_inner(left, out);
            extract_equi_join_columns_inner(right, out);
        }
        _ => {}
    }
}

fn has_column_ref(expr: &Expr) -> bool {
    match expr {
        Expr::Column(_) => true,
        Expr::BinOp { left, right, .. } => has_column_ref(left) || has_column_ref(right),
        Expr::UnaryOp { operand, .. } => has_column_ref(operand),
        Expr::Function { args, .. } => args.iter().any(has_column_ref),
        Expr::Cast { expr, .. } => has_column_ref(expr),
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            operand.as_ref().is_some_and(|o| has_column_ref(o))
                || when_clauses
                    .iter()
                    .any(|(c, r)| has_column_ref(c) || has_column_ref(r))
                || else_result.as_ref().is_some_and(|e| has_column_ref(e))
        }
        _ => false,
    }
}

fn collect_table_refs(expr: &Expr, tables: &mut HashSet<String>) {
    match expr {
        Expr::Column(ColumnRef { table: Some(t), .. }) => {
            tables.insert(t.clone());
        }
        Expr::BinOp { left, right, .. } => {
            collect_table_refs(left, tables);
            collect_table_refs(right, tables);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_table_refs(operand, tables);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_table_refs(arg, tables);
            }
        }
        Expr::Cast { expr, .. } => {
            collect_table_refs(expr, tables);
        }
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            if let Some(op) = operand {
                collect_table_refs(op, tables);
            }
            for (c, r) in when_clauses {
                collect_table_refs(c, tables);
                collect_table_refs(r, tables);
            }
            if let Some(el) = else_result {
                collect_table_refs(el, tables);
            }
        }
        _ => {}
    }
}

fn collect_scan_tables(expr: &RelExpr) -> HashSet<String> {
    let mut tables = HashSet::new();
    collect_scan_tables_inner(expr, &mut tables);
    tables
}

fn collect_scan_tables_inner(expr: &RelExpr, tables: &mut HashSet<String>) {
    match expr {
        RelExpr::Scan { table, alias, .. } => {
            if let Some(a) = alias {
                tables.insert(a.clone());
            }
            tables.insert(table.clone());
        }
        _ => {
            for child in expr.children() {
                collect_scan_tables_inner(child, tables);
            }
        }
    }
}

fn transform_children(expr: &RelExpr, schema: &SchemaInfo) -> RelExpr {
    match expr {
        RelExpr::Filter { predicate, input } => RelExpr::Filter {
            predicate: predicate.clone(),
            input: Box::new(apply_join_transformations(input, schema)),
        },
        RelExpr::Project { columns, input } => RelExpr::Project {
            columns: columns.clone(),
            input: Box::new(apply_join_transformations(input, schema)),
        },
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => RelExpr::Join {
            join_type: *join_type,
            condition: condition.clone(),
            left: Box::new(apply_join_transformations(left, schema)),
            right: Box::new(apply_join_transformations(right, schema)),
        },
        RelExpr::Sort { keys, input } => RelExpr::Sort {
            keys: keys.clone(),
            input: Box::new(apply_join_transformations(input, schema)),
        },
        RelExpr::Limit {
            count,
            offset,
            input,
        } => RelExpr::Limit {
            count: *count,
            offset: *offset,
            input: Box::new(apply_join_transformations(input, schema)),
        },
        RelExpr::Distinct { input } => RelExpr::Distinct {
            input: Box::new(apply_join_transformations(input, schema)),
        },
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => RelExpr::Aggregate {
            group_by: group_by.clone(),
            aggregates: aggregates.clone(),
            input: Box::new(apply_join_transformations(input, schema)),
        },
        RelExpr::Window { functions, input } => RelExpr::Window {
            functions: functions.clone(),
            input: Box::new(apply_join_transformations(input, schema)),
        },
        RelExpr::Union { all, left, right } => RelExpr::Union {
            all: *all,
            left: Box::new(apply_join_transformations(left, schema)),
            right: Box::new(apply_join_transformations(right, schema)),
        },
        RelExpr::Intersect { all, left, right } => RelExpr::Intersect {
            all: *all,
            left: Box::new(apply_join_transformations(left, schema)),
            right: Box::new(apply_join_transformations(right, schema)),
        },
        RelExpr::Except { all, left, right } => RelExpr::Except {
            all: *all,
            left: Box::new(apply_join_transformations(left, schema)),
            right: Box::new(apply_join_transformations(right, schema)),
        },
        // Leaf nodes and complex nodes pass through unchanged.
        other => other.clone(),
    }
}

// -- Rewrite rules for the e-graph --

/// Additional e-graph rewrite rules for join transformations.
///
/// These complement the existing rules in `rewrite.rs` with more
/// comprehensive outer-to-inner conversion patterns beyond just
/// IS NOT NULL.
#[must_use]
#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
pub fn join_transformation_rules(
) -> Vec<egg::Rewrite<crate::egraph::RelLang, crate::analysis::RelAnalysis>> {
    use egg::rewrite;

    vec![
        // LEFT OUTER -> INNER when filter has comparison on right side
        // col > const rejects nulls
        rewrite!("left-outer-to-inner-gt";
            "(filter (gt ?col ?val) (join left-outer ?cond ?left ?right))" =>
            "(filter (gt ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("left-outer-to-inner-lt";
            "(filter (lt ?col ?val) (join left-outer ?cond ?left ?right))" =>
            "(filter (lt ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("left-outer-to-inner-ge";
            "(filter (ge ?col ?val) (join left-outer ?cond ?left ?right))" =>
            "(filter (ge ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("left-outer-to-inner-le";
            "(filter (le ?col ?val) (join left-outer ?cond ?left ?right))" =>
            "(filter (le ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("left-outer-to-inner-eq";
            "(filter (eq ?col ?val) (join left-outer ?cond ?left ?right))" =>
            "(filter (eq ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("left-outer-to-inner-ne";
            "(filter (ne ?col ?val) (join left-outer ?cond ?left ?right))" =>
            "(filter (ne ?col ?val) (join inner ?cond ?left ?right))"
        ),
        // RIGHT OUTER -> INNER with comparison filters
        rewrite!("right-outer-to-inner-gt";
            "(filter (gt ?col ?val) (join right-outer ?cond ?left ?right))" =>
            "(filter (gt ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("right-outer-to-inner-lt";
            "(filter (lt ?col ?val) (join right-outer ?cond ?left ?right))" =>
            "(filter (lt ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("right-outer-to-inner-ge";
            "(filter (ge ?col ?val) (join right-outer ?cond ?left ?right))" =>
            "(filter (ge ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("right-outer-to-inner-le";
            "(filter (le ?col ?val) (join right-outer ?cond ?left ?right))" =>
            "(filter (le ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("right-outer-to-inner-eq";
            "(filter (eq ?col ?val) (join right-outer ?cond ?left ?right))" =>
            "(filter (eq ?col ?val) (join inner ?cond ?left ?right))"
        ),
        rewrite!("right-outer-to-inner-ne";
            "(filter (ne ?col ?val) (join right-outer ?cond ?left ?right))" =>
            "(filter (ne ?col ?val) (join inner ?cond ?left ?right))"
        ),
        // Outer-to-inner through AND conjunction:
        // If either conjunct rejects nulls, the whole conjunction does
        rewrite!("left-outer-to-inner-and";
            "(filter (and ?p1 ?p2) (join left-outer ?cond ?left ?right))" =>
            "(filter (and ?p1 ?p2) (join inner ?cond ?left ?right))"
        ),
        rewrite!("right-outer-to-inner-and";
            "(filter (and ?p1 ?p2) (join right-outer ?cond ?left ?right))" =>
            "(filter (and ?p1 ?p2) (join inner ?cond ?left ?right))"
        ),
        // Self-join elimination: join of same scan on same condition
        // is identity when result is same as left (simplified pattern)
        rewrite!("self-join-same-table";
            "(join inner (eq ?col ?col) (scan ?t) (scan ?t))" =>
            "(scan ?t)"
        ),
    ]
}

#[cfg(test)]
#[expect(clippy::panic, reason = "test assertions")]
mod tests {
    use super::*;
    use ra_core::expr::Const;

    // -- Helper builders --

    fn eq_expr(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef::new(name))
    }

    fn qcol(table: &str, name: &str) -> Expr {
        Expr::Column(ColumnRef::qualified(table, name))
    }

    fn gt_expr(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn and_expr(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::And,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn or_expr(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn is_not_null(expr: Expr) -> Expr {
        Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            operand: Box::new(expr),
        }
    }

    fn is_null(expr: Expr) -> Expr {
        Expr::UnaryOp {
            op: UnaryOp::IsNull,
            operand: Box::new(expr),
        }
    }

    fn int_const(val: i64) -> Expr {
        Expr::Const(Const::Int(val))
    }

    fn make_join(join_type: JoinType, condition: Expr, left: RelExpr, right: RelExpr) -> RelExpr {
        RelExpr::Join {
            join_type,
            condition,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    // -- SchemaInfo tests --

    #[test]
    fn schema_info_new_empty() {
        let s = SchemaInfo::new();
        assert!(!s.is_unique_key("t", &["id"]));
    }

    #[test]
    fn schema_info_add_unique() {
        let mut s = SchemaInfo::new();
        s.add_unique("users", &["id"]);
        assert!(s.is_unique_key("users", &["id"]));
    }

    #[test]
    fn schema_info_composite_unique() {
        let mut s = SchemaInfo::new();
        s.add_unique("order_items", &["order_id", "item_id"]);
        assert!(s.is_unique_key("order_items", &["order_id", "item_id"],));
        // Superset also satisfies the constraint
        assert!(s.is_unique_key("order_items", &["order_id", "item_id", "qty"],));
    }

    #[test]
    fn schema_info_unique_wrong_table() {
        let mut s = SchemaInfo::new();
        s.add_unique("users", &["id"]);
        assert!(!s.is_unique_key("orders", &["id"]));
    }

    #[test]
    fn schema_info_unique_wrong_columns() {
        let mut s = SchemaInfo::new();
        s.add_unique("users", &["id"]);
        assert!(!s.is_unique_key("users", &["name"]));
    }

    #[test]
    fn schema_info_partial_composite_not_unique() {
        let mut s = SchemaInfo::new();
        s.add_unique("t", &["a", "b"]);
        assert!(!s.is_unique_key("t", &["a"]));
    }

    // -- Self-join detection --

    #[test]
    fn detect_self_join_basic() {
        let expr = make_join(
            JoinType::Inner,
            eq_expr(col("id"), col("id")),
            RelExpr::scan("users"),
            RelExpr::scan("users"),
        );
        let result = detect_self_join(&expr);
        assert!(result.is_some());
        let m = result.expect("should detect self-join");
        assert_eq!(m.table, "users");
        assert_eq!(m.join_columns.len(), 1);
    }

    #[test]
    fn detect_self_join_different_tables() {
        let expr = make_join(
            JoinType::Inner,
            eq_expr(col("id"), col("id")),
            RelExpr::scan("users"),
            RelExpr::scan("orders"),
        );
        assert!(detect_self_join(&expr).is_none());
    }

    #[test]
    fn detect_self_join_outer_not_detected() {
        let expr = make_join(
            JoinType::LeftOuter,
            eq_expr(col("id"), col("id")),
            RelExpr::scan("users"),
            RelExpr::scan("users"),
        );
        assert!(detect_self_join(&expr).is_none());
    }

    #[test]
    fn detect_self_join_not_equi() {
        let expr = make_join(
            JoinType::Inner,
            gt_expr(col("id"), col("id")),
            RelExpr::scan("users"),
            RelExpr::scan("users"),
        );
        assert!(detect_self_join(&expr).is_none());
    }

    #[test]
    fn detect_self_join_non_join() {
        let expr = RelExpr::scan("users");
        assert!(detect_self_join(&expr).is_none());
    }

    #[test]
    fn detect_self_join_composite_condition() {
        let condition = and_expr(eq_expr(col("a"), col("a")), eq_expr(col("b"), col("b")));
        let expr = make_join(
            JoinType::Inner,
            condition,
            RelExpr::scan("t"),
            RelExpr::scan("t"),
        );
        let result = detect_self_join(&expr);
        assert!(result.is_some());
        assert_eq!(result.expect("should detect").join_columns.len(), 2,);
    }

    // -- Self-join elimination --

    #[test]
    fn can_eliminate_with_unique_key() {
        let mut schema = SchemaInfo::new();
        schema.add_unique("users", &["id"]);
        let m = SelfJoinMatch {
            table: "users".to_string(),
            join_columns: vec![("id".to_string(), "id".to_string())],
        };
        assert!(can_eliminate_self_join(&m, &schema));
    }

    #[test]
    fn cannot_eliminate_without_unique_key() {
        let schema = SchemaInfo::new();
        let m = SelfJoinMatch {
            table: "users".to_string(),
            join_columns: vec![("id".to_string(), "id".to_string())],
        };
        assert!(!can_eliminate_self_join(&m, &schema));
    }

    #[test]
    fn try_eliminate_self_join_success() {
        let mut schema = SchemaInfo::new();
        schema.add_unique("users", &["id"]);
        let expr = make_join(
            JoinType::Inner,
            eq_expr(col("id"), col("id")),
            RelExpr::scan("users"),
            RelExpr::scan("users"),
        );
        let result = try_eliminate_self_join(&expr, &schema);
        assert!(result.is_some());
        let simplified = result.expect("should eliminate");
        assert!(matches!(simplified, RelExpr::Scan { table, .. } if table == "users"));
    }

    #[test]
    fn try_eliminate_self_join_no_unique() {
        let schema = SchemaInfo::new();
        let expr = make_join(
            JoinType::Inner,
            eq_expr(col("id"), col("id")),
            RelExpr::scan("users"),
            RelExpr::scan("users"),
        );
        assert!(try_eliminate_self_join(&expr, &schema).is_none());
    }

    // -- Null rejection --

    #[test]
    fn null_rejecting_comparison() {
        let expr = gt_expr(qcol("b", "value"), int_const(10));
        assert!(is_null_rejecting(&expr));
    }

    #[test]
    fn null_rejecting_eq() {
        let expr = eq_expr(qcol("b", "id"), int_const(5));
        assert!(is_null_rejecting(&expr));
    }

    #[test]
    fn null_rejecting_is_not_null() {
        let expr = is_not_null(qcol("b", "id"));
        assert!(is_null_rejecting(&expr));
    }

    #[test]
    fn not_null_rejecting_is_null() {
        let expr = is_null(qcol("b", "id"));
        assert!(!is_null_rejecting(&expr));
    }

    #[test]
    fn not_null_rejecting_const_only() {
        let expr = gt_expr(int_const(5), int_const(3));
        assert!(!is_null_rejecting(&expr));
    }

    #[test]
    fn null_rejecting_and_one_side() {
        // AND: one side null-rejecting is enough
        let expr = and_expr(
            gt_expr(qcol("b", "x"), int_const(5)),
            Expr::Const(Const::Bool(true)),
        );
        assert!(is_null_rejecting(&expr));
    }

    #[test]
    fn null_rejecting_or_both_sides() {
        // OR: both sides must reject nulls
        let expr = or_expr(
            gt_expr(qcol("b", "x"), int_const(5)),
            gt_expr(qcol("b", "y"), int_const(3)),
        );
        assert!(is_null_rejecting(&expr));
    }

    #[test]
    fn not_null_rejecting_or_one_const() {
        // OR with one constant-only side: not null-rejecting
        let expr = or_expr(
            gt_expr(qcol("b", "x"), int_const(5)),
            Expr::Const(Const::Bool(true)),
        );
        assert!(!is_null_rejecting(&expr));
    }

    #[test]
    fn null_rejecting_function() {
        let expr = Expr::Function {
            name: "upper".to_string(),
            args: vec![qcol("b", "name")],
        };
        assert!(is_null_rejecting(&expr));
    }

    #[test]
    fn not_null_rejecting_function_no_col() {
        let expr = Expr::Function {
            name: "now".to_string(),
            args: vec![],
        };
        assert!(!is_null_rejecting(&expr));
    }

    // -- referenced_tables --

    #[test]
    fn referenced_tables_qualified() {
        let expr = gt_expr(qcol("b", "value"), int_const(10));
        let tables = referenced_tables(&expr);
        assert!(tables.contains("b"));
        assert_eq!(tables.len(), 1);
    }

    #[test]
    fn referenced_tables_unqualified() {
        let expr = gt_expr(col("value"), int_const(10));
        let tables = referenced_tables(&expr);
        assert!(tables.is_empty());
    }

    #[test]
    fn referenced_tables_multiple() {
        let expr = and_expr(
            gt_expr(qcol("a", "x"), int_const(1)),
            gt_expr(qcol("b", "y"), int_const(2)),
        );
        let tables = referenced_tables(&expr);
        assert!(tables.contains("a"));
        assert!(tables.contains("b"));
        assert_eq!(tables.len(), 2);
    }

    // -- Outer-to-inner conversion --

    #[test]
    fn left_outer_to_inner_right_side_filter() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = gt_expr(qcol("b", "value"), int_const(10));

        let result =
            outer_to_inner_conversion(JoinType::LeftOuter, &filter, &left_tables, &right_tables);
        assert_eq!(result, Some(JoinType::Inner));
    }

    #[test]
    fn left_outer_not_converted_left_side_filter() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = gt_expr(qcol("a", "value"), int_const(10));

        let result =
            outer_to_inner_conversion(JoinType::LeftOuter, &filter, &left_tables, &right_tables);
        assert_eq!(result, None);
    }

    #[test]
    fn right_outer_to_inner_left_side_filter() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = gt_expr(qcol("a", "id"), int_const(0));

        let result =
            outer_to_inner_conversion(JoinType::RightOuter, &filter, &left_tables, &right_tables);
        assert_eq!(result, Some(JoinType::Inner));
    }

    #[test]
    fn right_outer_not_converted_right_side_filter() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = gt_expr(qcol("b", "id"), int_const(0));

        assert!(outer_to_inner_conversion(
            JoinType::RightOuter,
            &filter,
            &left_tables,
            &right_tables,
        )
        .is_none());
    }

    #[test]
    fn full_outer_to_inner_both_sides() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = and_expr(
            gt_expr(qcol("a", "x"), int_const(1)),
            gt_expr(qcol("b", "y"), int_const(2)),
        );

        let result =
            outer_to_inner_conversion(JoinType::FullOuter, &filter, &left_tables, &right_tables);
        assert_eq!(result, Some(JoinType::Inner));
    }

    #[test]
    fn full_outer_to_left_right_side_filter() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = gt_expr(qcol("b", "y"), int_const(2));

        let result =
            outer_to_inner_conversion(JoinType::FullOuter, &filter, &left_tables, &right_tables);
        assert_eq!(result, Some(JoinType::LeftOuter));
    }

    #[test]
    fn full_outer_to_right_left_side_filter() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = gt_expr(qcol("a", "x"), int_const(1));

        let result =
            outer_to_inner_conversion(JoinType::FullOuter, &filter, &left_tables, &right_tables);
        assert_eq!(result, Some(JoinType::RightOuter));
    }

    #[test]
    fn inner_join_not_converted() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = gt_expr(qcol("b", "x"), int_const(1));

        assert!(
            outer_to_inner_conversion(JoinType::Inner, &filter, &left_tables, &right_tables,)
                .is_none()
        );
    }

    #[test]
    fn non_null_rejecting_not_converted() {
        let left_tables: HashSet<String> = ["a".to_string()].into_iter().collect();
        let right_tables: HashSet<String> = ["b".to_string()].into_iter().collect();
        let filter = Expr::Const(Const::Bool(true));

        assert!(outer_to_inner_conversion(
            JoinType::LeftOuter,
            &filter,
            &left_tables,
            &right_tables,
        )
        .is_none());
    }

    // -- try_convert_outer_to_inner --

    #[test]
    fn try_convert_left_outer_to_inner() {
        let join = make_join(
            JoinType::LeftOuter,
            eq_expr(qcol("a", "id"), qcol("b", "id")),
            RelExpr::Scan {
                table: "a".to_string(),
                alias: None,
            },
            RelExpr::Scan {
                table: "b".to_string(),
                alias: None,
            },
        );
        let expr = RelExpr::Filter {
            predicate: gt_expr(qcol("b", "value"), int_const(10)),
            input: Box::new(join),
        };

        let result = try_convert_outer_to_inner(&expr);
        assert!(result.is_some());
        let converted = result.expect("should convert");
        if let RelExpr::Filter { input, .. } = &converted {
            if let RelExpr::Join { join_type, .. } = input.as_ref() {
                assert_eq!(*join_type, JoinType::Inner);
            } else {
                panic!("expected Join inside Filter");
            }
        } else {
            panic!("expected Filter");
        }
    }

    #[test]
    fn try_convert_no_filter() {
        let expr = RelExpr::scan("t");
        assert!(try_convert_outer_to_inner(&expr).is_none());
    }

    #[test]
    fn try_convert_inner_join_no_change() {
        let join = make_join(
            JoinType::Inner,
            eq_expr(qcol("a", "id"), qcol("b", "id")),
            RelExpr::scan("a"),
            RelExpr::scan("b"),
        );
        let expr = RelExpr::Filter {
            predicate: gt_expr(qcol("b", "value"), int_const(10)),
            input: Box::new(join),
        };
        assert!(try_convert_outer_to_inner(&expr).is_none());
    }

    // -- apply_join_transformations --

    #[test]
    fn apply_transforms_eliminates_self_join() {
        let mut schema = SchemaInfo::new();
        schema.add_unique("users", &["id"]);
        let expr = make_join(
            JoinType::Inner,
            eq_expr(col("id"), col("id")),
            RelExpr::scan("users"),
            RelExpr::scan("users"),
        );
        let result = apply_join_transformations(&expr, &schema);
        assert!(matches!(result, RelExpr::Scan { table, .. } if table == "users"));
    }

    #[test]
    fn apply_transforms_converts_outer() {
        let schema = SchemaInfo::new();
        let join = make_join(
            JoinType::LeftOuter,
            eq_expr(qcol("a", "id"), qcol("b", "id")),
            RelExpr::Scan {
                table: "a".to_string(),
                alias: None,
            },
            RelExpr::Scan {
                table: "b".to_string(),
                alias: None,
            },
        );
        let expr = RelExpr::Filter {
            predicate: gt_expr(qcol("b", "value"), int_const(10)),
            input: Box::new(join),
        };

        let result = apply_join_transformations(&expr, &schema);
        if let RelExpr::Filter { input, .. } = &result {
            if let RelExpr::Join { join_type, .. } = input.as_ref() {
                assert_eq!(*join_type, JoinType::Inner);
            } else {
                panic!("expected inner join after conversion");
            }
        } else {
            panic!("expected filter");
        }
    }

    #[test]
    fn apply_transforms_nested() {
        let mut schema = SchemaInfo::new();
        schema.add_unique("t", &["id"]);

        // Outer join wrapping a self-join
        let self_join = make_join(
            JoinType::Inner,
            eq_expr(col("id"), col("id")),
            RelExpr::scan("t"),
            RelExpr::scan("t"),
        );
        let outer = make_join(
            JoinType::LeftOuter,
            eq_expr(qcol("t", "id"), qcol("s", "id")),
            self_join,
            RelExpr::Scan {
                table: "s".to_string(),
                alias: None,
            },
        );
        let expr = RelExpr::Filter {
            predicate: gt_expr(qcol("s", "val"), int_const(0)),
            input: Box::new(outer),
        };

        let result = apply_join_transformations(&expr, &schema);
        // The self-join should be eliminated and outer converted
        if let RelExpr::Filter { input, .. } = &result {
            if let RelExpr::Join {
                join_type, left, ..
            } = input.as_ref()
            {
                assert_eq!(*join_type, JoinType::Inner);
                // Left side should now be a single scan
                assert!(matches!(
                    left.as_ref(),
                    RelExpr::Scan { table, .. } if table == "t"
                ));
            } else {
                panic!("expected join");
            }
        } else {
            panic!("expected filter");
        }
    }

    #[test]
    fn apply_transforms_passthrough_unchanged() {
        let schema = SchemaInfo::new();
        let expr = RelExpr::scan("t").filter(gt_expr(col("x"), int_const(5)));
        let result = apply_join_transformations(&expr, &schema);
        assert_eq!(result, expr);
    }

    // -- E-graph rewrite rules --

    #[test]
    fn join_transformation_rules_count() {
        let rules = join_transformation_rules();
        assert!(
            rules.len() >= 15,
            "expected at least 15 join transformation rules, got {}",
            rules.len(),
        );
    }

    // -- collect_scan_tables --

    #[test]
    fn collect_scan_tables_basic() {
        let expr = RelExpr::scan("users");
        let tables = collect_scan_tables(&expr);
        assert!(tables.contains("users"));
    }

    #[test]
    fn collect_scan_tables_with_alias() {
        let expr = RelExpr::Scan {
            table: "users".to_string(),
            alias: Some("u".to_string()),
        };
        let tables = collect_scan_tables(&expr);
        assert!(tables.contains("users"));
        assert!(tables.contains("u"));
    }

    #[test]
    fn collect_scan_tables_nested() {
        let expr = make_join(
            JoinType::Inner,
            Expr::Const(Const::Bool(true)),
            RelExpr::scan("a"),
            RelExpr::scan("b"),
        );
        let tables = collect_scan_tables(&expr);
        assert!(tables.contains("a"));
        assert!(tables.contains("b"));
    }

    // -- has_column_ref --

    #[test]
    fn has_column_ref_column() {
        assert!(has_column_ref(&col("x")));
    }

    #[test]
    fn has_column_ref_const() {
        assert!(!has_column_ref(&int_const(5)));
    }

    #[test]
    fn has_column_ref_nested() {
        let expr = gt_expr(col("x"), int_const(5));
        assert!(has_column_ref(&expr));
    }

    // -- extract_equi_join_columns --

    #[test]
    fn extract_equi_single() {
        let expr = eq_expr(col("a"), col("b"));
        let cols = extract_equi_join_columns(&expr);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0], ("a".to_string(), "b".to_string()));
    }

    #[test]
    fn extract_equi_composite() {
        let expr = and_expr(eq_expr(col("a"), col("b")), eq_expr(col("c"), col("d")));
        let cols = extract_equi_join_columns(&expr);
        assert_eq!(cols.len(), 2);
    }

    #[test]
    fn extract_equi_non_column() {
        let expr = eq_expr(col("a"), int_const(5));
        let cols = extract_equi_join_columns(&expr);
        assert!(cols.is_empty());
    }

    #[test]
    fn extract_equi_no_eq() {
        let expr = gt_expr(col("a"), col("b"));
        let cols = extract_equi_join_columns(&expr);
        assert!(cols.is_empty());
    }
}
