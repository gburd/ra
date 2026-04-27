use ra_core::algebra::{JoinType, NullOrdering, ProjectionColumn, RelExpr, SortDirection, SortKey};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use sqlparser::ast::{
    BinaryOperator, Distinct as SqlDistinct, Expr as SqlExpr, FunctionArg, GroupByExpr,
    Join as SqlJoin, JoinConstraint, JoinOperator, ObjectName, OrderByExpr as SqlOrderByExpr,
    SelectItem, TableFactor, TableWithJoins, Value,
};

use super::error::SqlConversionError;
use super::expr::convert_expr;
use super::groupby::{convert_group_by, convert_having_expr, is_aggregate};
use super::helpers::{
    convert_function_arg, extract_func_args, extract_u64_from_expr, object_name_to_string,
    pad_aliases,
};
use super::query::convert_query;
use super::window::{extract_window_functions, is_window_function};

pub fn convert_select(select: &sqlparser::ast::Select) -> Result<RelExpr, SqlConversionError> {
    if !select.cluster_by.is_empty()
        || !select.distribute_by.is_empty()
        || !select.sort_by.is_empty()
    {
        return Err(SqlConversionError::UnsupportedFeature(
            "CLUSTER BY/DISTRIBUTE BY/SORT BY not supported".to_owned(),
        ));
    }

    let mut plan = convert_from(&select.from)?;

    if let Some(ref where_expr) = select.selection {
        if let Some((vector_expr, query_vector, metric, threshold)) =
            extract_vector_filter(where_expr)?
        {
            plan = RelExpr::VectorFilter {
                vector_expr,
                query_vector,
                metric,
                threshold,
                input: Box::new(plan),
            };
        } else {
            let predicate = convert_expr(where_expr)?;
            plan = RelExpr::Filter {
                predicate,
                input: Box::new(plan),
            };
        }
    }

    let has_group_by = !matches!(
        select.group_by,
        GroupByExpr::All(ref exprs) if exprs.is_empty()
    );
    let has_aggregates = select.projection.iter().any(|item| match item {
        SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
            is_aggregate(expr)
        }
        _ => false,
    });
    let has_having = select.having.is_some();

    if has_group_by || has_aggregates || has_having {
        plan = convert_group_by(plan, select)?;
    }

    if let Some(ref having_expr) = select.having {
        let predicate = convert_having_expr(having_expr)?;
        plan = RelExpr::Filter {
            predicate,
            input: Box::new(plan),
        };
    }

    let window_fns = extract_window_functions(&select.projection)?;
    if !window_fns.is_empty() {
        plan = RelExpr::Window {
            functions: window_fns,
            input: Box::new(plan),
        };
    }

    plan = convert_projection(plan, &select.projection)?;

    if let Some(ref distinct) = select.distinct {
        match distinct {
            SqlDistinct::Distinct | SqlDistinct::On(_) => {
                plan = RelExpr::Distinct {
                    input: Box::new(plan),
                };
            }
        }
    }

    Ok(plan)
}

fn convert_from(from: &[TableWithJoins]) -> Result<RelExpr, SqlConversionError> {
    if from.is_empty() {
        return Ok(RelExpr::Values { rows: vec![vec![]] });
    }

    let mut plan = convert_table_with_joins(&from[0])?;
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

fn convert_table_with_joins(twj: &TableWithJoins) -> Result<RelExpr, SqlConversionError> {
    let mut plan = convert_table_factor(&twj.relation)?;

    for join in &twj.joins {
        plan = convert_join(plan, join)?;
    }

    Ok(plan)
}

fn convert_table_factor(table: &TableFactor) -> Result<RelExpr, SqlConversionError> {
    match table {
        TableFactor::Table {
            name, alias, args, ..
        } => {
            if let Some(table_args) = args {
                let func_args: Vec<FunctionArg> = table_args.args.iter().cloned().collect();
                return convert_table_valued_function(name, &func_args, alias);
            }
            let table_name = object_name_to_string(name);
            let alias_name = alias.as_ref().map(|a| a.name.value.clone());

            Ok(RelExpr::Scan {
                table: table_name,
                alias: alias_name,
            })
        }
        TableFactor::Derived {
            subquery, alias, ..
        } => {
            let mut sub_plan = convert_query(subquery)?;
            if let Some(alias) = alias {
                sub_plan = RelExpr::Project {
                    columns: vec![ProjectionColumn {
                        expr: Expr::Column(ColumnRef::new("*")),
                        alias: Some(alias.name.value.clone()),
                    }],
                    input: Box::new(sub_plan),
                };
            }
            Ok(sub_plan)
        }
        TableFactor::TableFunction { .. } => Err(SqlConversionError::UnsupportedFeature(
            "TABLE() syntax not supported".to_owned(),
        )),
        TableFactor::UNNEST {
            alias,
            array_exprs,
            with_ordinality,
            ..
        } => {
            if array_exprs.is_empty() {
                return Err(SqlConversionError::InvalidSql(
                    "UNNEST requires at least one argument".to_owned(),
                ));
            }
            if array_exprs.len() == 1 {
                let arr_expr = convert_expr(&array_exprs[0])?;
                let alias_name = alias.as_ref().map(|a| a.name.value.clone());
                Ok(RelExpr::Unnest {
                    expr: arr_expr,
                    alias: alias_name,
                    input: None,
                    with_ordinality: *with_ordinality,
                })
            } else {
                let exprs: Result<Vec<_>, _> = array_exprs.iter().map(convert_expr).collect();
                let col_aliases = alias
                    .as_ref()
                    .map(|a| {
                        a.columns
                            .iter()
                            .map(|c| Some(c.value.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let aliases = pad_aliases(col_aliases, array_exprs.len());
                Ok(RelExpr::MultiUnnest {
                    exprs: exprs?,
                    aliases,
                    with_ordinality: *with_ordinality,
                })
            }
        }
        TableFactor::Function {
            name, args, alias, ..
        } => convert_table_valued_function(name, args, alias),
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported table factor".to_owned(),
        )),
    }
}

fn convert_table_valued_function(
    name: &ObjectName,
    args: &[FunctionArg],
    alias: &Option<sqlparser::ast::TableAlias>,
) -> Result<RelExpr, SqlConversionError> {
    let func_name = object_name_to_string(name);
    let lower = func_name.to_lowercase();

    match lower.as_str() {
        "unnest" => {
            if args.is_empty() {
                return Err(SqlConversionError::InvalidSql(
                    "UNNEST requires at least one argument".to_owned(),
                ));
            }
            if args.len() == 1 {
                let arr_expr = convert_function_arg(&args[0])?;
                let alias_name = alias.as_ref().map(|a| a.name.value.clone());
                Ok(RelExpr::Unnest {
                    expr: arr_expr,
                    alias: alias_name,
                    input: None,
                    with_ordinality: false,
                })
            } else {
                let exprs: Result<Vec<_>, _> = args.iter().map(convert_function_arg).collect();
                let col_aliases = alias
                    .as_ref()
                    .map(|a| {
                        a.columns
                            .iter()
                            .map(|c| Some(c.value.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let aliases = pad_aliases(col_aliases, args.len());
                Ok(RelExpr::MultiUnnest {
                    exprs: exprs?,
                    aliases,
                    with_ordinality: false,
                })
            }
        }
        "generate_series" => {
            let arg_exprs: Result<Vec<_>, _> = args.iter().map(convert_function_arg).collect();
            Ok(RelExpr::TableFunction {
                name: "generate_series".to_owned(),
                args: arg_exprs?,
                columns: vec![("generate_series".to_owned(), "Int64".to_owned())],
                input: None,
            })
        }
        _ => {
            let arg_exprs: Result<Vec<_>, _> = args.iter().map(convert_function_arg).collect();
            Ok(RelExpr::TableFunction {
                name: func_name,
                args: arg_exprs?,
                columns: vec![],
                input: None,
            })
        }
    }
}

fn convert_join(left: RelExpr, join: &SqlJoin) -> Result<RelExpr, SqlConversionError> {
    let right = convert_table_factor(&join.relation)?;

    let (join_type, constraint) = match &join.join_operator {
        JoinOperator::Inner(c) => (JoinType::Inner, c),
        JoinOperator::LeftOuter(c) => (JoinType::LeftOuter, c),
        JoinOperator::RightOuter(c) => (JoinType::RightOuter, c),
        JoinOperator::FullOuter(c) => (JoinType::FullOuter, c),
        JoinOperator::CrossJoin => {
            return Ok(RelExpr::Join {
                join_type: JoinType::Cross,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        JoinOperator::LeftSemi(c) => (JoinType::Semi, c),
        JoinOperator::LeftAnti(c) => (JoinType::Anti, c),
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(
                "unsupported join type".to_owned(),
            ))
        }
    };

    let condition = match constraint {
        JoinConstraint::On(expr) => convert_expr(expr)?,
        JoinConstraint::Using(cols) => build_using_condition(cols)?,
        JoinConstraint::Natural => {
            return Err(SqlConversionError::UnsupportedFeature(
                "NATURAL JOIN not supported".to_owned(),
            ))
        }
        JoinConstraint::None => Expr::Const(Const::Bool(true)),
    };

    Ok(RelExpr::Join {
        join_type,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn build_using_condition(cols: &[sqlparser::ast::Ident]) -> Result<Expr, SqlConversionError> {
    if cols.is_empty() {
        return Ok(Expr::Const(Const::Bool(true)));
    }

    let mut conditions: Vec<Expr> = Vec::new();
    for col in cols {
        let col_name = &col.value;
        conditions.push(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(col_name))),
            right: Box::new(Expr::Column(ColumnRef::new(col_name))),
        });
    }

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
                if !is_aggregate(expr) && !is_window_function(expr) {
                    columns.push(ProjectionColumn {
                        expr: convert_expr(expr)?,
                        alias: None,
                    });
                }
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                if !is_aggregate(expr) && !is_window_function(expr) {
                    columns.push(ProjectionColumn {
                        expr: convert_expr(expr)?,
                        alias: Some(alias.value.clone()),
                    });
                }
            }
            SelectItem::QualifiedWildcard(obj_name, _) => {
                let table = object_name_to_string(obj_name);
                columns.push(ProjectionColumn {
                    expr: Expr::Column(ColumnRef::qualified(&table, "*")),
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

pub fn convert_order_by_exprs(
    exprs: &[SqlOrderByExpr],
) -> Result<Vec<SortKey>, SqlConversionError> {
    let mut keys = Vec::new();
    for ob in exprs {
        let expr = convert_expr(&ob.expr)?;
        let direction = match ob.asc {
            Some(true) | None => SortDirection::Asc,
            Some(false) => SortDirection::Desc,
        };
        let nulls = match ob.nulls_first {
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

/// Try to convert ORDER BY + LIMIT into a TopK vector search.
pub fn try_convert_topk(
    order_exprs: &[SqlOrderByExpr],
    limit: Option<&SqlExpr>,
    input: &RelExpr,
) -> Result<Option<RelExpr>, SqlConversionError> {
    if order_exprs.len() != 1 {
        return Ok(None);
    }

    let order_by = &order_exprs[0];

    if order_by.asc == Some(false) {
        return Ok(None);
    }

    let Some(limit_expr) = limit else {
        return Ok(None);
    };
    let k = extract_u64_from_expr(limit_expr)?;

    if let Some((vector_expr, query_vector, metric)) = extract_vector_distance(&order_by.expr)? {
        return Ok(Some(RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k,
            input: Box::new(input.clone()),
        }));
    }

    Ok(None)
}

/// Extract vector distance components from an expression.
fn extract_vector_distance(
    expr: &SqlExpr,
) -> Result<Option<(Expr, Expr, ra_core::search_types::DistanceMetric)>, SqlConversionError> {
    use ra_core::search_types::DistanceMetric;

    match expr {
        SqlExpr::BinaryOp { left, op, right } => {
            let op_str = format!("{op:?}");
            let metric = match op_str.as_str() {
                "Custom(\"<->\", None)" => Some(DistanceMetric::L2),
                "Custom(\"<#>\", None)" => Some(DistanceMetric::InnerProduct),
                "Custom(\"<=>\", None)" => Some(DistanceMetric::Cosine),
                _ => None,
            };

            if let Some(metric) = metric {
                let vector_expr = convert_expr(left)?;
                let query_vector = convert_expr(right)?;
                return Ok(Some((vector_expr, query_vector, metric)));
            }
            Ok(None)
        }
        SqlExpr::Function(func) => {
            let name = func.name.to_string().to_uppercase();
            let metric = match name.as_str() {
                "VEC_DISTANCE_L2" => Some(DistanceMetric::L2),
                "VEC_DISTANCE_COSINE" => Some(DistanceMetric::Cosine),
                "VEC_DISTANCE_L1" => Some(DistanceMetric::L1),
                "L2_DISTANCE" => Some(DistanceMetric::L2),
                "COSINE_DISTANCE" => Some(DistanceMetric::Cosine),
                _ => None,
            };

            if let Some(metric) = metric {
                let args = extract_func_args(&func.args)?;
                if args.len() == 2 {
                    return Ok(Some((args[0].clone(), args[1].clone(), metric)));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// Check if a WHERE predicate contains a vector filter.
fn extract_vector_filter(
    expr: &SqlExpr,
) -> Result<Option<(Expr, Expr, ra_core::search_types::DistanceMetric, f64)>, SqlConversionError> {
    match expr {
        SqlExpr::BinaryOp { left, op, right } => {
            if matches!(op, BinaryOperator::Lt | BinaryOperator::LtEq) {
                if let Some((vector_expr, query_vector, metric)) = extract_vector_distance(left)? {
                    if let SqlExpr::Value(Value::Number(n, _)) = right.as_ref() {
                        use std::str::FromStr;
                        let n_str = n.to_string();
                        if let Ok(threshold) = f64::from_str(&n_str) {
                            return Ok(Some((vector_expr, query_vector, metric, threshold)));
                        }
                    }
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}
