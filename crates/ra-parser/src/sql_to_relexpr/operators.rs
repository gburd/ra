use ra_core::expr::{BinOp, Const, Expr};
use sqlparser::ast::{BinaryOperator, UnaryOperator, Value};

use super::error::SqlConversionError;

pub fn convert_value(val: &Value) -> Result<Expr, SqlConversionError> {
    match val {
        #[cfg(not(feature = "bigdecimal"))]
        Value::Number(n, _) => {
            use std::str::FromStr;
            if let Ok(i) = i64::from_str(n) {
                Ok(Expr::Const(Const::Int(i)))
            } else if let Ok(f) = f64::from_str(n) {
                Ok(Expr::Const(Const::Float(f)))
            } else {
                Err(SqlConversionError::InvalidSql(format!(
                    "invalid number: {n}"
                )))
            }
        }
        #[cfg(feature = "bigdecimal")]
        Value::Number(n, _) => {
            use std::str::FromStr;
            let n_str = n.to_string();
            if let Ok(i) = i64::from_str(&n_str) {
                Ok(Expr::Const(Const::Int(i)))
            } else if let Ok(f) = f64::from_str(&n_str) {
                Ok(Expr::Const(Const::Float(f)))
            } else {
                Err(SqlConversionError::InvalidSql(format!(
                    "invalid number: {n_str}"
                )))
            }
        }
        Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) => {
            Ok(Expr::Const(Const::String(s.clone())))
        }
        Value::Boolean(b) => Ok(Expr::Const(Const::Bool(*b))),
        Value::Null => Ok(Expr::Const(Const::Null)),
        Value::Placeholder(p) => Ok(Expr::Const(Const::String(p.clone()))),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "value type not supported: {val:?}"
        ))),
    }
}

pub fn convert_binary_op(op: &BinaryOperator) -> Result<BinOp, SqlConversionError> {
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

pub fn convert_unary_op(op: &UnaryOperator) -> Result<ra_core::expr::UnaryOp, SqlConversionError> {
    match op {
        UnaryOperator::Not => Ok(ra_core::expr::UnaryOp::Not),
        UnaryOperator::Minus => Ok(ra_core::expr::UnaryOp::Neg),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "unary operator not supported: {op:?}"
        ))),
    }
}
