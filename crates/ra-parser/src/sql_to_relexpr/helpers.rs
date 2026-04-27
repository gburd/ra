use ra_core::expr::{ColumnRef, Expr};
use sqlparser::ast::{FunctionArg, FunctionArgExpr, FunctionArguments, ObjectName, Value};

use super::error::SqlConversionError;
use super::expr::convert_expr;

pub fn extract_single_func_arg(
    args: &FunctionArguments,
) -> Result<Option<Expr>, SqlConversionError> {
    let func_args = match args {
        FunctionArguments::None => return Ok(None),
        FunctionArguments::Subquery(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "subquery in function not supported".to_owned(),
            ))
        }
        FunctionArguments::List(list) => &list.args,
    };

    if func_args.is_empty() {
        return Ok(None);
    }

    if func_args.len() == 1 {
        match &func_args[0] {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => Ok(Some(convert_expr(e)?)),
            FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => Ok(None),
            _ => Err(SqlConversionError::UnsupportedFeature(
                "complex function arguments not supported".to_owned(),
            )),
        }
    } else {
        match &func_args[0] {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => Ok(Some(convert_expr(e)?)),
            _ => Err(SqlConversionError::UnsupportedFeature(
                "complex multi-argument functions not supported".to_owned(),
            )),
        }
    }
}

/// Extract all function arguments as a Vec<Expr>.
pub fn extract_func_args(args: &FunctionArguments) -> Result<Vec<Expr>, SqlConversionError> {
    let func_args = match args {
        FunctionArguments::None => return Ok(vec![]),
        FunctionArguments::Subquery(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "subquery in function not supported".to_owned(),
            ))
        }
        FunctionArguments::List(list) => &list.args,
    };

    let mut exprs = Vec::new();
    for arg in func_args {
        match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                exprs.push(convert_expr(e)?);
            }
            FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => {
                // Skip wildcards
            }
            _ => {
                return Err(SqlConversionError::UnsupportedFeature(
                    "complex function arguments not supported".to_owned(),
                ))
            }
        }
    }
    Ok(exprs)
}

pub fn convert_function_arg(arg: &FunctionArg) -> Result<Expr, SqlConversionError> {
    match arg {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => convert_expr(e),
        FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => Ok(Expr::Column(ColumnRef::new("*"))),
        _ => Err(SqlConversionError::UnsupportedFeature(
            "named or qualified function arguments not supported".to_owned(),
        )),
    }
}

pub fn extract_u64_from_expr(expr: &sqlparser::ast::Expr) -> Result<u64, SqlConversionError> {
    match expr {
        #[cfg(not(feature = "bigdecimal"))]
        sqlparser::ast::Expr::Value(Value::Number(n, _)) => {
            use std::str::FromStr;
            u64::from_str(n).map_err(|_| {
                SqlConversionError::InvalidSql(format!("expected unsigned integer, got: {n}"))
            })
        }
        #[cfg(feature = "bigdecimal")]
        sqlparser::ast::Expr::Value(Value::Number(n, _)) => {
            use std::str::FromStr;
            let n_str = n.to_string();
            u64::from_str(&n_str).map_err(|_| {
                SqlConversionError::InvalidSql(format!("expected unsigned integer, got: {n_str}"))
            })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "only literal integers supported for LIMIT/OFFSET".to_owned(),
        )),
    }
}

/// Pad column aliases to match the expected count, filling with None.
pub fn pad_aliases(mut aliases: Vec<Option<String>>, count: usize) -> Vec<Option<String>> {
    aliases.resize(count, None);
    aliases
}

pub fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .iter()
        .map(|i| i.value.as_str())
        .collect::<Vec<_>>()
        .join(".")
}
