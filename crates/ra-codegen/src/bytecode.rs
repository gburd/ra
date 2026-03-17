//! Stack-based bytecode compiler and interpreter for expressions.
//!
//! The bytecode VM evaluates [`PhysicalExpr`] trees by compiling them
//! into a flat instruction sequence and executing them on a simple
//! stack machine. This avoids the overhead of recursive tree-walking
//! for hot inner loops.

use crate::ir::{DataType, Literal, PhysicalExpr};
use crate::volcano::{ExecError, Value};
use ra_core::{BinOp, UnaryOp};

/// A bytecode instruction for the expression VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// Push a literal value onto the stack.
    PushLiteral(Value),

    /// Push the value at column index `i` from the current row.
    LoadColumn(usize),

    /// Pop two values, apply the binary operator, push the result.
    BinaryOp(BinOp),

    /// Pop one value, apply the unary operator, push the result.
    UnaryOp(UnaryOp),

    /// Pop one value, cast to the target type, push the result.
    Cast(DataType),
}

/// A compiled bytecode program for expression evaluation.
#[derive(Debug, Clone)]
pub struct BytecodeProgram {
    /// The instruction sequence.
    pub instructions: Vec<Instruction>,
}

impl BytecodeProgram {
    /// Compile a physical expression into bytecode.
    ///
    /// The resulting program, when executed against a row, pushes
    /// exactly one value onto the stack.
    #[must_use]
    pub fn compile(expr: &PhysicalExpr) -> Self {
        let mut instructions = Vec::new();
        compile_expr(expr, &mut instructions);
        Self { instructions }
    }

    /// Execute this program against a row, returning the result value.
    ///
    /// # Errors
    ///
    /// Returns `ExecError` if a column index is out of bounds,
    /// a type error occurs during evaluation, or the bytecode
    /// stack is in an invalid state.
    pub fn execute(&self, row: &[Value]) -> Result<Value, ExecError> {
        let mut stack: Vec<Value> =
            Vec::with_capacity(self.instructions.len());

        for instr in &self.instructions {
            match instr {
                Instruction::PushLiteral(v) => {
                    stack.push(v.clone());
                }
                Instruction::LoadColumn(idx) => {
                    let v = row.get(*idx).cloned().ok_or(
                        ExecError::ColumnOutOfBounds {
                            index: *idx,
                            width: row.len(),
                        },
                    )?;
                    stack.push(v);
                }
                Instruction::BinaryOp(op) => {
                    let right = pop_stack(&mut stack)?;
                    let left = pop_stack(&mut stack)?;
                    let dummy_row = vec![];
                    let result = crate::volcano::eval_expr(
                        &PhysicalExpr::BinaryOp {
                            op: *op,
                            left: Box::new(PhysicalExpr::Literal(
                                value_to_literal(&left),
                            )),
                            right: Box::new(PhysicalExpr::Literal(
                                value_to_literal(&right),
                            )),
                        },
                        &dummy_row,
                    )?;
                    stack.push(result);
                }
                Instruction::UnaryOp(op) => {
                    let operand = pop_stack(&mut stack)?;
                    let dummy_row = vec![];
                    let result = crate::volcano::eval_expr(
                        &PhysicalExpr::UnaryOp {
                            op: *op,
                            operand: Box::new(PhysicalExpr::Literal(
                                value_to_literal(&operand),
                            )),
                        },
                        &dummy_row,
                    )?;
                    stack.push(result);
                }
                Instruction::Cast(target) => {
                    let val = pop_stack(&mut stack)?;
                    let dummy_row = vec![];
                    let result = crate::volcano::eval_expr(
                        &PhysicalExpr::Cast {
                            expr: Box::new(PhysicalExpr::Literal(
                                value_to_literal(&val),
                            )),
                            target: *target,
                        },
                        &dummy_row,
                    )?;
                    stack.push(result);
                }
            }
        }

        pop_stack(&mut stack)
    }

    /// Number of instructions in this program.
    #[must_use]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Whether the program has no instructions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}

fn pop_stack(stack: &mut Vec<Value>) -> Result<Value, ExecError> {
    stack.pop().ok_or_else(|| {
        ExecError::Internal("bytecode stack underflow".into())
    })
}

fn compile_expr(
    expr: &PhysicalExpr,
    out: &mut Vec<Instruction>,
) {
    match expr {
        PhysicalExpr::ColumnIndex(idx) => {
            out.push(Instruction::LoadColumn(*idx));
        }
        PhysicalExpr::Literal(lit) => {
            out.push(Instruction::PushLiteral(literal_to_value(lit)));
        }
        PhysicalExpr::BinaryOp { op, left, right } => {
            compile_expr(left, out);
            compile_expr(right, out);
            out.push(Instruction::BinaryOp(*op));
        }
        PhysicalExpr::UnaryOp { op, operand } => {
            compile_expr(operand, out);
            out.push(Instruction::UnaryOp(*op));
        }
        PhysicalExpr::Cast { expr, target } => {
            compile_expr(expr, out);
            out.push(Instruction::Cast(*target));
        }
    }
}

fn literal_to_value(lit: &Literal) -> Value {
    match lit {
        Literal::Null => Value::Null,
        Literal::Bool(b) => Value::Bool(*b),
        Literal::Int64(i) => Value::Int64(*i),
        Literal::Float64(f) => Value::Float64(*f),
        Literal::Utf8(s) => Value::Utf8(s.clone()),
    }
}

fn value_to_literal(val: &Value) -> Literal {
    match val {
        Value::Null => Literal::Null,
        Value::Bool(b) => Literal::Bool(*b),
        Value::Int64(i) => Literal::Int64(*i),
        Value::Float64(f) => Literal::Float64(*f),
        Value::Utf8(s) => Literal::Utf8(s.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Literal;

    #[test]
    fn compile_literal() {
        let expr = PhysicalExpr::Literal(Literal::Int64(42));
        let prog = BytecodeProgram::compile(&expr);
        assert_eq!(prog.len(), 1);

        let result = prog.execute(&[])
            .expect("execution should succeed");
        assert_eq!(result, Value::Int64(42));
    }

    #[test]
    fn compile_column_load() {
        let expr = PhysicalExpr::ColumnIndex(1);
        let prog = BytecodeProgram::compile(&expr);
        assert_eq!(prog.len(), 1);

        let row = vec![Value::Int64(10), Value::Utf8("hello".into())];
        let result = prog.execute(&row)
            .expect("execution should succeed");
        assert_eq!(result, Value::Utf8("hello".into()));
    }

    #[test]
    fn compile_binary_op() {
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(PhysicalExpr::ColumnIndex(0)),
            right: Box::new(PhysicalExpr::Literal(Literal::Int64(10))),
        };
        let prog = BytecodeProgram::compile(&expr);
        assert_eq!(prog.len(), 3); // load, push, add

        let row = vec![Value::Int64(5)];
        let result = prog.execute(&row)
            .expect("execution should succeed");
        assert_eq!(result, Value::Int64(15));
    }

    #[test]
    fn compile_nested_expression() {
        // (col[0] + 10) > 20
        let expr = PhysicalExpr::BinaryOp {
            op: BinOp::Gt,
            left: Box::new(PhysicalExpr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(PhysicalExpr::ColumnIndex(0)),
                right: Box::new(PhysicalExpr::Literal(
                    Literal::Int64(10),
                )),
            }),
            right: Box::new(PhysicalExpr::Literal(Literal::Int64(20))),
        };
        let prog = BytecodeProgram::compile(&expr);
        assert_eq!(prog.len(), 5);

        // (5 + 10) > 20 = false
        let row = vec![Value::Int64(5)];
        let result = prog.execute(&row)
            .expect("execution should succeed");
        assert_eq!(result, Value::Bool(false));

        // (15 + 10) > 20 = true
        let row = vec![Value::Int64(15)];
        let result = prog.execute(&row)
            .expect("execution should succeed");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn compile_unary_op() {
        let expr = PhysicalExpr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(PhysicalExpr::Literal(
                Literal::Bool(true),
            )),
        };
        let prog = BytecodeProgram::compile(&expr);
        let result = prog.execute(&[])
            .expect("execution should succeed");
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn compile_cast() {
        let expr = PhysicalExpr::Cast {
            expr: Box::new(PhysicalExpr::Literal(Literal::Int64(42))),
            target: DataType::Utf8,
        };
        let prog = BytecodeProgram::compile(&expr);
        let result = prog.execute(&[])
            .expect("execution should succeed");
        assert_eq!(result, Value::Utf8("42".into()));
    }

    #[test]
    fn column_out_of_bounds() {
        let expr = PhysicalExpr::ColumnIndex(5);
        let prog = BytecodeProgram::compile(&expr);
        let err = prog.execute(&[Value::Int64(1)]).unwrap_err();
        assert!(
            err.to_string().contains("out of bounds"),
            "got: {err}"
        );
    }

    #[test]
    fn program_is_empty() {
        let prog = BytecodeProgram {
            instructions: vec![],
        };
        assert!(prog.is_empty());
    }
}
