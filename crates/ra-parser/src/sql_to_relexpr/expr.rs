use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use sqlparser::ast::{Expr as SqlExpr, FunctionArg, FunctionArgExpr, FunctionArguments};

use super::error::SqlConversionError;
use super::helpers::extract_single_func_arg;
use super::operators::{convert_binary_op, convert_unary_op, convert_value};

pub fn convert_expr(expr: &SqlExpr) -> Result<Expr, SqlConversionError> {
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
        SqlExpr::BinaryOp { left, op, right } => convert_binary_expr(left, op, right),
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
        SqlExpr::Function(func) => convert_function_expr(func),
        SqlExpr::Subquery(query) => {
            let _plan = super::query::convert_query(query)?;
            Ok(Expr::Function {
                name: "SUBQUERY".to_owned(),
                args: vec![],
            })
        }
        SqlExpr::InSubquery { expr, subquery, .. } => {
            let left = convert_expr(expr)?;
            let _plan = super::query::convert_query(subquery)?;
            Ok(Expr::Function {
                name: "IN_SUBQUERY".to_owned(),
                args: vec![left],
            })
        }
        SqlExpr::Exists { subquery, .. } => {
            let _plan = super::query::convert_query(subquery)?;
            Ok(Expr::Function {
                name: "EXISTS".to_owned(),
                args: vec![],
            })
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let left = convert_expr(expr)?;
            let list_exprs: Result<Vec<_>, _> = list.iter().map(convert_expr).collect();
            let mut args = vec![left];
            args.extend(list_exprs?);
            let func = if *negated { "NOT_IN_LIST" } else { "IN_LIST" };
            Ok(Expr::Function {
                name: func.to_owned(),
                args,
            })
        }
        SqlExpr::Like {
            expr,
            pattern,
            negated,
            ..
        } => {
            let left = convert_expr(expr)?;
            let right = convert_expr(pattern)?;
            let like_expr = Expr::Function {
                name: "LIKE".to_owned(),
                args: vec![left, right],
            };
            if *negated {
                Ok(Expr::UnaryOp {
                    op: ra_core::expr::UnaryOp::Not,
                    operand: Box::new(like_expr),
                })
            } else {
                Ok(like_expr)
            }
        }
        SqlExpr::ILike {
            expr,
            pattern,
            negated,
            ..
        } => {
            let left = convert_expr(expr)?;
            let right = convert_expr(pattern)?;
            let ilike_expr = Expr::Function {
                name: "ILIKE".to_owned(),
                args: vec![left, right],
            };
            if *negated {
                Ok(Expr::UnaryOp {
                    op: ra_core::expr::UnaryOp::Not,
                    operand: Box::new(ilike_expr),
                })
            } else {
                Ok(ilike_expr)
            }
        }
        SqlExpr::Interval(interval) => {
            let val = convert_expr(&interval.value)?;
            Ok(Expr::Function {
                name: "INTERVAL".to_owned(),
                args: vec![val],
            })
        }
        SqlExpr::TypedString { data_type, value } => Ok(Expr::Cast {
            expr: Box::new(Expr::Const(Const::String(value.clone()))),
            target_type: data_type.to_string(),
        }),
        SqlExpr::Between {
            expr, low, high, ..
        } => {
            let col = convert_expr(expr)?;
            let low_expr = convert_expr(low)?;
            let high_expr = convert_expr(high)?;
            Ok(Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::BinOp {
                    op: BinOp::Ge,
                    left: Box::new(col.clone()),
                    right: Box::new(low_expr),
                }),
                right: Box::new(Expr::BinOp {
                    op: BinOp::Le,
                    left: Box::new(col),
                    right: Box::new(high_expr),
                }),
            })
        }
        SqlExpr::Cast {
            expr, data_type, ..
        } => Ok(Expr::Cast {
            expr: Box::new(convert_expr(expr)?),
            target_type: data_type.to_string(),
        }),
        SqlExpr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => {
            let op = match operand {
                Some(e) => Some(Box::new(convert_expr(e)?)),
                None => None,
            };
            let mut when_clauses = Vec::new();
            for (cond, result) in conditions.iter().zip(results.iter()) {
                when_clauses.push((convert_expr(cond)?, convert_expr(result)?));
            }
            let else_r = match else_result {
                Some(e) => Some(Box::new(convert_expr(e)?)),
                None => None,
            };
            Ok(Expr::Case {
                operand: op,
                when_clauses,
                else_result: else_r,
            })
        }
        SqlExpr::Extract { field, expr, .. } => {
            let arg = convert_expr(expr)?;
            Ok(Expr::Function {
                name: format!("EXTRACT_{field}"),
                args: vec![arg],
            })
        }
        SqlExpr::Trim {
            expr, trim_what, ..
        } => {
            let arg = convert_expr(expr)?;
            let mut args = vec![arg];
            if let Some(what) = trim_what {
                args.push(convert_expr(what)?);
            }
            Ok(Expr::Function {
                name: "TRIM".to_owned(),
                args,
            })
        }
        SqlExpr::Array(arr) => {
            let elems: Result<Vec<_>, _> = arr.elem.iter().map(convert_expr).collect();
            Ok(Expr::Array(elems?))
        }
        SqlExpr::AnyOp { .. } | SqlExpr::AllOp { .. } => Ok(Expr::Function {
            name: "ANY_ALL".to_owned(),
            args: vec![],
        }),
        SqlExpr::Position { expr, r#in, .. } => {
            let needle = convert_expr(expr)?;
            let haystack = convert_expr(r#in)?;
            Ok(Expr::Function {
                name: "POSITION".to_owned(),
                args: vec![needle, haystack],
            })
        }
        SqlExpr::Substring {
            expr,
            substring_from,
            substring_for,
            ..
        } => {
            let arg = convert_expr(expr)?;
            let mut args = vec![arg];
            if let Some(from) = substring_from {
                args.push(convert_expr(from)?);
            }
            if let Some(len) = substring_for {
                args.push(convert_expr(len)?);
            }
            Ok(Expr::Function {
                name: "SUBSTRING".to_owned(),
                args,
            })
        }
        SqlExpr::Subscript { expr, subscript } => {
            let array_expr = convert_expr(expr)?;
            match subscript.as_ref() {
                sqlparser::ast::Subscript::Index { index } => {
                    let index_expr = convert_expr(index)?;
                    Ok(Expr::ArrayIndex(Box::new(array_expr), Box::new(index_expr)))
                }
                sqlparser::ast::Subscript::Slice {
                    lower_bound,
                    upper_bound,
                    ..
                } => {
                    let start = lower_bound
                        .as_ref()
                        .map(|e| convert_expr(e).map(Box::new))
                        .transpose()?;
                    let end = upper_bound
                        .as_ref()
                        .map(|e| convert_expr(e).map(Box::new))
                        .transpose()?;
                    Ok(Expr::ArraySlice {
                        array: Box::new(array_expr),
                        start,
                        end,
                    })
                }
            }
        }
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "expression type not yet supported: {expr}"
        ))),
    }
}

fn convert_binary_expr(
    left: &SqlExpr,
    op: &sqlparser::ast::BinaryOperator,
    right: &SqlExpr,
) -> Result<Expr, SqlConversionError> {
    use ra_core::search_types::DistanceMetric;
    let op_str = format!("{op:?}");

    if std::env::var("DEBUG_RA").is_ok() {
        eprintln!("DEBUG: BinaryOp operator: {op_str}");
    }

    let vector_metric = if op_str.contains("<->") {
        Some(DistanceMetric::L2)
    } else if op_str.contains("<#>") {
        Some(DistanceMetric::InnerProduct)
    } else if op_str.contains("<=>") {
        Some(DistanceMetric::Cosine)
    } else {
        None
    };

    if let Some(metric) = vector_metric {
        if std::env::var("DEBUG_RA").is_ok() {
            eprintln!("DEBUG: Converting to VectorDistance with metric: {metric:?}");
        }
        let column = convert_expr(left).map_err(|e| {
            if std::env::var("DEBUG_RA").is_ok() {
                eprintln!("DEBUG: Failed to convert left side: {e:?}");
            }
            e
        })?;
        let target = convert_expr(right).map_err(|e| {
            if std::env::var("DEBUG_RA").is_ok() {
                eprintln!("DEBUG: Failed to convert right side: {e:?}");
            }
            e
        })?;

        if std::env::var("DEBUG_RA").is_ok() {
            eprintln!("DEBUG: VectorDistance created successfully");
            eprintln!("  column: {column:?}");
            eprintln!("  target: {target:?}");
            eprintln!("  metric: {}", format!("{metric:?}").to_lowercase());
        }

        return Ok(Expr::VectorDistance {
            metric: format!("{metric:?}").to_lowercase(),
            column: Box::new(column),
            target: Box::new(target),
        });
    }

    match convert_binary_op(op) {
        Ok(bin_op) => Ok(Expr::BinOp {
            op: bin_op,
            left: Box::new(convert_expr(left)?),
            right: Box::new(convert_expr(right)?),
        }),
        Err(_) => {
            let l = convert_expr(left)?;
            let r = convert_expr(right)?;
            Ok(Expr::Function {
                name: format!("OP_{op:?}"),
                args: vec![l, r],
            })
        }
    }
}

fn convert_function_expr(func: &sqlparser::ast::Function) -> Result<Expr, SqlConversionError> {
    let name = func.name.to_string().to_uppercase();

    if name == "MATCH" {
        return convert_mysql_match(func);
    }
    if name == "CONTAINS"
        || name == "FREETEXT"
        || name == "CONTAINSTABLE"
        || name == "FREETEXTTABLE"
    {
        return convert_sqlserver_fts(func);
    }

    let arg = extract_single_func_arg(&func.args)?;
    let args = match arg {
        Some(a) => vec![a],
        None => vec![],
    };
    Ok(Expr::Function { name, args })
}

/// Convert MySQL MATCH...AGAINST expression to FullTextMatch.
fn convert_mysql_match(func: &sqlparser::ast::Function) -> Result<Expr, SqlConversionError> {
    let func_args = match &func.args {
        FunctionArguments::List(list) => &list.args,
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(
                "MATCH requires column list".to_owned(),
            ))
        }
    };

    let mut columns = Vec::new();
    for arg in func_args {
        match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(SqlExpr::Identifier(ident))) => {
                columns.push(ident.value.clone());
            }
            FunctionArg::Unnamed(FunctionArgExpr::Expr(SqlExpr::CompoundIdentifier(idents))) => {
                if idents.len() == 2 {
                    columns.push(format!("{}.{}", idents[0].value, idents[1].value));
                } else {
                    return Err(SqlConversionError::UnsupportedFeature(
                        "complex identifiers in MATCH not supported".to_owned(),
                    ));
                }
            }
            _ => {
                return Err(SqlConversionError::UnsupportedFeature(
                    "MATCH requires column identifiers".to_owned(),
                ))
            }
        }
    }

    if columns.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "MATCH requires at least one column".to_owned(),
        ));
    }

    Ok(Expr::Function {
        name: "MATCH".to_owned(),
        args: columns
            .iter()
            .map(|c| Expr::Column(ColumnRef::new(c)))
            .collect(),
    })
}

/// Convert SQL Server CONTAINS/FREETEXT expression.
fn convert_sqlserver_fts(func: &sqlparser::ast::Function) -> Result<Expr, SqlConversionError> {
    let name = func.name.to_string().to_uppercase();

    let func_args = match &func.args {
        FunctionArguments::List(list) => &list.args,
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(format!(
                "{name} requires arguments"
            )))
        }
    };

    if func_args.len() < 2 {
        return Err(SqlConversionError::InvalidSql(format!(
            "{name} requires at least 2 arguments"
        )));
    }

    let column = match &func_args[0] {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(SqlExpr::Identifier(ident))) => {
            ident.value.clone()
        }
        FunctionArg::Unnamed(FunctionArgExpr::Expr(SqlExpr::CompoundIdentifier(idents))) => {
            if idents.len() == 2 {
                format!("{}.{}", idents[0].value, idents[1].value)
            } else {
                return Err(SqlConversionError::UnsupportedFeature(format!(
                    "{name} requires simple column identifier"
                )));
            }
        }
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(format!(
                "{name} requires column as first argument"
            )))
        }
    };

    let query_arg = match &func_args[1] {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => convert_expr(e)?,
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(format!(
                "{name} requires expression as second argument"
            )))
        }
    };

    Ok(Expr::Function {
        name,
        args: vec![Expr::Column(ColumnRef::new(column)), query_arg],
    })
}
