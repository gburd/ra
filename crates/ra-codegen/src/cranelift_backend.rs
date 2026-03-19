//! Cranelift IR generation backend for expression evaluation.
//!
//! Compiles [`PhysicalExpr`] trees into Cranelift IR that can be
//! verified and analyzed. The IR represents what native JIT-compiled
//! code would look like, enabling optimization analysis and testing
//! of the code generation pipeline.
//!
//! Full JIT execution requires the `cranelift-jit` crate. This module
//! generates and verifies the IR, which is the prerequisite step.

use cranelift::prelude::*;
use cranelift_codegen::ir::Function;
use cranelift_codegen::verify_function;
use thiserror::Error;

use crate::ir::{Literal, PhysicalExpr};
use ra_core::BinOp;

/// Errors from Cranelift IR generation.
#[derive(Debug, Error)]
pub enum CraneliftError {
    /// Expression contains features not supported by the backend.
    #[error("unsupported expression for Cranelift: {0}")]
    Unsupported(String),

    /// Cranelift codegen error.
    #[error("cranelift codegen error: {0}")]
    Codegen(String),

    /// Cranelift IR verification failed.
    #[error("cranelift IR verification failed: {0}")]
    Verification(String),
}

/// Configuration for the Cranelift backend.
#[derive(Debug, Clone)]
pub struct CraneliftConfig {
    /// Optimization level.
    pub opt_level: CraneliftOptLevel,
}

impl Default for CraneliftConfig {
    fn default() -> Self {
        Self {
            opt_level: CraneliftOptLevel::Speed,
        }
    }
}

/// Optimization level for Cranelift code generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraneliftOptLevel {
    /// No optimization (fastest compilation).
    None,
    /// Optimize for speed (default).
    Speed,
    /// Optimize for code size.
    SpeedAndSize,
}

/// The result of Cranelift IR generation.
#[derive(Debug)]
pub struct CraneliftIR {
    /// The generated Cranelift function.
    pub function: Function,
    /// Human-readable Cranelift IR text.
    pub ir_text: String,
}

/// Check whether a physical expression can be compiled via Cranelift.
///
/// Currently supports integer arithmetic and comparisons.
/// Expressions involving strings, NULLs, or complex types are not
/// supported.
#[must_use]
pub fn can_compile_cranelift(expr: &PhysicalExpr) -> bool {
    match expr {
        PhysicalExpr::ColumnIndex(_) => true,
        PhysicalExpr::Literal(lit) => {
            matches!(lit, Literal::Int64(_) | Literal::Bool(_))
        }
        PhysicalExpr::BinaryOp { left, right, .. } => {
            can_compile_cranelift(left)
                && can_compile_cranelift(right)
        }
        PhysicalExpr::UnaryOp { op, operand } => {
            matches!(
                op,
                ra_core::UnaryOp::Not | ra_core::UnaryOp::Neg
            ) && can_compile_cranelift(operand)
        }
        PhysicalExpr::Cast { .. } => false,
    }
}

/// Generate and verify Cranelift IR for a physical expression.
///
/// The generated function has the signature `fn(*const i64) -> i64`.
/// Column values are read as i64 from the pointer at the
/// corresponding offset.
///
/// # Errors
///
/// Returns `CraneliftError::Unsupported` if the expression contains
/// features the backend cannot compile (strings, casts, NULL checks).
/// Returns `CraneliftError::Codegen` on ISA or settings errors.
/// Returns `CraneliftError::Verification` if the generated IR fails
/// verification.
pub fn generate_cranelift_ir(
    expr: &PhysicalExpr,
    config: &CraneliftConfig,
) -> Result<CraneliftIR, CraneliftError> {
    let mut flag_builder = settings::builder();
    let cl_opt = match config.opt_level {
        CraneliftOptLevel::None => "none",
        CraneliftOptLevel::Speed => "speed",
        CraneliftOptLevel::SpeedAndSize => "speed_and_size",
    };
    flag_builder
        .set("opt_level", cl_opt)
        .map_err(|e| CraneliftError::Codegen(e.to_string()))?;

    let isa_builder = cranelift_native::builder()
        .map_err(|e| CraneliftError::Codegen(e.to_string()))?;
    let flags = settings::Flags::new(flag_builder);
    let isa = isa_builder
        .finish(flags)
        .map_err(|e| CraneliftError::Codegen(e.to_string()))?;

    let ptr_type = isa.pointer_type();
    let call_conv = isa.default_call_conv();

    let mut sig = Signature::new(call_conv);
    sig.params.push(AbiParam::new(ptr_type));
    sig.returns.push(AbiParam::new(types::I64));

    let mut func = Function::with_name_signature(
        cranelift_codegen::ir::UserFuncName::user(0, 0),
        sig,
    );

    let mut func_builder_ctx = FunctionBuilderContext::new();
    {
        let mut builder =
            FunctionBuilder::new(&mut func, &mut func_builder_ctx);
        let entry_block = builder.create_block();
        builder
            .append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let row_ptr = builder.block_params(entry_block)[0];
        let result = emit_expr(&mut builder, expr, row_ptr)?;
        builder.ins().return_(&[result]);
        builder.finalize();
    }

    // Verify the generated IR
    let flags_for_verify = settings::Flags::new(settings::builder());
    verify_function(&func, &flags_for_verify).map_err(|e| {
        CraneliftError::Verification(e.to_string())
    })?;

    let ir_text = format!("{func}");

    Ok(CraneliftIR {
        function: func,
        ir_text,
    })
}

fn emit_expr(
    builder: &mut FunctionBuilder<'_>,
    expr: &PhysicalExpr,
    row_ptr: cranelift::prelude::Value,
) -> Result<cranelift::prelude::Value, CraneliftError> {
    match expr {
        PhysicalExpr::ColumnIndex(idx) => {
            let offset = i32::try_from(*idx)
                .ok()
                .and_then(|i| i.checked_mul(8))
                .ok_or_else(|| {
                    CraneliftError::Unsupported(format!(
                        "column index {idx} too large"
                    ))
                })?;
            let mem_flags = MemFlags::new();
            Ok(builder
                .ins()
                .load(types::I64, mem_flags, row_ptr, offset))
        }
        PhysicalExpr::Literal(lit) => match lit {
            Literal::Int64(val) => {
                Ok(builder.ins().iconst(types::I64, *val))
            }
            Literal::Bool(val) => {
                let v = i64::from(*val);
                Ok(builder.ins().iconst(types::I64, v))
            }
            _ => Err(CraneliftError::Unsupported(format!(
                "literal type: {lit:?}"
            ))),
        },
        PhysicalExpr::BinaryOp { op, left, right } => {
            let lv = emit_expr(builder, left, row_ptr)?;
            let rv = emit_expr(builder, right, row_ptr)?;
            Ok(emit_binop(builder, *op, lv, rv))
        }
        PhysicalExpr::UnaryOp { op, operand } => {
            let v = emit_expr(builder, operand, row_ptr)?;
            emit_unaryop(builder, *op, v)
        }
        PhysicalExpr::Cast { .. } => Err(
            CraneliftError::Unsupported("cast expressions".into()),
        ),
    }
}

fn emit_binop(
    builder: &mut FunctionBuilder<'_>,
    op: BinOp,
    left: cranelift::prelude::Value,
    right: cranelift::prelude::Value,
) -> cranelift::prelude::Value {
    match op {
        BinOp::Add => builder.ins().iadd(left, right),
        BinOp::Sub => builder.ins().isub(left, right),
        BinOp::Mul => builder.ins().imul(left, right),
        BinOp::Div => builder.ins().sdiv(left, right),
        BinOp::Mod => builder.ins().srem(left, right),
        BinOp::Eq => {
            let cmp =
                builder.ins().icmp(IntCC::Equal, left, right);
            builder.ins().uextend(types::I64, cmp)
        }
        BinOp::Ne => {
            let cmp =
                builder.ins().icmp(IntCC::NotEqual, left, right);
            builder.ins().uextend(types::I64, cmp)
        }
        BinOp::Lt => {
            let cmp = builder
                .ins()
                .icmp(IntCC::SignedLessThan, left, right);
            builder.ins().uextend(types::I64, cmp)
        }
        BinOp::Le => {
            let cmp = builder.ins().icmp(
                IntCC::SignedLessThanOrEqual,
                left,
                right,
            );
            builder.ins().uextend(types::I64, cmp)
        }
        BinOp::Gt => {
            let cmp = builder
                .ins()
                .icmp(IntCC::SignedGreaterThan, left, right);
            builder.ins().uextend(types::I64, cmp)
        }
        BinOp::Ge => {
            let cmp = builder.ins().icmp(
                IntCC::SignedGreaterThanOrEqual,
                left,
                right,
            );
            builder.ins().uextend(types::I64, cmp)
        }
        BinOp::And => builder.ins().band(left, right),
        BinOp::Or => builder.ins().bor(left, right),
        BinOp::Concat | BinOp::JsonAccess => {
            // String operations not yet supported in Cranelift backend
            // Return left operand as placeholder
            left
        }
    }
}

fn emit_unaryop(
    builder: &mut FunctionBuilder<'_>,
    op: ra_core::UnaryOp,
    val: cranelift::prelude::Value,
) -> Result<cranelift::prelude::Value, CraneliftError> {
    match op {
        ra_core::UnaryOp::Not => {
            let zero = builder.ins().iconst(types::I64, 0);
            let cmp = builder.ins().icmp(IntCC::Equal, val, zero);
            Ok(builder.ins().uextend(types::I64, cmp))
        }
        ra_core::UnaryOp::Neg => Ok(builder.ins().ineg(val)),
        ra_core::UnaryOp::IsNull
        | ra_core::UnaryOp::IsNotNull => {
            Err(CraneliftError::Unsupported(
                "NULL checks in Cranelift".into(),
            ))
        }
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
            right: Box::new(PhysicalExpr::Literal(
                Literal::Int64(10),
            )),
        };
        assert!(can_compile_cranelift(&expr));
    }

    #[test]
    fn cannot_compile_string_literal() {
        let expr =
            PhysicalExpr::Literal(Literal::Utf8("hello".into()));
        assert!(!can_compile_cranelift(&expr));
    }

    #[test]
    fn cannot_compile_cast() {
        let expr = PhysicalExpr::Cast {
            expr: Box::new(PhysicalExpr::Literal(
                Literal::Int64(42),
            )),
            target: crate::ir::DataType::Utf8,
        };
        assert!(!can_compile_cranelift(&expr));
    }

    #[test]
    fn generate_literal_ir() {
        let expr = PhysicalExpr::Literal(Literal::Int64(42));
        let config = CraneliftConfig::default();
        let result = generate_cranelift_ir(&expr, &config)
            .expect("IR generation should succeed");
        assert!(
            result.ir_text.contains("iconst"),
            "IR should contain iconst: {}",
            result.ir_text
        );
    }

    #[test]
    fn generate_addition_ir() {
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(PhysicalExpr::Literal(
                Literal::Int64(3),
            )),
            right: Box::new(PhysicalExpr::Literal(
                Literal::Int64(4),
            )),
        };
        let config = CraneliftConfig::default();
        let result = generate_cranelift_ir(&expr, &config)
            .expect("IR generation should succeed");
        assert!(
            result.ir_text.contains("iadd"),
            "IR should contain iadd: {}",
            result.ir_text
        );
    }

    #[test]
    fn generate_comparison_ir() {
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Gt,
            left: Box::new(PhysicalExpr::ColumnIndex(0)),
            right: Box::new(PhysicalExpr::Literal(
                Literal::Int64(100),
            )),
        };
        let config = CraneliftConfig::default();
        let result = generate_cranelift_ir(&expr, &config)
            .expect("IR generation should succeed");
        assert!(
            result.ir_text.contains("icmp"),
            "IR should contain icmp: {}",
            result.ir_text
        );
    }

    #[test]
    fn generate_nested_expression_ir() {
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
        let config = CraneliftConfig::default();
        let result = generate_cranelift_ir(&expr, &config)
            .expect("IR generation should succeed");
        assert!(result.ir_text.contains("iadd"));
        assert!(result.ir_text.contains("imul"));
        assert!(result.ir_text.contains("icmp"));
    }

    #[test]
    fn generate_negation_ir() {
        let expr = PhysicalExpr::UnaryOp {
            op: ra_core::UnaryOp::Neg,
            operand: Box::new(PhysicalExpr::Literal(
                Literal::Int64(5),
            )),
        };
        let config = CraneliftConfig::default();
        let result = generate_cranelift_ir(&expr, &config)
            .expect("IR generation should succeed");
        assert!(
            result.ir_text.contains("ineg"),
            "IR should contain ineg: {}",
            result.ir_text
        );
    }

    #[test]
    fn cranelift_config_default() {
        let config = CraneliftConfig::default();
        assert_eq!(config.opt_level, CraneliftOptLevel::Speed);
    }

    #[test]
    fn unsupported_null_check() {
        let expr = PhysicalExpr::UnaryOp {
            op: ra_core::UnaryOp::IsNull,
            operand: Box::new(PhysicalExpr::ColumnIndex(0)),
        };
        assert!(!can_compile_cranelift(&expr));
    }
}
