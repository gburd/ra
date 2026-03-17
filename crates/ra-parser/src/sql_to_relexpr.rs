//! SQL to RelExpr converter using sqlparser.
//!
//! Supports basic SQL constructs:
//! - SELECT with projection list
//! - FROM single table or INNER JOIN
//! - WHERE with AND, OR, comparison operators
//! - GROUP BY with aggregates (COUNT, SUM, AVG, MIN, MAX)

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, ProjectionColumn, RelExpr,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use sqlparser::ast::{
    BinaryOperator, Expr as SqlExpr, FunctionArg, FunctionArgExpr,
    FunctionArguments, GroupByExpr, Join as SqlJoin, JoinConstraint,
    JoinOperator, ObjectName, Query, Select, SelectItem, SetExpr, Statement,
    TableFactor, TableWithJoins, UnaryOperator, Value,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use thiserror::Error;

/// Errors that can occur during SQL parsing and conversion.
#[derive(Debug, Error)]
pub enum SqlConversionError {
    /// SQL parsing failed.
    #[error("failed to parse SQL: {0}")]
    ParseError(String),

    /// Unsupported SQL construct.
    #[error("unsupported SQL feature: {0}")]
    UnsupportedFeature(String),

    /// Invalid SQL semantics.
    #[error("invalid SQL: {0}")]
    InvalidSql(String),
}

/// Parse SQL and convert to RelExpr.
///
/// # Errors
///
/// Returns error if SQL is invalid or contains unsupported features.
pub fn sql_to_relexpr(sql: &str) -> Result<RelExpr, SqlConversionError> {
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, sql)
        .map_err(|e| SqlConversionError::ParseError(e.to_string()))?;

    if statements.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "no SQL statement found".to_owned(),
        ));
    }

    if statements.len() > 1 {
        return Err(SqlConversionError::UnsupportedFeature(
            "multiple statements not supported".to_owned(),
        ));
    }

    match &statements[0] {
        Statement::Query(query) => convert_query(query),
        _ => Err(SqlConversionError::UnsupportedFeature(
            "only SELECT queries are supported".to_owned(),
        )),
    }
}

fn convert_query(query: &Query) -> Result<RelExpr, SqlConversionError> {
    if query.with.is_some() {
        return Err(SqlConversionError::UnsupportedFeature(
            "WITH/CTE not supported".to_owned(),
        ));
    }

    if query.order_by.is_some() {
        return Err(SqlConversionError::UnsupportedFeature(
            "ORDER BY not yet supported".to_owned(),
        ));
    }

    if query.limit.is_some() || query.offset.is_some() {
        return Err(SqlConversionError::UnsupportedFeature(
            "LIMIT/OFFSET not yet supported".to_owned(),
        ));
    }

    match &*query.body {
        SetExpr::Select(select) => convert_select(select),
        _ => Err(SqlConversionError::UnsupportedFeature(
            "only simple SELECT supported (no UNION/INTERSECT/EXCEPT)".to_owned(),
        )),
    }
}

fn convert_select(select: &Select) -> Result<RelExpr, SqlConversionError> {
    if select.distinct.is_some() {
        return Err(SqlConversionError::UnsupportedFeature(
            "DISTINCT not yet supported".to_owned(),
        ));
    }

    if !select.cluster_by.is_empty()
        || !select.distribute_by.is_empty()
        || !select.sort_by.is_empty()
    {
        return Err(SqlConversionError::UnsupportedFeature(
            "CLUSTER BY/DISTRIBUTE BY/SORT BY not supported".to_owned(),
        ));
    }

    if select.having.is_some() {
        return Err(SqlConversionError::UnsupportedFeature(
            "HAVING not yet supported".to_owned(),
        ));
    }

    // Start with FROM clause
    let mut plan = convert_from(&select.from)?;

    // Apply WHERE clause
    if let Some(ref where_expr) = select.selection {
        let predicate = convert_expr(where_expr)?;
        plan = RelExpr::Filter {
            predicate,
            input: Box::new(plan),
        };
    }

    // Apply GROUP BY (only if there are actual grouping expressions or aggregates)
    let has_group_by = !matches!(select.group_by, GroupByExpr::All(ref exprs) if exprs.is_empty());
    let has_aggregates = select.projection.iter().any(|item| {
        match item {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => is_aggregate(expr),
            _ => false,
        }
    });

    if has_group_by || has_aggregates {
        plan = convert_group_by(plan, select)?;
    }

    // Apply projection (SELECT list)
    plan = convert_projection(plan, &select.projection)?;

    Ok(plan)
}

fn convert_from(
    from: &[TableWithJoins],
) -> Result<RelExpr, SqlConversionError> {
    if from.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "SELECT without FROM not supported".to_owned(),
        ));
    }

    if from.len() > 1 {
        return Err(SqlConversionError::UnsupportedFeature(
            "multiple FROM items (implicit cross join) not supported".to_owned(),
        ));
    }

    convert_table_with_joins(&from[0])
}

fn convert_table_with_joins(
    twj: &TableWithJoins,
) -> Result<RelExpr, SqlConversionError> {
    let mut plan = convert_table_factor(&twj.relation)?;

    for join in &twj.joins {
        plan = convert_join(plan, join)?;
    }

    Ok(plan)
}

fn convert_table_factor(
    table: &TableFactor,
) -> Result<RelExpr, SqlConversionError> {
    match table {
        TableFactor::Table {
            name,
            alias,
            ..
        } => {
            let table_name = object_name_to_string(name);
            let alias_name = alias.as_ref().map(|a| a.name.value.clone());

            Ok(RelExpr::Scan {
                table: table_name,
                alias: alias_name,
            })
        }
        TableFactor::Derived { .. } => Err(SqlConversionError::UnsupportedFeature(
            "subqueries not yet supported".to_owned(),
        )),
        TableFactor::TableFunction { .. } => {
            Err(SqlConversionError::UnsupportedFeature(
                "table functions not supported".to_owned(),
            ))
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported table factor".to_owned(),
        )),
    }
}

fn convert_join(
    left: RelExpr,
    join: &SqlJoin,
) -> Result<RelExpr, SqlConversionError> {
    let right = convert_table_factor(&join.relation)?;

    let join_type = match &join.join_operator {
        JoinOperator::Inner(_) => JoinType::Inner,
        JoinOperator::LeftOuter(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "LEFT OUTER JOIN not yet supported".to_owned(),
            ))
        }
        JoinOperator::RightOuter(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "RIGHT OUTER JOIN not yet supported".to_owned(),
            ))
        }
        JoinOperator::FullOuter(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "FULL OUTER JOIN not yet supported".to_owned(),
            ))
        }
        JoinOperator::CrossJoin => {
            return Err(SqlConversionError::UnsupportedFeature(
                "CROSS JOIN not yet supported".to_owned(),
            ))
        }
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(
                "unsupported join type".to_owned(),
            ))
        }
    };

    let condition = match &join.join_operator {
        JoinOperator::Inner(constraint) => match constraint {
            JoinConstraint::On(expr) => convert_expr(expr)?,
            JoinConstraint::Using(_) => {
                return Err(SqlConversionError::UnsupportedFeature(
                    "USING clause not yet supported".to_owned(),
                ))
            }
            JoinConstraint::Natural => {
                return Err(SqlConversionError::UnsupportedFeature(
                    "NATURAL JOIN not supported".to_owned(),
                ))
            }
            JoinConstraint::None => Expr::Const(Const::Bool(true)),
        },
        _ => Expr::Const(Const::Bool(true)),
    };

    Ok(RelExpr::Join {
        join_type,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn convert_group_by(
    input: RelExpr,
    select: &Select,
) -> Result<RelExpr, SqlConversionError> {
    let group_by_exprs = match &select.group_by {
        GroupByExpr::All(_) => vec![],
        GroupByExpr::Expressions(exprs, _) => exprs.clone(),
    };

    let group_by: Result<Vec<_>, _> = group_by_exprs
        .iter()
        .map(convert_expr)
        .collect();
    let group_by = group_by?;

    // Extract aggregates from projection
    let mut aggregates = Vec::new();
    for item in &select.projection {
        if let SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } = item {
            if let Some(agg) = try_extract_aggregate(expr, item)? {
                aggregates.push(agg);
            }
        }
    }

    // Only create Aggregate node if there are actually grouping expressions or aggregates
    if group_by.is_empty() && aggregates.is_empty() {
        return Ok(input);
    }

    Ok(RelExpr::Aggregate {
        group_by,
        aggregates,
        input: Box::new(input),
    })
}

fn try_extract_aggregate(
    expr: &SqlExpr,
    item: &SelectItem,
) -> Result<Option<AggregateExpr>, SqlConversionError> {
    match expr {
        SqlExpr::Function(func) => {
            let func_name = func.name.to_string().to_uppercase();
            let agg_func = match func_name.as_str() {
                "COUNT" => AggregateFunction::Count,
                "SUM" => AggregateFunction::Sum,
                "AVG" => AggregateFunction::Avg,
                "MIN" => AggregateFunction::Min,
                "MAX" => AggregateFunction::Max,
                _ => return Ok(None),
            };

            let func_args = match &func.args {
                FunctionArguments::None => vec![],
                FunctionArguments::Subquery(_) => {
                    return Err(SqlConversionError::UnsupportedFeature(
                        "subquery in aggregate not supported".to_owned(),
                    ))
                }
                FunctionArguments::List(args) => args.args.clone(),
            };

            let arg = if func_args.is_empty() {
                None
            } else if func_args.len() == 1 {
                match &func_args[0] {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                        Some(convert_expr(e)?)
                    }
                    FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => None,
                    _ => {
                        return Err(SqlConversionError::UnsupportedFeature(
                            "complex aggregate arguments not supported".to_owned(),
                        ))
                    }
                }
            } else {
                return Err(SqlConversionError::UnsupportedFeature(
                    "multi-argument aggregates not supported".to_owned(),
                ));
            };

            let alias = match item {
                SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.clone()),
                _ => None,
            };

            // Check if DISTINCT is specified in the args
            let distinct = match &func.args {
                FunctionArguments::List(list) => list.duplicate_treatment.is_some(),
                _ => false,
            };

            Ok(Some(AggregateExpr {
                function: agg_func,
                arg,
                distinct,
                alias,
            }))
        }
        _ => Ok(None),
    }
}

fn convert_projection(
    input: RelExpr,
    projection: &[SelectItem],
) -> Result<RelExpr, SqlConversionError> {
    if projection.len() == 1 {
        if let SelectItem::Wildcard(_) = projection[0] {
            // SELECT * - no projection needed
            return Ok(input);
        }
    }

    let mut columns = Vec::new();

    for item in projection {
        match item {
            SelectItem::UnnamedExpr(expr) => {
                // Skip aggregates - they're handled in GROUP BY
                if !is_aggregate(expr) {
                    columns.push(ProjectionColumn {
                        expr: convert_expr(expr)?,
                        alias: None,
                    });
                }
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                if !is_aggregate(expr) {
                    columns.push(ProjectionColumn {
                        expr: convert_expr(expr)?,
                        alias: Some(alias.value.clone()),
                    });
                }
            }
            SelectItem::QualifiedWildcard(obj_name, _) => {
                return Err(SqlConversionError::UnsupportedFeature(format!(
                    "qualified wildcard {obj_name}.* not yet supported"
                )))
            }
            SelectItem::Wildcard(_) => {
                return Err(SqlConversionError::UnsupportedFeature(
                    "wildcard in multi-column projection not yet supported".to_owned(),
                ))
            }
        }
    }

    // If all columns were aggregates, don't add projection
    if columns.is_empty() {
        return Ok(input);
    }

    Ok(RelExpr::Project {
        columns,
        input: Box::new(input),
    })
}

fn is_aggregate(expr: &SqlExpr) -> bool {
    matches!(
        expr,
        SqlExpr::Function(func) if matches!(
            func.name.to_string().to_uppercase().as_str(),
            "COUNT" | "SUM" | "AVG" | "MIN" | "MAX"
        )
    )
}

fn convert_expr(expr: &SqlExpr) -> Result<Expr, SqlConversionError> {
    match expr {
        SqlExpr::Identifier(ident) => Ok(Expr::Column(ColumnRef::new(&ident.value))),
        SqlExpr::CompoundIdentifier(idents) => {
            if idents.len() == 2 {
                Ok(Expr::Column(ColumnRef::qualified(
                    &idents[0].value,
                    &idents[1].value,
                )))
            } else {
                Err(SqlConversionError::UnsupportedFeature(format!(
                    "complex identifier with {} parts not supported",
                    idents.len()
                )))
            }
        }
        SqlExpr::Value(val) => convert_value(val),
        SqlExpr::BinaryOp { left, op, right } => {
            let bin_op = convert_binary_op(op)?;
            Ok(Expr::BinOp {
                op: bin_op,
                left: Box::new(convert_expr(left)?),
                right: Box::new(convert_expr(right)?),
            })
        }
        SqlExpr::UnaryOp { op, expr } => {
            let unary_op = convert_unary_op(op)?;
            Ok(Expr::UnaryOp {
                op: unary_op,
                operand: Box::new(convert_expr(expr)?),
            })
        }
        SqlExpr::Nested(inner) => convert_expr(inner),
        SqlExpr::IsNull(expr) => Ok(Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::IsNull,
            operand: Box::new(convert_expr(expr)?),
        }),
        SqlExpr::IsNotNull(expr) => Ok(Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::IsNotNull,
            operand: Box::new(convert_expr(expr)?),
        }),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "expression type not yet supported: {expr:?}"
        ))),
    }
}

fn convert_value(val: &Value) -> Result<Expr, SqlConversionError> {
    match val {
        Value::Number(n, _) => {
            if let Ok(i) = n.parse::<i64>() {
                Ok(Expr::Const(Const::Int(i)))
            } else if let Ok(f) = n.parse::<f64>() {
                Ok(Expr::Const(Const::Float(f)))
            } else {
                Err(SqlConversionError::InvalidSql(format!(
                    "invalid number: {n}"
                )))
            }
        }
        Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) => {
            Ok(Expr::Const(Const::String(s.clone())))
        }
        Value::Boolean(b) => Ok(Expr::Const(Const::Bool(*b))),
        Value::Null => Ok(Expr::Const(Const::Null)),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "value type not supported: {val:?}"
        ))),
    }
}

fn convert_binary_op(
    op: &BinaryOperator,
) -> Result<BinOp, SqlConversionError> {
    match op {
        BinaryOperator::Plus => Ok(BinOp::Add),
        BinaryOperator::Minus => Ok(BinOp::Sub),
        BinaryOperator::Multiply => Ok(BinOp::Mul),
        BinaryOperator::Divide => Ok(BinOp::Div),
        BinaryOperator::Eq => Ok(BinOp::Eq),
        BinaryOperator::NotEq => Ok(BinOp::Ne),
        BinaryOperator::Lt => Ok(BinOp::Lt),
        BinaryOperator::LtEq => Ok(BinOp::Le),
        BinaryOperator::Gt => Ok(BinOp::Gt),
        BinaryOperator::GtEq => Ok(BinOp::Ge),
        BinaryOperator::And => Ok(BinOp::And),
        BinaryOperator::Or => Ok(BinOp::Or),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "binary operator not supported: {op:?}"
        ))),
    }
}

fn convert_unary_op(
    op: &UnaryOperator,
) -> Result<ra_core::expr::UnaryOp, SqlConversionError> {
    match op {
        UnaryOperator::Not => Ok(ra_core::expr::UnaryOp::Not),
        UnaryOperator::Minus => Ok(ra_core::expr::UnaryOp::Neg),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "unary operator not supported: {op:?}"
        ))),
    }
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.0.iter().map(|i| i.value.as_str()).collect::<Vec<_>>().join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_select() {
        let sql = "SELECT * FROM users";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_where() {
        let sql = "SELECT * FROM users WHERE age > 18";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_join() {
        let sql = "SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_aggregate() {
        let sql = "SELECT region, COUNT(*), SUM(amount) FROM orders GROUP BY region";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unsupported_subquery() {
        let sql = "SELECT * FROM (SELECT * FROM users) t";
        let result = sql_to_relexpr(sql);
        assert!(matches!(
            result,
            Err(SqlConversionError::UnsupportedFeature(_))
        ));
    }
}
