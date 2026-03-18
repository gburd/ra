//! SQL to RelExpr converter using sqlparser.
//!
//! Supports SQL constructs:
//! - SELECT with projection list, DISTINCT
//! - FROM single table, subqueries, multiple FROM items
//! - JOINs: INNER, LEFT/RIGHT/FULL OUTER, CROSS
//! - WHERE with AND, OR, comparison operators
//! - GROUP BY with aggregates (COUNT, SUM, AVG, MIN, MAX, etc.)
//! - HAVING (converted to Filter after Aggregate)
//! - ORDER BY with ASC/DESC, NULLS FIRST/LAST
//! - LIMIT / OFFSET
//! - WITH / CTE (Common Table Expressions)
//! - UNION / INTERSECT / EXCEPT (with ALL)
//! - Scalar subqueries and derived tables

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use sqlparser::ast::{
    BinaryOperator, Expr as SqlExpr, FunctionArg, FunctionArgExpr,
    FunctionArguments, GroupByExpr, Join as SqlJoin, JoinConstraint,
    JoinOperator, ObjectName, Query, Select, SelectItem, SetExpr,
    SetOperator, SetQuantifier, Statement, TableFactor,
    TableWithJoins, UnaryOperator, Value,
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
pub fn sql_to_relexpr(
    sql: &str,
) -> Result<RelExpr, SqlConversionError> {
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

fn convert_query(
    query: &Query,
) -> Result<RelExpr, SqlConversionError> {
    let mut plan = convert_set_expr(&query.body)?;

    // Wrap with CTEs if present
    if let Some(with_clause) = &query.with {
        for cte in with_clause.cte_tables.iter().rev() {
            let cte_name = cte.alias.name.value.clone();
            let cte_def = convert_query(&cte.query)?;
            plan = RelExpr::Cte {
                name: cte_name,
                definition: Box::new(cte_def),
                body: Box::new(plan),
            };
        }
    }

    // Apply ORDER BY
    if let Some(order_by) = &query.order_by {
        let keys = convert_order_by_exprs(&order_by.exprs)?;
        if !keys.is_empty() {
            plan = RelExpr::Sort {
                keys,
                input: Box::new(plan),
            };
        }
    }

    // Apply LIMIT / OFFSET
    if query.limit.is_some() || query.offset.is_some() {
        let count = match &query.limit {
            Some(expr) => convert_limit_expr(expr)?,
            None => u64::MAX,
        };
        let offset = match &query.offset {
            Some(off) => convert_limit_expr(&off.value)?,
            None => 0,
        };
        plan = RelExpr::Limit {
            count,
            offset,
            input: Box::new(plan),
        };
    }

    Ok(plan)
}

fn convert_set_expr(
    set_expr: &SetExpr,
) -> Result<RelExpr, SqlConversionError> {
    match set_expr {
        SetExpr::Select(select) => convert_select(select),
        SetExpr::Query(query) => convert_query(query),
        SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            right,
        } => {
            let left_plan = convert_set_expr(left)?;
            let right_plan = convert_set_expr(right)?;
            let all = matches!(
                set_quantifier,
                SetQuantifier::All | SetQuantifier::AllByName
            );
            match op {
                SetOperator::Union => Ok(RelExpr::Union {
                    all,
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                }),
                SetOperator::Intersect => Ok(RelExpr::Intersect {
                    all,
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                }),
                SetOperator::Except => Ok(RelExpr::Except {
                    all,
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                }),
            }
        }
        SetExpr::Values(values) => {
            let rows: Result<Vec<Vec<Expr>>, _> = values
                .rows
                .iter()
                .map(|row| {
                    row.iter().map(convert_expr).collect()
                })
                .collect();
            Ok(RelExpr::Values { rows: rows? })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported set expression".to_owned(),
        )),
    }
}

fn convert_limit_expr(
    expr: &SqlExpr,
) -> Result<u64, SqlConversionError> {
    match expr {
        SqlExpr::Value(Value::Number(n, _)) => {
            n.parse::<u64>().map_err(|_| {
                SqlConversionError::InvalidSql(format!(
                    "invalid LIMIT/OFFSET value: {n}"
                ))
            })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "only integer LIMIT/OFFSET supported".to_owned(),
        )),
    }
}

fn convert_order_by_exprs(
    exprs: &[sqlparser::ast::OrderByExpr],
) -> Result<Vec<SortKey>, SqlConversionError> {
    let mut keys = Vec::with_capacity(exprs.len());
    for obe in exprs {
        let expr = convert_expr(&obe.expr)?;
        let direction = match obe.asc {
            Some(true) | None => SortDirection::Asc,
            Some(false) => SortDirection::Desc,
        };
        let nulls = match obe.nulls_first {
            Some(true) => NullOrdering::First,
            Some(false) => NullOrdering::Last,
            None => match direction {
                SortDirection::Asc => NullOrdering::Last,
                SortDirection::Desc => NullOrdering::First,
            },
        };
        keys.push(SortKey {
            expr,
            direction,
            nulls,
        });
    }
    Ok(keys)
}

fn convert_select(
    select: &Select,
) -> Result<RelExpr, SqlConversionError> {
    let is_distinct = select.distinct.is_some();

    if !select.cluster_by.is_empty()
        || !select.distribute_by.is_empty()
        || !select.sort_by.is_empty()
    {
        return Err(SqlConversionError::UnsupportedFeature(
            "CLUSTER BY/DISTRIBUTE BY/SORT BY not supported"
                .to_owned(),
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

    // Apply GROUP BY
    let has_group_by = !matches!(
        select.group_by,
        GroupByExpr::All(ref exprs) if exprs.is_empty()
    );
    let has_aggregates =
        select.projection.iter().any(|item| match item {
            SelectItem::UnnamedExpr(expr)
            | SelectItem::ExprWithAlias { expr, .. } => {
                is_aggregate(expr)
            }
            _ => false,
        });

    if has_group_by || has_aggregates {
        plan = convert_group_by(plan, select)?;
    }

    // Apply HAVING (as a Filter after Aggregate)
    if let Some(ref having_expr) = select.having {
        let predicate = convert_having_expr(having_expr)?;
        plan = RelExpr::Filter {
            predicate,
            input: Box::new(plan),
        };
    }

    // Apply projection (SELECT list)
    plan = convert_projection(plan, &select.projection)?;

    // Apply DISTINCT
    if is_distinct {
        plan = RelExpr::Distinct {
            input: Box::new(plan),
        };
    }

    Ok(plan)
}

fn convert_having_expr(
    expr: &SqlExpr,
) -> Result<Expr, SqlConversionError> {
    match expr {
        SqlExpr::BinaryOp { left, op, right } => {
            let bin_op = convert_binary_op(op)?;
            Ok(Expr::BinOp {
                op: bin_op,
                left: Box::new(convert_having_expr(left)?),
                right: Box::new(convert_having_expr(right)?),
            })
        }
        SqlExpr::Function(func) => {
            let func_name = func.name.to_string().to_uppercase();
            if matches!(
                func_name.as_str(),
                "COUNT"
                    | "SUM"
                    | "AVG"
                    | "MIN"
                    | "MAX"
                    | "STDDEV_POP"
                    | "STDDEV_SAMP"
                    | "VAR_POP"
                    | "VAR_SAMP"
            ) {
                Ok(Expr::Function {
                    name: func_name,
                    args: extract_func_args(func)?,
                })
            } else {
                convert_expr(expr)
            }
        }
        _ => convert_expr(expr),
    }
}

fn extract_func_args(
    func: &sqlparser::ast::Function,
) -> Result<Vec<Expr>, SqlConversionError> {
    let func_args = match &func.args {
        FunctionArguments::None => return Ok(vec![]),
        FunctionArguments::Subquery(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "subquery in function not supported".to_owned(),
            ))
        }
        FunctionArguments::List(args) => &args.args,
    };
    let mut result = Vec::new();
    for arg in func_args {
        match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                result.push(convert_expr(e)?);
            }
            FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => {}
            _ => {
                return Err(
                    SqlConversionError::UnsupportedFeature(
                        "complex function arguments not supported"
                            .to_owned(),
                    ),
                )
            }
        }
    }
    Ok(result)
}

fn convert_from(
    from: &[TableWithJoins],
) -> Result<RelExpr, SqlConversionError> {
    if from.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "SELECT without FROM not supported".to_owned(),
        ));
    }

    let mut plan = convert_table_with_joins(&from[0])?;

    // Handle multiple FROM items as implicit cross joins
    for twj in &from[1..] {
        let right = convert_table_with_joins(twj)?;
        plan = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(plan),
            right: Box::new(right),
        };
    }

    Ok(plan)
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
        TableFactor::Table { name, alias, .. } => {
            let table_name = object_name_to_string(name);
            let alias_name =
                alias.as_ref().map(|a| a.name.value.clone());

            Ok(RelExpr::Scan {
                table: table_name,
                alias: alias_name,
            })
        }
        TableFactor::Derived {
            subquery, alias, ..
        } => {
            let mut plan = convert_query(subquery)?;
            if let Some(a) = alias {
                plan = wrap_with_alias(plan, &a.name.value);
            }
            Ok(plan)
        }
        TableFactor::NestedJoin {
            table_with_joins,
            alias,
        } => {
            let mut plan =
                convert_table_with_joins(table_with_joins)?;
            if let Some(a) = alias {
                plan = wrap_with_alias(plan, &a.name.value);
            }
            Ok(plan)
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported table factor".to_owned(),
        )),
    }
}

fn wrap_with_alias(plan: RelExpr, alias: &str) -> RelExpr {
    RelExpr::Project {
        columns: vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("*")),
            alias: Some(alias.to_owned()),
        }],
        input: Box::new(plan),
    }
}

fn convert_join(
    left: RelExpr,
    join: &SqlJoin,
) -> Result<RelExpr, SqlConversionError> {
    let right = convert_table_factor(&join.relation)?;

    let (join_type, constraint) = match &join.join_operator {
        JoinOperator::Inner(c) => (JoinType::Inner, Some(c)),
        JoinOperator::LeftOuter(c) => {
            (JoinType::LeftOuter, Some(c))
        }
        JoinOperator::RightOuter(c) => {
            (JoinType::RightOuter, Some(c))
        }
        JoinOperator::FullOuter(c) => {
            (JoinType::FullOuter, Some(c))
        }
        JoinOperator::CrossJoin => (JoinType::Cross, None),
        JoinOperator::LeftSemi(c) => (JoinType::Semi, Some(c)),
        JoinOperator::LeftAnti(c) => (JoinType::Anti, Some(c)),
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(
                "unsupported join type".to_owned(),
            ))
        }
    };

    let condition = match constraint {
        Some(c) => convert_join_constraint(c)?,
        None => Expr::Const(Const::Bool(true)),
    };

    Ok(RelExpr::Join {
        join_type,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn convert_join_constraint(
    constraint: &JoinConstraint,
) -> Result<Expr, SqlConversionError> {
    match constraint {
        JoinConstraint::On(expr) => convert_expr(expr),
        JoinConstraint::Using(columns) => {
            let mut conditions = Vec::new();
            for col in columns {
                let col_name = &col.value;
                conditions.push(Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(
                        ColumnRef::new(col_name),
                    )),
                    right: Box::new(Expr::Column(
                        ColumnRef::new(col_name),
                    )),
                });
            }
            if conditions.is_empty() {
                Ok(Expr::Const(Const::Bool(true)))
            } else {
                let mut result = conditions.remove(0);
                for cond in conditions {
                    result = Expr::BinOp {
                        op: BinOp::And,
                        left: Box::new(result),
                        right: Box::new(cond),
                    };
                }
                Ok(result)
            }
        }
        JoinConstraint::Natural => {
            Err(SqlConversionError::UnsupportedFeature(
                "NATURAL JOIN not supported".to_owned(),
            ))
        }
        JoinConstraint::None => Ok(Expr::Const(Const::Bool(true))),
    }
}

fn convert_group_by(
    input: RelExpr,
    select: &Select,
) -> Result<RelExpr, SqlConversionError> {
    let group_by_exprs = match &select.group_by {
        GroupByExpr::All(_) => vec![],
        GroupByExpr::Expressions(exprs, _) => exprs.clone(),
    };

    let group_by: Result<Vec<_>, _> =
        group_by_exprs.iter().map(convert_expr).collect();
    let group_by = group_by?;

    let mut aggregates = Vec::new();
    for item in &select.projection {
        if let SelectItem::UnnamedExpr(expr)
        | SelectItem::ExprWithAlias { expr, .. } = item
        {
            if let Some(agg) =
                try_extract_aggregate(expr, item)?
            {
                aggregates.push(agg);
            }
        }
    }

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
            let func_name =
                func.name.to_string().to_uppercase();
            let agg_func = match func_name.as_str() {
                "COUNT" => AggregateFunction::Count,
                "SUM" => AggregateFunction::Sum,
                "AVG" => AggregateFunction::Avg,
                "MIN" => AggregateFunction::Min,
                "MAX" => AggregateFunction::Max,
                "STDDEV_POP" | "STDDEV" => {
                    AggregateFunction::StddevPop
                }
                "STDDEV_SAMP" => AggregateFunction::StddevSamp,
                "VAR_POP" | "VARIANCE" => {
                    AggregateFunction::VariancePop
                }
                "VAR_SAMP" => AggregateFunction::VarianceSamp,
                "STRING_AGG" => AggregateFunction::StringAgg,
                "ARRAY_AGG" => AggregateFunction::ArrayAgg,
                "MODE" => AggregateFunction::Mode,
                "BOOL_AND" => AggregateFunction::BoolAnd,
                "BOOL_OR" => AggregateFunction::BoolOr,
                _ => return Ok(None),
            };

            let func_args = match &func.args {
                FunctionArguments::None => vec![],
                FunctionArguments::Subquery(_) => {
                    return Err(
                        SqlConversionError::UnsupportedFeature(
                            "subquery in aggregate not supported"
                                .to_owned(),
                        ),
                    )
                }
                FunctionArguments::List(args) => {
                    args.args.clone()
                }
            };

            let arg = if func_args.is_empty() {
                None
            } else if func_args.len() == 1 {
                match &func_args[0] {
                    FunctionArg::Unnamed(
                        FunctionArgExpr::Expr(e),
                    ) => Some(convert_expr(e)?),
                    FunctionArg::Unnamed(
                        FunctionArgExpr::Wildcard,
                    ) => None,
                    _ => {
                        return Err(
                            SqlConversionError::UnsupportedFeature(
                                "complex aggregate arguments not supported".to_owned(),
                            ),
                        )
                    }
                }
            } else {
                // Multi-arg: use the first arg
                match &func_args[0] {
                    FunctionArg::Unnamed(
                        FunctionArgExpr::Expr(e),
                    ) => Some(convert_expr(e)?),
                    _ => None,
                }
            };

            let alias = match item {
                SelectItem::ExprWithAlias { alias, .. } => {
                    Some(alias.value.clone())
                }
                _ => None,
            };

            let distinct = match &func.args {
                FunctionArguments::List(list) => {
                    list.duplicate_treatment.is_some()
                }
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
            return Ok(input);
        }
    }

    let mut columns = Vec::new();

    for item in projection {
        match item {
            SelectItem::UnnamedExpr(expr) => {
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
                columns.push(ProjectionColumn {
                    expr: Expr::Column(ColumnRef::qualified(
                        &object_name_to_string(obj_name),
                        "*",
                    )),
                    alias: None,
                });
            }
            SelectItem::Wildcard(_) => {
                columns.push(ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("*")),
                    alias: None,
                });
            }
        }
    }

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
            | "STDDEV_POP" | "STDDEV_SAMP" | "STDDEV"
            | "VAR_POP" | "VAR_SAMP" | "VARIANCE"
            | "STRING_AGG" | "ARRAY_AGG" | "MODE"
            | "BOOL_AND" | "BOOL_OR"
        )
    )
}

fn convert_expr(
    expr: &SqlExpr,
) -> Result<Expr, SqlConversionError> {
    match expr {
        SqlExpr::Identifier(ident) => {
            Ok(Expr::Column(ColumnRef::new(&ident.value)))
        }
        SqlExpr::CompoundIdentifier(idents) => {
            if idents.len() == 2 {
                Ok(Expr::Column(ColumnRef::qualified(
                    &idents[0].value,
                    &idents[1].value,
                )))
            } else {
                Err(SqlConversionError::UnsupportedFeature(
                    format!(
                        "complex identifier with {} parts \
                         not supported",
                        idents.len()
                    ),
                ))
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
        SqlExpr::Function(func) => {
            let name = func.name.to_string();
            let args = extract_func_args(func)?;
            Ok(Expr::Function { name, args })
        }
        SqlExpr::Cast {
            expr, data_type, ..
        } => Ok(Expr::Cast {
            expr: Box::new(convert_expr(expr)?),
            target_type: data_type.to_string(),
        }),
        SqlExpr::Between {
            expr,
            negated,
            low,
            high,
        } => {
            let col = convert_expr(expr)?;
            let low_e = convert_expr(low)?;
            let high_e = convert_expr(high)?;
            let between = Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::BinOp {
                    op: BinOp::Ge,
                    left: Box::new(col.clone()),
                    right: Box::new(low_e),
                }),
                right: Box::new(Expr::BinOp {
                    op: BinOp::Le,
                    left: Box::new(col),
                    right: Box::new(high_e),
                }),
            };
            if *negated {
                Ok(Expr::UnaryOp {
                    op: ra_core::expr::UnaryOp::Not,
                    operand: Box::new(between),
                })
            } else {
                Ok(between)
            }
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let col = convert_expr(expr)?;
            let mut conditions = Vec::new();
            for val in list {
                conditions.push(Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(col.clone()),
                    right: Box::new(convert_expr(val)?),
                });
            }
            let combined = if conditions.is_empty() {
                Expr::Const(Const::Bool(false))
            } else {
                let mut result = conditions.remove(0);
                for cond in conditions {
                    result = Expr::BinOp {
                        op: BinOp::Or,
                        left: Box::new(result),
                        right: Box::new(cond),
                    };
                }
                result
            };
            if *negated {
                Ok(Expr::UnaryOp {
                    op: ra_core::expr::UnaryOp::Not,
                    operand: Box::new(combined),
                })
            } else {
                Ok(combined)
            }
        }
        SqlExpr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => {
            let op = operand
                .as_ref()
                .map(|e| convert_expr(e))
                .transpose()?
                .map(Box::new);
            let mut when_clauses = Vec::new();
            for (cond, res) in
                conditions.iter().zip(results.iter())
            {
                when_clauses.push((
                    convert_expr(cond)?,
                    convert_expr(res)?,
                ));
            }
            let else_r = else_result
                .as_ref()
                .map(|e| convert_expr(e))
                .transpose()?
                .map(Box::new);
            Ok(Expr::Case {
                operand: op,
                when_clauses,
                else_result: else_r,
            })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            format!(
                "expression type not yet supported: {expr:?}"
            ),
        )),
    }
}

fn convert_value(
    val: &Value,
) -> Result<Expr, SqlConversionError> {
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
        Value::SingleQuotedString(s)
        | Value::DoubleQuotedString(s) => {
            Ok(Expr::Const(Const::String(s.clone())))
        }
        Value::Boolean(b) => Ok(Expr::Const(Const::Bool(*b))),
        Value::Null => Ok(Expr::Const(Const::Null)),
        _ => Err(SqlConversionError::UnsupportedFeature(
            format!("value type not supported: {val:?}"),
        )),
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
        _ => Err(SqlConversionError::UnsupportedFeature(
            format!("binary operator not supported: {op:?}"),
        )),
    }
}

fn convert_unary_op(
    op: &UnaryOperator,
) -> Result<ra_core::expr::UnaryOp, SqlConversionError> {
    match op {
        UnaryOperator::Not => Ok(ra_core::expr::UnaryOp::Not),
        UnaryOperator::Minus => Ok(ra_core::expr::UnaryOp::Neg),
        _ => Err(SqlConversionError::UnsupportedFeature(
            format!("unary operator not supported: {op:?}"),
        )),
    }
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .iter()
        .map(|i| i.value.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_parses(sql: &str) -> RelExpr {
        sql_to_relexpr(sql).unwrap_or_else(|e| {
            panic!("failed to parse '{sql}': {e}")
        })
    }

    fn assert_fails(sql: &str) {
        assert!(
            sql_to_relexpr(sql).is_err(),
            "expected parse failure for: {sql}"
        );
    }

    // === Basic SELECT ===

    #[test]
    fn test_simple_select() {
        assert_parses("SELECT * FROM users");
    }

    #[test]
    fn test_select_columns() {
        let plan = assert_parses("SELECT name, age FROM users");
        assert!(matches!(plan, RelExpr::Project { .. }));
    }

    #[test]
    fn test_select_with_alias() {
        let plan = assert_parses(
            "SELECT name AS n, age AS a FROM users",
        );
        if let RelExpr::Project { columns, .. } = &plan {
            assert_eq!(columns[0].alias.as_deref(), Some("n"));
            assert_eq!(columns[1].alias.as_deref(), Some("a"));
        } else {
            panic!("expected Project");
        }
    }

    // === WHERE ===

    #[test]
    fn test_select_with_where() {
        let plan =
            assert_parses("SELECT * FROM users WHERE age > 18");
        assert!(matches!(plan, RelExpr::Filter { .. }));
    }

    #[test]
    fn test_where_and() {
        assert_parses(
            "SELECT * FROM t WHERE a > 1 AND b < 10",
        );
    }

    #[test]
    fn test_where_or() {
        assert_parses(
            "SELECT * FROM t WHERE a = 1 OR b = 2",
        );
    }

    #[test]
    fn test_where_is_null() {
        assert_parses("SELECT * FROM t WHERE x IS NULL");
    }

    #[test]
    fn test_where_is_not_null() {
        assert_parses("SELECT * FROM t WHERE x IS NOT NULL");
    }

    #[test]
    fn test_where_between() {
        let plan = assert_parses(
            "SELECT * FROM t WHERE x BETWEEN 1 AND 10",
        );
        assert!(matches!(plan, RelExpr::Filter { .. }));
    }

    #[test]
    fn test_where_not_between() {
        assert_parses(
            "SELECT * FROM t WHERE x NOT BETWEEN 1 AND 10",
        );
    }

    #[test]
    fn test_where_in_list() {
        assert_parses(
            "SELECT * FROM t WHERE x IN (1, 2, 3)",
        );
    }

    #[test]
    fn test_where_not_in_list() {
        assert_parses(
            "SELECT * FROM t WHERE x NOT IN (1, 2, 3)",
        );
    }

    // === JOINs ===

    #[test]
    fn test_inner_join() {
        assert_parses(
            "SELECT * FROM orders o \
             JOIN customers c ON o.customer_id = c.id",
        );
    }

    #[test]
    fn test_left_outer_join() {
        let plan = assert_parses(
            "SELECT * FROM a LEFT JOIN b ON a.id = b.id",
        );
        if let RelExpr::Join { join_type, .. } = &plan {
            assert_eq!(*join_type, JoinType::LeftOuter);
        } else {
            panic!("expected Join");
        }
    }

    #[test]
    fn test_right_outer_join() {
        let plan = assert_parses(
            "SELECT * FROM a RIGHT JOIN b ON a.id = b.id",
        );
        if let RelExpr::Join { join_type, .. } = &plan {
            assert_eq!(*join_type, JoinType::RightOuter);
        } else {
            panic!("expected Join");
        }
    }

    #[test]
    fn test_full_outer_join() {
        let plan = assert_parses(
            "SELECT * FROM a FULL OUTER JOIN b ON a.id = b.id",
        );
        if let RelExpr::Join { join_type, .. } = &plan {
            assert_eq!(*join_type, JoinType::FullOuter);
        } else {
            panic!("expected Join");
        }
    }

    #[test]
    fn test_cross_join() {
        let plan = assert_parses(
            "SELECT * FROM a CROSS JOIN b",
        );
        if let RelExpr::Join { join_type, .. } = &plan {
            assert_eq!(*join_type, JoinType::Cross);
        } else {
            panic!("expected Join");
        }
    }

    #[test]
    fn test_multiple_joins() {
        assert_parses(
            "SELECT * FROM a \
             JOIN b ON a.id = b.a_id \
             JOIN c ON b.id = c.b_id",
        );
    }

    #[test]
    fn test_join_using() {
        assert_parses(
            "SELECT * FROM a JOIN b USING (id)",
        );
    }

    #[test]
    fn test_implicit_cross_join() {
        let plan = assert_parses(
            "SELECT * FROM a, b",
        );
        assert!(matches!(plan, RelExpr::Join { .. }));
    }

    // === Aggregates ===

    #[test]
    fn test_basic_aggregate() {
        assert_parses(
            "SELECT region, COUNT(*), SUM(amount) \
             FROM orders GROUP BY region",
        );
    }

    #[test]
    fn test_aggregate_without_group_by() {
        assert_parses(
            "SELECT COUNT(*) FROM users",
        );
    }

    #[test]
    fn test_aggregate_distinct() {
        assert_parses(
            "SELECT COUNT(DISTINCT name) FROM users",
        );
    }

    #[test]
    fn test_extended_aggregates() {
        assert_parses(
            "SELECT STDDEV_POP(price), VAR_SAMP(quantity) \
             FROM products",
        );
    }

    #[test]
    fn test_having() {
        let plan = assert_parses(
            "SELECT dept, COUNT(*) \
             FROM emp GROUP BY dept HAVING COUNT(*) > 5",
        );
        // Structure: Project(Filter(Aggregate(...)))
        // The non-aggregate column `dept` creates a Project
        if let RelExpr::Project { input, .. } = &plan {
            if let RelExpr::Filter { input, .. } = input.as_ref()
            {
                assert!(matches!(
                    input.as_ref(),
                    RelExpr::Aggregate { .. }
                ));
            } else {
                panic!("expected Filter inside Project");
            }
        } else {
            panic!("expected Project(Filter(Aggregate))");
        }
    }

    #[test]
    fn test_having_complex() {
        assert_parses(
            "SELECT dept, AVG(salary) AS avg_sal \
             FROM emp GROUP BY dept \
             HAVING AVG(salary) > 50000 AND COUNT(*) > 3",
        );
    }

    // === DISTINCT ===

    #[test]
    fn test_distinct() {
        let plan = assert_parses(
            "SELECT DISTINCT name FROM users",
        );
        assert!(matches!(plan, RelExpr::Distinct { .. }));
    }

    #[test]
    fn test_distinct_star() {
        let plan = assert_parses(
            "SELECT DISTINCT * FROM users",
        );
        assert!(matches!(plan, RelExpr::Distinct { .. }));
    }

    // === ORDER BY ===

    #[test]
    fn test_order_by() {
        let plan = assert_parses(
            "SELECT * FROM users ORDER BY name",
        );
        assert!(matches!(plan, RelExpr::Sort { .. }));
    }

    #[test]
    fn test_order_by_desc() {
        let plan = assert_parses(
            "SELECT * FROM users ORDER BY age DESC",
        );
        if let RelExpr::Sort { keys, .. } = &plan {
            assert_eq!(keys[0].direction, SortDirection::Desc);
        } else {
            panic!("expected Sort");
        }
    }

    #[test]
    fn test_order_by_nulls_first() {
        let plan = assert_parses(
            "SELECT * FROM t ORDER BY x ASC NULLS FIRST",
        );
        if let RelExpr::Sort { keys, .. } = &plan {
            assert_eq!(keys[0].nulls, NullOrdering::First);
        } else {
            panic!("expected Sort");
        }
    }

    #[test]
    fn test_order_by_multiple() {
        let plan = assert_parses(
            "SELECT * FROM t ORDER BY a ASC, b DESC",
        );
        if let RelExpr::Sort { keys, .. } = &plan {
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0].direction, SortDirection::Asc);
            assert_eq!(keys[1].direction, SortDirection::Desc);
        } else {
            panic!("expected Sort");
        }
    }

    // === LIMIT / OFFSET ===

    #[test]
    fn test_limit() {
        let plan = assert_parses(
            "SELECT * FROM users LIMIT 10",
        );
        if let RelExpr::Limit {
            count, offset, ..
        } = &plan
        {
            assert_eq!(*count, 10);
            assert_eq!(*offset, 0);
        } else {
            panic!("expected Limit");
        }
    }

    #[test]
    fn test_limit_offset() {
        let plan = assert_parses(
            "SELECT * FROM users LIMIT 10 OFFSET 20",
        );
        if let RelExpr::Limit {
            count, offset, ..
        } = &plan
        {
            assert_eq!(*count, 10);
            assert_eq!(*offset, 20);
        } else {
            panic!("expected Limit");
        }
    }

    #[test]
    fn test_offset_only() {
        let plan = assert_parses(
            "SELECT * FROM users OFFSET 5",
        );
        if let RelExpr::Limit {
            count, offset, ..
        } = &plan
        {
            assert_eq!(*count, u64::MAX);
            assert_eq!(*offset, 5);
        } else {
            panic!("expected Limit");
        }
    }

    // === CTE / WITH ===

    #[test]
    fn test_simple_cte() {
        let plan = assert_parses(
            "WITH recent AS (SELECT * FROM orders) \
             SELECT * FROM recent",
        );
        if let RelExpr::Cte { name, .. } = &plan {
            assert_eq!(name, "recent");
        } else {
            panic!("expected Cte");
        }
    }

    #[test]
    fn test_multiple_ctes() {
        let plan = assert_parses(
            "WITH a AS (SELECT * FROM t1), \
                  b AS (SELECT * FROM t2) \
             SELECT * FROM a JOIN b ON a.id = b.id",
        );
        // Should be nested: Cte(a, Cte(b, ...))
        if let RelExpr::Cte {
            name, body, ..
        } = &plan
        {
            assert_eq!(name, "a");
            assert!(
                matches!(body.as_ref(), RelExpr::Cte { name, .. } if name == "b")
            );
        } else {
            panic!("expected nested CTEs");
        }
    }

    #[test]
    fn test_cte_with_filter() {
        assert_parses(
            "WITH active AS (\
                 SELECT * FROM users WHERE status = 'active'\
             ) SELECT name FROM active WHERE age > 21",
        );
    }

    // === Subqueries (derived tables) ===

    #[test]
    fn test_subquery_in_from() {
        assert_parses(
            "SELECT * FROM (SELECT * FROM users) AS t",
        );
    }

    #[test]
    fn test_subquery_with_filter() {
        assert_parses(
            "SELECT * FROM (\
                 SELECT name, age FROM users WHERE age > 18\
             ) AS adults WHERE name = 'Alice'",
        );
    }

    // === Set operations ===

    #[test]
    fn test_union() {
        let plan = assert_parses(
            "SELECT * FROM a UNION SELECT * FROM b",
        );
        if let RelExpr::Union { all, .. } = &plan {
            assert!(!*all);
        } else {
            panic!("expected Union");
        }
    }

    #[test]
    fn test_union_all() {
        let plan = assert_parses(
            "SELECT * FROM a UNION ALL SELECT * FROM b",
        );
        if let RelExpr::Union { all, .. } = &plan {
            assert!(*all);
        } else {
            panic!("expected Union ALL");
        }
    }

    #[test]
    fn test_intersect() {
        let plan = assert_parses(
            "SELECT * FROM a INTERSECT SELECT * FROM b",
        );
        assert!(matches!(plan, RelExpr::Intersect { .. }));
    }

    #[test]
    fn test_except() {
        let plan = assert_parses(
            "SELECT * FROM a EXCEPT SELECT * FROM b",
        );
        assert!(matches!(plan, RelExpr::Except { .. }));
    }

    // === VALUES ===

    #[test]
    fn test_values() {
        let plan = assert_parses(
            "VALUES (1, 'a'), (2, 'b')",
        );
        if let RelExpr::Values { rows } = &plan {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].len(), 2);
        } else {
            panic!("expected Values");
        }
    }

    // === CASE expression ===

    #[test]
    fn test_case_expression() {
        assert_parses(
            "SELECT CASE WHEN x > 0 THEN 'pos' \
             ELSE 'non-pos' END FROM t",
        );
    }

    // === CAST expression ===

    #[test]
    fn test_cast() {
        assert_parses(
            "SELECT CAST(x AS INTEGER) FROM t",
        );
    }

    // === Complex queries ===

    #[test]
    fn test_order_by_limit_combo() {
        let plan = assert_parses(
            "SELECT * FROM users ORDER BY age DESC LIMIT 5",
        );
        // Limit is outermost, then Sort
        if let RelExpr::Limit { input, count, .. } = &plan {
            assert_eq!(*count, 5);
            assert!(
                matches!(input.as_ref(), RelExpr::Sort { .. })
            );
        } else {
            panic!("expected Limit(Sort(...))");
        }
    }

    #[test]
    fn test_full_query() {
        assert_parses(
            "WITH top_depts AS (\
                 SELECT dept, COUNT(*) AS cnt \
                 FROM emp \
                 GROUP BY dept \
                 HAVING COUNT(*) > 10\
             ) \
             SELECT DISTINCT d.dept, d.cnt \
             FROM top_depts d \
             ORDER BY d.cnt DESC \
             LIMIT 5",
        );
    }

    #[test]
    fn test_tpch_q1_style() {
        assert_parses(
            "SELECT l_returnflag, l_linestatus, \
                    SUM(l_quantity), \
                    SUM(l_extendedprice), \
                    AVG(l_quantity), \
                    AVG(l_extendedprice), \
                    COUNT(*) \
             FROM lineitem \
             WHERE l_shipdate <= '1998-09-02' \
             GROUP BY l_returnflag, l_linestatus \
             ORDER BY l_returnflag, l_linestatus",
        );
    }

    #[test]
    fn test_tpch_q3_style() {
        assert_parses(
            "SELECT l_orderkey, SUM(l_extendedprice) AS revenue \
             FROM customer \
             JOIN orders ON c_custkey = o_custkey \
             JOIN lineitem ON l_orderkey = o_orderkey \
             WHERE c_mktsegment = 'BUILDING' \
               AND o_orderdate < '1995-03-15' \
             GROUP BY l_orderkey \
             ORDER BY revenue DESC \
             LIMIT 10",
        );
    }

    // === Error cases ===

    #[test]
    fn test_empty_sql() {
        assert_fails("");
    }

    #[test]
    fn test_invalid_sql() {
        assert_fails("NOT A VALID SQL");
    }

    #[test]
    fn test_multiple_statements() {
        assert_fails(
            "SELECT * FROM a; SELECT * FROM b",
        );
    }

    #[test]
    fn test_insert_not_supported() {
        assert_fails(
            "INSERT INTO t VALUES (1, 2)",
        );
    }

    // === Function calls ===

    #[test]
    fn test_function_in_projection() {
        assert_parses(
            "SELECT UPPER(name) FROM users",
        );
    }

    #[test]
    fn test_nested_function() {
        assert_parses(
            "SELECT COALESCE(name, 'unknown') FROM users",
        );
    }
}
