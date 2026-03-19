//! Constraint-based query optimization.
//!
//! Uses database constraints (primary keys, foreign keys, unique,
//! check, not-null) to simplify query plans by removing redundant
//! operations that are guaranteed by the schema.
//!
//! Three main optimizations:
//! - Remove redundant predicates implied by constraints
//! - Eliminate DISTINCT when unique constraints guarantee uniqueness
//! - Remove joins guaranteed by foreign key relationships

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr, UnaryOp};
use ra_metadata::schema::{
    ConstraintInfo, ConstraintKind, SchemaInfo, TableInfo,
};

/// Result of a constraint-based optimization pass.
#[derive(Debug, Clone)]
pub struct ConstraintOptResult {
    /// The (possibly rewritten) expression.
    pub expr: RelExpr,
    /// Optimizations applied (human-readable descriptions).
    pub applied: Vec<String>,
}

/// Apply all constraint-based optimizations to a relational
/// expression, given the schema metadata.
///
/// Returns the optimized expression along with descriptions of
/// what was changed.
#[must_use]
pub fn optimize_with_constraints(
    expr: &RelExpr,
    schema: &SchemaInfo,
) -> ConstraintOptResult {
    let mut applied = Vec::new();
    let mut result = expr.clone();

    result = prune_redundant_predicates(&result, schema, &mut applied);
    result =
        eliminate_distinct_with_unique(&result, schema, &mut applied);
    result =
        eliminate_join_with_fk(&result, schema, &mut applied);

    ConstraintOptResult {
        expr: result,
        applied,
    }
}

/// Remove predicates that are implied by NOT NULL constraints.
///
/// If a column has a NOT NULL constraint, any `IS NOT NULL` check
/// on that column is redundant and can be removed. Similarly,
/// `IS NULL` on a NOT NULL column is always false, so a filter
/// with that predicate produces no rows.
fn prune_redundant_predicates(
    expr: &RelExpr,
    schema: &SchemaInfo,
    applied: &mut Vec<String>,
) -> RelExpr {
    match expr {
        RelExpr::Filter { predicate, input } => {
            let new_input =
                prune_redundant_predicates(input, schema, applied);

            let table_name = find_scan_table(&new_input);
            if let Some(ref tname) = table_name {
                if let Some(table_info) = schema.get_table(tname) {
                    if let Some(rewritten) =
                        try_prune_predicate(
                            predicate,
                            table_info,
                            tname,
                            applied,
                        )
                    {
                        return match rewritten {
                            PrunedPredicate::AlwaysTrue => {
                                new_input
                            }
                            PrunedPredicate::Simplified(p) => {
                                RelExpr::Filter {
                                    predicate: p,
                                    input: Box::new(new_input),
                                }
                            }
                        };
                    }
                }
            }

            RelExpr::Filter {
                predicate: predicate.clone(),
                input: Box::new(new_input),
            }
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => RelExpr::Join {
            join_type: *join_type,
            condition: condition.clone(),
            left: Box::new(prune_redundant_predicates(
                left, schema, applied,
            )),
            right: Box::new(prune_redundant_predicates(
                right, schema, applied,
            )),
        },
        RelExpr::Project { columns, input } => RelExpr::Project {
            columns: columns.clone(),
            input: Box::new(prune_redundant_predicates(
                input, schema, applied,
            )),
        },
        RelExpr::Distinct { input } => RelExpr::Distinct {
            input: Box::new(prune_redundant_predicates(
                input, schema, applied,
            )),
        },
        RelExpr::Sort { keys, input } => RelExpr::Sort {
            keys: keys.clone(),
            input: Box::new(prune_redundant_predicates(
                input, schema, applied,
            )),
        },
        RelExpr::Limit {
            count,
            offset,
            input,
        } => RelExpr::Limit {
            count: *count,
            offset: *offset,
            input: Box::new(prune_redundant_predicates(
                input, schema, applied,
            )),
        },
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => RelExpr::Aggregate {
            group_by: group_by.clone(),
            aggregates: aggregates.clone(),
            input: Box::new(prune_redundant_predicates(
                input, schema, applied,
            )),
        },
        other => other.clone(),
    }
}

enum PrunedPredicate {
    AlwaysTrue,
    Simplified(Expr),
}

fn try_prune_predicate(
    predicate: &Expr,
    table_info: &TableInfo,
    table_name: &str,
    applied: &mut Vec<String>,
) -> Option<PrunedPredicate> {
    match predicate {
        // IS NOT NULL on a NOT NULL column => always true
        Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            operand,
        } => {
            if let Expr::Column(col_ref) = operand.as_ref() {
                let col_name = &col_ref.column;
                if is_not_null_column(table_info, col_name) {
                    applied.push(format!(
                        "Removed redundant IS NOT NULL on \
                         {table_name}.{col_name} \
                         (NOT NULL constraint)"
                    ));
                    return Some(PrunedPredicate::AlwaysTrue);
                }
            }
            None
        }
        // AND of predicates: prune each side
        Expr::BinOp {
            op: BinOp::And,
            left,
            right,
        } => {
            let left_pruned = try_prune_predicate(
                left,
                table_info,
                table_name,
                applied,
            );
            let right_pruned = try_prune_predicate(
                right,
                table_info,
                table_name,
                applied,
            );

            match (left_pruned, right_pruned) {
                (
                    Some(PrunedPredicate::AlwaysTrue),
                    Some(PrunedPredicate::AlwaysTrue),
                ) => Some(PrunedPredicate::AlwaysTrue),
                (Some(PrunedPredicate::AlwaysTrue), None) => {
                    Some(PrunedPredicate::Simplified(
                        *right.clone(),
                    ))
                }
                (None, Some(PrunedPredicate::AlwaysTrue)) => {
                    Some(PrunedPredicate::Simplified(
                        *left.clone(),
                    ))
                }
                (
                    Some(PrunedPredicate::AlwaysTrue),
                    Some(PrunedPredicate::Simplified(r)),
                ) => Some(PrunedPredicate::Simplified(r)),
                (
                    Some(PrunedPredicate::Simplified(l)),
                    Some(PrunedPredicate::AlwaysTrue),
                ) => Some(PrunedPredicate::Simplified(l)),
                (
                    Some(PrunedPredicate::Simplified(l)),
                    Some(PrunedPredicate::Simplified(r)),
                ) => {
                    Some(PrunedPredicate::Simplified(Expr::BinOp {
                        op: BinOp::And,
                        left: Box::new(l),
                        right: Box::new(r),
                    }))
                }
                (
                    Some(PrunedPredicate::Simplified(l)),
                    None,
                ) => {
                    Some(PrunedPredicate::Simplified(Expr::BinOp {
                        op: BinOp::And,
                        left: Box::new(l),
                        right: right.clone(),
                    }))
                }
                (
                    None,
                    Some(PrunedPredicate::Simplified(r)),
                ) => {
                    Some(PrunedPredicate::Simplified(Expr::BinOp {
                        op: BinOp::And,
                        left: left.clone(),
                        right: Box::new(r),
                    }))
                }
                (None, None) => None,
            }
        }
        _ => None,
    }
}

fn is_not_null_column(table_info: &TableInfo, col: &str) -> bool {
    table_info
        .get_column(col)
        .is_some_and(|c| !c.nullable)
}

/// Eliminate DISTINCT when a unique constraint covers the output
/// columns.
///
/// If SELECT DISTINCT is applied over columns that are already
/// guaranteed unique by a primary key or unique constraint, the
/// DISTINCT is unnecessary.
fn eliminate_distinct_with_unique(
    expr: &RelExpr,
    schema: &SchemaInfo,
    applied: &mut Vec<String>,
) -> RelExpr {
    match expr {
        RelExpr::Distinct { input } => {
            let new_input = eliminate_distinct_with_unique(
                input, schema, applied,
            );

            if let Some(table_name) = find_scan_table(&new_input)
            {
                if let Some(table_info) =
                    schema.get_table(&table_name)
                {
                    let output_cols =
                        collect_output_columns(&new_input);

                    if is_covered_by_unique(
                        &output_cols,
                        table_info,
                    ) {
                        applied.push(format!(
                            "Eliminated DISTINCT on \
                             {table_name}: output columns \
                             covered by unique constraint"
                        ));
                        return new_input;
                    }
                }
            }

            RelExpr::Distinct {
                input: Box::new(new_input),
            }
        }
        RelExpr::Filter { predicate, input } => RelExpr::Filter {
            predicate: predicate.clone(),
            input: Box::new(eliminate_distinct_with_unique(
                input, schema, applied,
            )),
        },
        RelExpr::Project { columns, input } => RelExpr::Project {
            columns: columns.clone(),
            input: Box::new(eliminate_distinct_with_unique(
                input, schema, applied,
            )),
        },
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => RelExpr::Join {
            join_type: *join_type,
            condition: condition.clone(),
            left: Box::new(eliminate_distinct_with_unique(
                left, schema, applied,
            )),
            right: Box::new(eliminate_distinct_with_unique(
                right, schema, applied,
            )),
        },
        other => other.clone(),
    }
}

/// Check if the output columns are covered by any unique
/// constraint (the unique constraint columns are a subset of
/// the output columns).
fn is_covered_by_unique(
    output_cols: &[String],
    table_info: &TableInfo,
) -> bool {
    for constraint in &table_info.constraints {
        if constraint.kind != ConstraintKind::PrimaryKey
            && constraint.kind != ConstraintKind::Unique
        {
            continue;
        }

        let all_covered = constraint.columns.iter().all(|c| {
            output_cols.iter().any(|oc| oc == c)
        });

        if all_covered {
            return true;
        }
    }
    false
}

/// Eliminate inner joins that are guaranteed by foreign key
/// relationships when only columns from the FK-referencing side
/// are used.
///
/// If we have `SELECT orders.* FROM orders JOIN users ON
/// orders.user_id = users.id` and there's a FK from
/// `orders.user_id -> users.id`, the join is guaranteed to
/// succeed for every row (assuming FK integrity), so we can
/// remove the join to `users` if no `users` columns are needed.
fn eliminate_join_with_fk(
    expr: &RelExpr,
    schema: &SchemaInfo,
    applied: &mut Vec<String>,
) -> RelExpr {
    match expr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition,
            left,
            right,
        } => {
            let new_left =
                eliminate_join_with_fk(left, schema, applied);
            let new_right =
                eliminate_join_with_fk(right, schema, applied);

            let left_table = find_scan_table(&new_left);
            let right_table = find_scan_table(&new_right);

            if let (Some(ref lt), Some(ref rt)) =
                (&left_table, &right_table)
            {
                if let Some((fk_side, fk_info)) =
                    find_fk_join_match(
                        condition, lt, rt, schema,
                    )
                {
                    let used_tables =
                        collect_used_tables_above(expr);

                    match fk_side {
                        FkSide::LeftReferenceRight => {
                            if !used_tables.contains(
                                &rt.to_string(),
                            ) {
                                applied.push(format!(
                                    "Eliminated join to {rt}: \
                                     FK {}.({}) -> {rt}.({}) \
                                     guarantees match",
                                    lt,
                                    fk_info.columns.join(", "),
                                    fk_info
                                        .referenced_columns
                                        .join(", ")
                                ));
                                return new_left;
                            }
                        }
                        FkSide::RightReferenceLeft => {
                            if !used_tables.contains(
                                &lt.to_string(),
                            ) {
                                applied.push(format!(
                                    "Eliminated join to {lt}: \
                                     FK {}.({}) -> {lt}.({}) \
                                     guarantees match",
                                    rt,
                                    fk_info.columns.join(", "),
                                    fk_info
                                        .referenced_columns
                                        .join(", ")
                                ));
                                return new_right;
                            }
                        }
                    }
                }
            }

            RelExpr::Join {
                join_type: JoinType::Inner,
                condition: condition.clone(),
                left: Box::new(new_left),
                right: Box::new(new_right),
            }
        }
        RelExpr::Filter { predicate, input } => RelExpr::Filter {
            predicate: predicate.clone(),
            input: Box::new(eliminate_join_with_fk(
                input, schema, applied,
            )),
        },
        RelExpr::Project { columns, input } => RelExpr::Project {
            columns: columns.clone(),
            input: Box::new(eliminate_join_with_fk(
                input, schema, applied,
            )),
        },
        RelExpr::Distinct { input } => RelExpr::Distinct {
            input: Box::new(eliminate_join_with_fk(
                input, schema, applied,
            )),
        },
        other => other.clone(),
    }
}

#[derive(Debug)]
enum FkSide {
    LeftReferenceRight,
    RightReferenceLeft,
}

/// Check if a join condition matches a foreign key relationship
/// between two tables.
fn find_fk_join_match<'a>(
    condition: &Expr,
    left_table: &str,
    right_table: &str,
    schema: &'a SchemaInfo,
) -> Option<(FkSide, &'a ConstraintInfo)> {
    let (left_col, right_col) =
        extract_eq_columns(condition)?;

    let left_info = schema.get_table(left_table)?;
    for fk in left_info.foreign_keys() {
        if fk.referenced_table.as_deref() == Some(right_table)
            && fk.columns.len() == 1
            && fk.referenced_columns.len() == 1
            && fk.columns[0] == left_col
            && fk.referenced_columns[0] == right_col
        {
            return Some((FkSide::LeftReferenceRight, fk));
        }
    }

    let right_info = schema.get_table(right_table)?;
    for fk in right_info.foreign_keys() {
        if fk.referenced_table.as_deref() == Some(left_table)
            && fk.columns.len() == 1
            && fk.referenced_columns.len() == 1
            && fk.columns[0] == right_col
            && fk.referenced_columns[0] == left_col
        {
            return Some((FkSide::RightReferenceLeft, fk));
        }
    }

    None
}

/// Extract column names from a simple equality condition
/// like `a.col = b.col`.
fn extract_eq_columns(expr: &Expr) -> Option<(String, String)> {
    if let Expr::BinOp {
        op: BinOp::Eq,
        left,
        right,
    } = expr
    {
        if let (Expr::Column(l), Expr::Column(r)) =
            (left.as_ref(), right.as_ref())
        {
            return Some((l.column.clone(), r.column.clone()));
        }
    }
    None
}

// ── helpers ──────────────────────────────────────────────────

/// Find the table name from the first Scan node in a tree.
fn find_scan_table(expr: &RelExpr) -> Option<String> {
    match expr {
        RelExpr::Scan { table, .. } => Some(table.clone()),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Window { input, .. } => {
            find_scan_table(input)
        }
        _ => None,
    }
}

/// Collect column names from projections, or all columns from
/// a scan.
fn collect_output_columns(expr: &RelExpr) -> Vec<String> {
    match expr {
        RelExpr::Project { columns, .. } => columns
            .iter()
            .filter_map(|pc| {
                if let Some(ref alias) = pc.alias {
                    Some(alias.clone())
                } else {
                    extract_column_name(&pc.expr)
                }
            })
            .collect(),
        RelExpr::Scan { .. } => Vec::new(),
        RelExpr::Filter { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. } => {
            collect_output_columns(input)
        }
        _ => Vec::new(),
    }
}

fn extract_column_name(expr: &Expr) -> Option<String> {
    if let Expr::Column(col_ref) = expr {
        Some(col_ref.column.clone())
    } else {
        None
    }
}

/// Collect table names referenced above the current node.
/// For FK elimination we need to check no parent node uses
/// the eliminated table's columns. As a simplified heuristic,
/// we collect table references from the entire expression.
fn collect_used_tables_above(expr: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    collect_used_tables_recursive(expr, &mut tables);
    tables
}

fn collect_used_tables_recursive(
    expr: &RelExpr,
    tables: &mut Vec<String>,
) {
    match expr {
        RelExpr::Scan { table, .. } => {
            tables.push(table.clone());
        }
        RelExpr::Filter { predicate, input } => {
            collect_table_refs_from_expr(predicate, tables);
            collect_used_tables_recursive(input, tables);
        }
        RelExpr::Project { columns, input } => {
            for col in columns {
                collect_table_refs_from_expr(&col.expr, tables);
            }
            collect_used_tables_recursive(input, tables);
        }
        RelExpr::Join {
            condition,
            left,
            right,
            ..
        } => {
            collect_table_refs_from_expr(condition, tables);
            collect_used_tables_recursive(left, tables);
            collect_used_tables_recursive(right, tables);
        }
        RelExpr::Distinct { input }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Window { input, .. } => {
            collect_used_tables_recursive(input, tables);
        }
        _ => {}
    }
}

fn collect_table_refs_from_expr(
    expr: &Expr,
    tables: &mut Vec<String>,
) {
    match expr {
        Expr::Column(ColumnRef {
            table: Some(t), ..
        }) => {
            if !tables.contains(t) {
                tables.push(t.clone());
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_table_refs_from_expr(left, tables);
            collect_table_refs_from_expr(right, tables);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_table_refs_from_expr(operand, tables);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_table_refs_from_expr(arg, tables);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::ProjectionColumn;
    use ra_metadata::schema::{
        ColumnInfo, ConstraintInfo, ConstraintKind, IndexInfo,
        SchemaInfo, TableInfo,
    };
    use std::collections::HashMap;

    fn test_schema() -> SchemaInfo {
        let mut tables = HashMap::new();

        tables.insert(
            "users".to_owned(),
            TableInfo {
                name: "users".to_owned(),
                columns: vec![
                    ColumnInfo {
                        name: "id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        ordinal: 1,
                        default_value: None,
                    },
                    ColumnInfo {
                        name: "name".to_owned(),
                        data_type: "text".to_owned(),
                        nullable: true,
                        ordinal: 2,
                        default_value: None,
                    },
                    ColumnInfo {
                        name: "email".to_owned(),
                        data_type: "text".to_owned(),
                        nullable: false,
                        ordinal: 3,
                        default_value: None,
                    },
                ],
                constraints: vec![
                    ConstraintInfo {
                        name: "users_pkey".to_owned(),
                        kind: ConstraintKind::PrimaryKey,
                        columns: vec!["id".to_owned()],
                        referenced_table: None,
                        referenced_columns: vec![],
                        check_expression: None,
                    },
                    ConstraintInfo {
                        name: "users_email_unique".to_owned(),
                        kind: ConstraintKind::Unique,
                        columns: vec!["email".to_owned()],
                        referenced_table: None,
                        referenced_columns: vec![],
                        check_expression: None,
                    },
                ],
                indexes: vec![IndexInfo {
                    name: "users_pkey".to_owned(),
                    columns: vec!["id".to_owned()],
                    unique: true,
                    index_type: "btree".to_owned(),
                }],
                triggers: vec![],
                estimated_rows: Some(1000.0),
            },
        );

        tables.insert(
            "orders".to_owned(),
            TableInfo {
                name: "orders".to_owned(),
                columns: vec![
                    ColumnInfo {
                        name: "id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        ordinal: 1,
                        default_value: None,
                    },
                    ColumnInfo {
                        name: "user_id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        ordinal: 2,
                        default_value: None,
                    },
                    ColumnInfo {
                        name: "amount".to_owned(),
                        data_type: "numeric".to_owned(),
                        nullable: true,
                        ordinal: 3,
                        default_value: None,
                    },
                ],
                constraints: vec![
                    ConstraintInfo {
                        name: "orders_pkey".to_owned(),
                        kind: ConstraintKind::PrimaryKey,
                        columns: vec!["id".to_owned()],
                        referenced_table: None,
                        referenced_columns: vec![],
                        check_expression: None,
                    },
                    ConstraintInfo {
                        name: "orders_user_fk".to_owned(),
                        kind: ConstraintKind::ForeignKey,
                        columns: vec!["user_id".to_owned()],
                        referenced_table: Some(
                            "users".to_owned(),
                        ),
                        referenced_columns: vec![
                            "id".to_owned(),
                        ],
                        check_expression: None,
                    },
                ],
                indexes: vec![],
                triggers: vec![],
                estimated_rows: Some(5000.0),
            },
        );

        SchemaInfo {
            kind: ra_metadata::schema::DatabaseKind::PostgreSQL,
            schema_name: "public".to_owned(),
            tables,
        }
    }

    #[test]
    fn prune_is_not_null_on_not_null_column() {
        let schema = test_schema();
        let expr = RelExpr::scan("users").filter(Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            operand: Box::new(Expr::Column(ColumnRef::new("id"))),
        });

        let result = optimize_with_constraints(&expr, &schema);
        assert!(!result.applied.is_empty());
        assert!(
            matches!(result.expr, RelExpr::Scan { .. }),
            "Filter should be removed"
        );
    }

    #[test]
    fn keep_is_not_null_on_nullable_column() {
        let schema = test_schema();
        let expr = RelExpr::scan("users").filter(Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            operand: Box::new(Expr::Column(ColumnRef::new(
                "name",
            ))),
        });

        let result = optimize_with_constraints(&expr, &schema);
        assert!(result.applied.is_empty());
        assert!(matches!(result.expr, RelExpr::Filter { .. }));
    }

    #[test]
    fn prune_is_not_null_in_conjunction() {
        let schema = test_schema();
        let predicate = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::UnaryOp {
                op: UnaryOp::IsNotNull,
                operand: Box::new(Expr::Column(ColumnRef::new(
                    "id",
                ))),
            }),
            right: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new(
                    "id",
                ))),
                right: Box::new(Expr::Const(
                    ra_core::expr::Const::Int(10),
                )),
            }),
        };
        let expr = RelExpr::scan("users").filter(predicate);

        let result = optimize_with_constraints(&expr, &schema);
        assert_eq!(result.applied.len(), 1);
        assert!(matches!(result.expr, RelExpr::Filter { .. }));
    }

    #[test]
    fn eliminate_distinct_with_pk() {
        let schema = test_schema();
        let proj = RelExpr::scan("users").project(vec![
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("id")),
                alias: None,
            },
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            },
        ]);
        let expr = proj.distinct();

        let result = optimize_with_constraints(&expr, &schema);
        assert!(!result.applied.is_empty());
        assert!(
            !matches!(result.expr, RelExpr::Distinct { .. }),
            "DISTINCT should be removed"
        );
    }

    #[test]
    fn keep_distinct_without_unique_columns() {
        let schema = test_schema();
        let proj = RelExpr::scan("users").project(vec![
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            },
        ]);
        let expr = proj.distinct();

        let result = optimize_with_constraints(&expr, &schema);
        assert!(result.applied.is_empty());
        assert!(matches!(result.expr, RelExpr::Distinct { .. }));
    }

    #[test]
    fn eliminate_distinct_with_unique_constraint() {
        let schema = test_schema();
        let proj = RelExpr::scan("users").project(vec![
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("email")),
                alias: None,
            },
        ]);
        let expr = proj.distinct();

        let result = optimize_with_constraints(&expr, &schema);
        assert!(!result.applied.is_empty());
    }

    #[test]
    fn find_scan_table_through_filter() {
        let expr = RelExpr::scan("users").filter(Expr::Const(
            ra_core::expr::Const::Bool(true),
        ));
        assert_eq!(
            find_scan_table(&expr),
            Some("users".to_owned())
        );
    }

    #[test]
    fn extract_eq_columns_basic() {
        let condition = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(
                ColumnRef::qualified("orders", "user_id"),
            )),
            right: Box::new(Expr::Column(
                ColumnRef::qualified("users", "id"),
            )),
        };
        let result = extract_eq_columns(&condition);
        assert_eq!(
            result,
            Some(("user_id".to_owned(), "id".to_owned()))
        );
    }

    #[test]
    fn full_optimization_pipeline() {
        let schema = test_schema();

        let expr = RelExpr::scan("users")
            .filter(Expr::UnaryOp {
                op: UnaryOp::IsNotNull,
                operand: Box::new(Expr::Column(ColumnRef::new(
                    "email",
                ))),
            })
            .project(vec![
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("id")),
                    alias: None,
                },
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("name")),
                    alias: None,
                },
            ])
            .distinct();

        let result = optimize_with_constraints(&expr, &schema);
        assert!(result.applied.len() >= 2);
    }

    #[test]
    fn is_covered_by_unique_basic() {
        let schema = test_schema();
        let users = schema.get_table("users").unwrap();
        assert!(is_covered_by_unique(
            &["id".to_owned(), "name".to_owned()],
            users
        ));
        assert!(!is_covered_by_unique(
            &["name".to_owned()],
            users
        ));
    }

    #[test]
    fn optimize_result_no_changes_for_plain_scan() {
        let schema = test_schema();
        let expr = RelExpr::scan("users");
        let result = optimize_with_constraints(&expr, &schema);
        assert!(result.applied.is_empty());
        assert!(matches!(result.expr, RelExpr::Scan { .. }));
    }
}
