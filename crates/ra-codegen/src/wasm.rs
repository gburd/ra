//! WebAssembly code generation backend.
//!
//! Generates WASM modules from [`PhysicalExpr`] for portable
//! execution. The WASM module can be executed in any WASM runtime
//! (wasmtime, wasmer, browser V8) making it suitable for edge
//! deployments and cross-platform scenarios.
//!
//! The generated module exports an `eval` function with signature:
//! `(row_ptr: i32, num_cols: i32) -> i64`

use crate::ir::{Literal, PhysicalExpr};
use ra_core::{BinOp, UnaryOp};
use thiserror::Error;

/// Errors from WASM code generation.
#[derive(Debug, Error)]
pub enum WasmError {
    /// Expression contains features not supported by the WASM backend.
    #[error("unsupported expression for WASM: {0}")]
    Unsupported(String),

    /// Error during WASM module construction.
    #[error("WASM module error: {0}")]
    ModuleError(String),
}

/// A WASM module containing a compiled expression.
#[derive(Debug, Clone)]
pub struct WasmModule {
    /// The raw WASM binary (WAT text format for inspection).
    pub wat: String,
    /// The binary WASM bytes.
    pub bytes: Vec<u8>,
}

/// Generate a WASM module from a physical expression.
///
/// The module exports an `eval` function that reads i64 column values
/// from linear memory starting at `row_ptr` and returns an i64 result.
///
/// # Errors
///
/// Returns `WasmError::Unsupported` if the expression contains
/// features not yet supported (floats, strings, casts, NULL checks).
/// Returns `WasmError::ModuleError` if WAT parsing fails.
pub fn compile_to_wasm(
    expr: &PhysicalExpr,
) -> Result<WasmModule, WasmError> {
    let mut body = Vec::new();
    emit_wasm_expr(expr, &mut body)?;

    let wat = format!(
        r#"(module
  (memory (export "memory") 1)
  (func (export "eval") (param $row_ptr i32) (param $num_cols i32) (result i64)
    {body}
  )
)"#,
        body = body.join("\n    ")
    );

    let bytes = wat::parse_str(&wat).map_err(|e| {
        WasmError::ModuleError(format!("WAT parse error: {e}"))
    })?;

    Ok(WasmModule { wat, bytes })
}

fn emit_wasm_expr(
    expr: &PhysicalExpr,
    out: &mut Vec<String>,
) -> Result<(), WasmError> {
    match expr {
        PhysicalExpr::ColumnIndex(idx) => {
            // Load i64 from memory at row_ptr + idx * 8
            out.push(format!(
                "(i64.load (i32.add (local.get $row_ptr) (i32.const {})))",
                idx * 8
            ));
            Ok(())
        }
        PhysicalExpr::Literal(lit) => match lit {
            Literal::Int64(val) => {
                out.push(format!("(i64.const {val})"));
                Ok(())
            }
            Literal::Bool(val) => {
                let v = i64::from(*val);
                out.push(format!("(i64.const {v})"));
                Ok(())
            }
            Literal::Float64(_) => {
                Err(WasmError::Unsupported("float literals".into()))
            }
            Literal::Utf8(_) => {
                Err(WasmError::Unsupported("string literals".into()))
            }
            Literal::Null => {
                // Represent NULL as a sentinel value
                out.push("(i64.const -9223372036854775808)".into());
                Ok(())
            }
        },
        PhysicalExpr::BinaryOp { op, left, right } => {
            emit_wasm_expr(left, out)?;
            emit_wasm_expr(right, out)?;
            let wasm_op = match op {
                BinOp::Add => "i64.add",
                BinOp::Sub => "i64.sub",
                BinOp::Mul => "i64.mul",
                BinOp::Div => "i64.div_s",
                BinOp::Eq => "i64.eq",
                BinOp::Ne => "i64.ne",
                BinOp::Lt => "i64.lt_s",
                BinOp::Le => "i64.le_s",
                BinOp::Gt => "i64.gt_s",
                BinOp::Ge => "i64.ge_s",
                BinOp::And => "i64.and",
                BinOp::Or => "i64.or",
            };
            // Comparison ops return i32 in WASM; extend to i64
            let needs_extend = matches!(
                op,
                BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
            );
            if needs_extend {
                out.push(format!("(i64.extend_i32_u ({wasm_op}))"));
            } else {
                out.push(format!("({wasm_op})"));
            }
            Ok(())
        }
        PhysicalExpr::UnaryOp { op, operand } => {
            match op {
                UnaryOp::Neg => {
                    out.push("(i64.const 0)".into());
                    emit_wasm_expr(operand, out)?;
                    out.push("(i64.sub)".into());
                    Ok(())
                }
                UnaryOp::Not => {
                    emit_wasm_expr(operand, out)?;
                    out.push("(i64.eqz)".into());
                    out.push("(i64.extend_i32_u)".into());
                    Ok(())
                }
                UnaryOp::IsNull | UnaryOp::IsNotNull => {
                    Err(WasmError::Unsupported(
                        "NULL checks in WASM".into(),
                    ))
                }
            }
        }
        PhysicalExpr::Cast { .. } => {
            Err(WasmError::Unsupported("CAST in WASM".into()))
        }
    }
}

/// Check whether a physical expression can be compiled to WASM.
#[must_use]
pub fn can_compile_to_wasm(expr: &PhysicalExpr) -> bool {
    match expr {
        PhysicalExpr::ColumnIndex(_) => true,
        PhysicalExpr::Literal(lit) => {
            matches!(
                lit,
                Literal::Int64(_) | Literal::Bool(_) | Literal::Null
            )
        }
        PhysicalExpr::BinaryOp { left, right, .. } => {
            can_compile_to_wasm(left) && can_compile_to_wasm(right)
        }
        PhysicalExpr::UnaryOp { op, operand } => {
            matches!(op, UnaryOp::Neg | UnaryOp::Not)
                && can_compile_to_wasm(operand)
        }
        PhysicalExpr::Cast { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_compile_integer_arithmetic() {
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(PhysicalExpr::ColumnIndex(0)),
            right: Box::new(PhysicalExpr::Literal(Literal::Int64(10))),
        };
        assert!(can_compile_to_wasm(&expr));
    }

    #[test]
    fn cannot_compile_string() {
        let expr = PhysicalExpr::Literal(Literal::Utf8("hi".into()));
        assert!(!can_compile_to_wasm(&expr));
    }

    #[test]
    fn cannot_compile_cast() {
        let expr = PhysicalExpr::Cast {
            expr: Box::new(PhysicalExpr::Literal(Literal::Int64(1))),
            target: crate::ir::DataType::Utf8,
        };
        assert!(!can_compile_to_wasm(&expr));
    }

    #[test]
    fn compile_literal() {
        let expr = PhysicalExpr::Literal(Literal::Int64(42));
        let module = compile_to_wasm(&expr)
            .expect("compilation should succeed");
        assert!(!module.bytes.is_empty());
        assert!(module.wat.contains("i64.const 42"));
    }

    #[test]
    fn compile_addition() {
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(PhysicalExpr::Literal(Literal::Int64(3))),
            right: Box::new(PhysicalExpr::Literal(Literal::Int64(4))),
        };
        let module = compile_to_wasm(&expr)
            .expect("compilation should succeed");
        assert!(module.wat.contains("i64.add"));
    }

    #[test]
    fn compile_comparison() {
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Gt,
            left: Box::new(PhysicalExpr::ColumnIndex(0)),
            right: Box::new(PhysicalExpr::Literal(Literal::Int64(100))),
        };
        let module = compile_to_wasm(&expr)
            .expect("compilation should succeed");
        assert!(module.wat.contains("i64.gt_s"));
        assert!(module.wat.contains("i64.extend_i32_u"));
    }

    #[test]
    fn compile_column_load() {
        let expr = PhysicalExpr::ColumnIndex(2);
        let module = compile_to_wasm(&expr)
            .expect("compilation should succeed");
        // Column 2 at offset 16
        assert!(module.wat.contains("i32.const 16"));
        assert!(module.wat.contains("i64.load"));
    }

    #[test]
    fn compile_negation() {
        let expr = PhysicalExpr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(PhysicalExpr::Literal(Literal::Int64(5))),
        };
        let module = compile_to_wasm(&expr)
            .expect("compilation should succeed");
        assert!(module.wat.contains("i64.sub"));
    }

    #[test]
    fn compile_not() {
        let expr = PhysicalExpr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(PhysicalExpr::Literal(
                Literal::Bool(true),
            )),
        };
        let module = compile_to_wasm(&expr)
            .expect("compilation should succeed");
        assert!(module.wat.contains("i64.eqz"));
    }

    #[test]
    fn compile_nested() {
        // (col[0] + 10) > (col[1] * 2)
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Gt,
            left: Box::new(PhysicalExpr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(PhysicalExpr::ColumnIndex(0)),
                right: Box::new(PhysicalExpr::Literal(
                    Literal::Int64(10),
                )),
            }),
            right: Box::new(PhysicalExpr::BinaryOp {
                op: BinOp::Mul,
                left: Box::new(PhysicalExpr::ColumnIndex(1)),
                right: Box::new(PhysicalExpr::Literal(
                    Literal::Int64(2),
                )),
            }),
        };
        let module = compile_to_wasm(&expr)
            .expect("compilation should succeed");
        assert!(module.wat.contains("i64.add"));
        assert!(module.wat.contains("i64.mul"));
        assert!(module.wat.contains("i64.gt_s"));
    }

    #[test]
    fn unsupported_float_literal() {
        let expr = PhysicalExpr::Literal(Literal::Float64(2.72));
        let err = compile_to_wasm(&expr).unwrap_err();
        assert!(err.to_string().contains("float"), "got: {err}");
    }
}
