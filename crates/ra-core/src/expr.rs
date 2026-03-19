//! Expression types for relational algebra predicates and projections.
//!
//! Expressions represent computations over column values within
//! relational operators. They form the predicate language used in
//! filters, join conditions, and projection lists.

use serde::{Deserialize, Serialize};

/// A scalar expression that can be evaluated over a row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// Reference to a column by name.
    Column(ColumnRef),

    /// A literal constant value.
    Const(Const),

    /// A binary operation (e.g., `a + b`, `x = y`).
    BinOp {
        /// The operator.
        op: BinOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },

    /// A unary operation (e.g., `NOT x`, `IS NULL`).
    UnaryOp {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
    },

    /// A function call (e.g., `UPPER(name)`, `ABS(x)`).
    Function {
        /// Function name.
        name: String,
        /// Function arguments.
        args: Vec<Expr>,
    },

    /// A CASE expression.
    Case {
        /// The expression to match against (for simple CASE).
        operand: Option<Box<Expr>>,
        /// List of (condition, result) pairs.
        when_clauses: Vec<(Expr, Expr)>,
        /// The ELSE result, if any.
        else_result: Option<Box<Expr>>,
    },

    /// A type cast expression.
    Cast {
        /// The expression to cast.
        expr: Box<Expr>,
        /// Target type name.
        target_type: String,
    },

    /// An array constructor (e.g., `ARRAY[1, 2, 3]`).
    Array(Vec<Expr>),

    /// Array element access (e.g., `arr[2]`).
    ArrayIndex(Box<Expr>, Box<Expr>),

    /// Row pattern navigation: PREV(expr, offset).
    PatternPrev(Box<Expr>, usize),

    /// Row pattern navigation: NEXT(expr, offset).
    PatternNext(Box<Expr>, usize),

    /// Row pattern navigation: FIRST(expr) for a pattern variable.
    PatternFirst(Box<Expr>, String),

    /// Row pattern navigation: LAST(expr) for a pattern variable.
    PatternLast(Box<Expr>, String),

    /// Row pattern classifier: returns matched variable name.
    PatternClassifier,

    /// Row pattern match number: returns match sequence number.
    PatternMatchNumber,
}

/// A reference to a column, optionally qualified by a table name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnRef {
    /// Optional table or alias qualifier.
    pub table: Option<String>,
    /// The column name.
    pub column: String,
}

impl ColumnRef {
    /// Create an unqualified column reference.
    #[must_use]
    pub fn new(column: impl Into<String>) -> Self {
        Self {
            table: None,
            column: column.into(),
        }
    }

    /// Create a table-qualified column reference.
    #[must_use]
    pub fn qualified(
        table: impl Into<String>,
        column: impl Into<String>,
    ) -> Self {
        Self {
            table: Some(table.into()),
            column: column.into(),
        }
    }
}

/// A literal constant value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Const {
    /// A null value.
    Null,
    /// A boolean value.
    Bool(bool),
    /// A 64-bit integer.
    Int(i64),
    /// A 64-bit floating-point number.
    Float(f64),
    /// A string value.
    String(String),
}

/// Binary operators for expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BinOp {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Equality comparison.
    Eq,
    /// Inequality comparison.
    Ne,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// Logical AND.
    And,
    /// Logical OR.
    Or,
    /// Modulo.
    Mod,
    /// String concatenation (`||`).
    Concat,
    /// JSON field access (`->>`).
    JsonAccess,
}

/// Unary operators for expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnaryOp {
    /// Logical negation.
    Not,
    /// IS NULL test.
    IsNull,
    /// IS NOT NULL test.
    IsNotNull,
    /// Arithmetic negation.
    Neg,
}

impl std::fmt::Display for ColumnRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(table) = &self.table {
            write!(f, "{table}.{}", self.column)
        } else {
            write!(f, "{}", self.column)
        }
    }
}

impl std::fmt::Display for BinOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sym = match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Mod => "%",
            Self::Concat => "||",
            Self::JsonAccess => "->>",
        };
        write!(f, "{sym}")
    }
}

impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sym = match self {
            Self::Not => "NOT",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
            Self::Neg => "-",
        };
        write!(f, "{sym}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_ref_unqualified() {
        let col = ColumnRef::new("id");
        assert_eq!(col.column, "id");
        assert_eq!(col.table, None);
        assert_eq!(col.to_string(), "id");
    }

    #[test]
    fn column_ref_qualified() {
        let col = ColumnRef::qualified("users", "name");
        assert_eq!(col.table.as_deref(), Some("users"));
        assert_eq!(col.column, "name");
        assert_eq!(col.to_string(), "users.name");
    }

    #[test]
    fn const_variants() {
        let null = Const::Null;
        let b = Const::Bool(true);
        let i = Const::Int(42);
        let f = Const::Float(3.14);
        let s = Const::String("hello".into());

        assert_eq!(null, Const::Null);
        assert_eq!(b, Const::Bool(true));
        assert_eq!(i, Const::Int(42));
        assert_eq!(f, Const::Float(3.14));
        assert_eq!(s, Const::String("hello".into()));
    }

    #[test]
    fn binop_display() {
        assert_eq!(BinOp::Add.to_string(), "+");
        assert_eq!(BinOp::Eq.to_string(), "=");
        assert_eq!(BinOp::And.to_string(), "AND");
    }

    #[test]
    fn unaryop_display() {
        assert_eq!(UnaryOp::Not.to_string(), "NOT");
        assert_eq!(UnaryOp::IsNull.to_string(), "IS NULL");
        assert_eq!(UnaryOp::Neg.to_string(), "-");
    }

    #[test]
    fn build_binary_expression() {
        let expr = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(21))),
        };

        if let Expr::BinOp { op, left, right } = &expr {
            assert_eq!(*op, BinOp::Eq);
            assert_eq!(
                *left.as_ref(),
                Expr::Column(ColumnRef::new("age"))
            );
            assert_eq!(*right.as_ref(), Expr::Const(Const::Int(21)));
        } else {
            panic!("expected BinOp variant");
        }
    }

    #[test]
    fn build_case_expression() {
        let case = Expr::Case {
            operand: None,
            when_clauses: vec![(
                Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("x"))),
                    right: Box::new(Expr::Const(Const::Int(0))),
                },
                Expr::Const(Const::String("positive".into())),
            )],
            else_result: Some(Box::new(Expr::Const(
                Const::String("non-positive".into()),
            ))),
        };

        if let Expr::Case {
            when_clauses,
            else_result,
            ..
        } = &case
        {
            assert_eq!(when_clauses.len(), 1);
            assert!(else_result.is_some());
        } else {
            panic!("expected Case variant");
        }
    }

    #[test]
    fn serialize_roundtrip() {
        let expr = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        };

        let json = serde_json::to_string(&expr)
            .expect("serialization should succeed");
        let deserialized: Expr = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(expr, deserialized);
    }
}
