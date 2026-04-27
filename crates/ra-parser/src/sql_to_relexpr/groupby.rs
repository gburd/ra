use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
use ra_core::expr::Expr;
use sqlparser::ast::{Expr as SqlExpr, FunctionArguments, GroupByExpr, Select, SelectItem};

use super::error::SqlConversionError;
use super::expr::convert_expr;
use super::helpers::extract_single_func_arg;
use super::operators::convert_binary_op;

pub fn convert_group_by(input: RelExpr, select: &Select) -> Result<RelExpr, SqlConversionError> {
    let group_by_exprs = match &select.group_by {
        GroupByExpr::All(_) => vec![],
        GroupByExpr::Expressions(exprs, _) => exprs.clone(),
    };

    let group_by: Result<Vec<_>, _> = group_by_exprs.iter().map(convert_expr).collect();
    let group_by = group_by?;

    let mut aggregates = Vec::new();
    for item in &select.projection {
        if let SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } = item {
            if let Some(agg) = try_extract_aggregate(expr, item)? {
                aggregates.push(agg);
            }
        }
    }

    if let Some(ref having) = select.having {
        extract_having_aggregates(having, &mut aggregates)?;
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

fn extract_having_aggregates(
    expr: &SqlExpr,
    aggregates: &mut Vec<AggregateExpr>,
) -> Result<(), SqlConversionError> {
    match expr {
        SqlExpr::Function(func) => {
            let func_name = func.name.to_string().to_uppercase();
            if let Some(agg_func) = parse_agg_function(&func_name) {
                let arg = extract_single_func_arg(&func.args)?;
                let distinct = match &func.args {
                    FunctionArguments::List(list) => list.duplicate_treatment.is_some(),
                    _ => false,
                };
                let already_exists = aggregates
                    .iter()
                    .any(|a| a.function == agg_func && a.arg == arg);
                if !already_exists {
                    aggregates.push(AggregateExpr {
                        function: agg_func,
                        arg,
                        distinct,
                        alias: None,
                    });
                }
            }
        }
        SqlExpr::BinaryOp { left, right, .. } => {
            extract_having_aggregates(left, aggregates)?;
            extract_having_aggregates(right, aggregates)?;
        }
        SqlExpr::UnaryOp { expr, .. } => {
            extract_having_aggregates(expr, aggregates)?;
        }
        SqlExpr::Nested(inner) => {
            extract_having_aggregates(inner, aggregates)?;
        }
        _ => {}
    }
    Ok(())
}

fn try_extract_aggregate(
    expr: &SqlExpr,
    item: &SelectItem,
) -> Result<Option<AggregateExpr>, SqlConversionError> {
    match expr {
        SqlExpr::Function(func) if func.over.is_none() => {
            let func_name = func.name.to_string().to_uppercase();
            let agg_func = match parse_agg_function(&func_name) {
                Some(f) => f,
                None => return Ok(None),
            };

            let arg = extract_single_func_arg(&func.args)?;

            let alias = match item {
                SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.clone()),
                _ => None,
            };

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

pub fn parse_agg_function(name: &str) -> Option<AggregateFunction> {
    match name {
        "COUNT" => Some(AggregateFunction::Count),
        "SUM" => Some(AggregateFunction::Sum),
        "AVG" => Some(AggregateFunction::Avg),
        "MIN" => Some(AggregateFunction::Min),
        "MAX" => Some(AggregateFunction::Max),
        "STDDEV" | "STDDEV_POP" | "STDDEV_SAMP" => Some(AggregateFunction::StdDev),
        "VARIANCE" | "VAR_POP" | "VAR_SAMP" => Some(AggregateFunction::Variance),
        "STRING_AGG" | "GROUP_CONCAT" => Some(AggregateFunction::StringAgg),
        "ARRAY_AGG" => Some(AggregateFunction::ArrayAgg),
        _ => None,
    }
}

pub fn is_aggregate(expr: &SqlExpr) -> bool {
    match expr {
        SqlExpr::Function(func) if func.over.is_none() => {
            let name = func.name.to_string().to_uppercase();
            parse_agg_function(&name).is_some()
        }
        _ => false,
    }
}

pub fn convert_having_expr(expr: &SqlExpr) -> Result<Expr, SqlConversionError> {
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
            let arg = extract_single_func_arg(&func.args)?;
            let args = match arg {
                Some(a) => vec![a],
                None => vec![],
            };
            Ok(Expr::Function {
                name: func_name,
                args,
            })
        }
        SqlExpr::Nested(inner) => convert_having_expr(inner),
        _ => convert_expr(expr),
    }
}
