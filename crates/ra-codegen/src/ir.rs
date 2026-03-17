//! Intermediate representation for physical query plans.
//!
//! The IR sits between the logical [`RelExpr`] from `ra-core` and
//! the various code generation backends. It describes physical
//! operators with explicit data types, memory layouts, and execution
//! strategies that backends translate into executable code.

use std::fmt;

use ra_core::{
    AggregateFunction, BinOp, ColumnRef, JoinType, NullOrdering,
    SortDirection, UnaryOp,
};
use serde::{Deserialize, Serialize};

/// A data type for values flowing through the physical plan.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum DataType {
    /// Boolean (1 byte).
    Bool,
    /// 64-bit signed integer.
    Int64,
    /// 64-bit IEEE 754 float.
    Float64,
    /// Variable-length UTF-8 string.
    Utf8,
    /// SQL NULL of unknown type.
    Null,
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => write!(f, "bool"),
            Self::Int64 => write!(f, "int64"),
            Self::Float64 => write!(f, "float64"),
            Self::Utf8 => write!(f, "utf8"),
            Self::Null => write!(f, "null"),
        }
    }
}

/// Schema describing the columns produced by a physical operator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    /// Ordered list of output columns.
    pub columns: Vec<ColumnDef>,
}

impl Schema {
    /// Create a new schema from column definitions.
    #[must_use]
    pub fn new(columns: Vec<ColumnDef>) -> Self {
        Self { columns }
    }

    /// Number of columns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    /// Whether the schema has zero columns.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Look up a column index by reference. Returns `None` when the
    /// column is not in this schema.
    #[must_use]
    pub fn column_index(&self, col: &ColumnRef) -> Option<usize> {
        self.columns.iter().position(|c| {
            c.name == col.column
                && (col.table.is_none()
                    || col.table.as_deref() == c.table.as_deref())
        })
    }
}

/// A single column definition within a schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    /// Column name.
    pub name: String,
    /// Optional table qualifier.
    pub table: Option<String>,
    /// Data type.
    pub data_type: DataType,
    /// Whether this column is nullable.
    pub nullable: bool,
}

/// A physical plan node describing an executable operator.
///
/// Each variant carries enough information for a backend to generate
/// code without consulting the logical plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PhysicalPlan {
    /// Read rows from a named table (or in-memory batch).
    TableScan {
        /// Table name.
        table: String,
        /// Output schema.
        schema: Schema,
        /// Optional filter pushed into the scan.
        pushed_filter: Option<PhysicalExpr>,
    },

    /// Filter rows by a predicate.
    Filter {
        /// The predicate to evaluate.
        predicate: PhysicalExpr,
        /// Child operator providing input rows.
        input: Box<PhysicalPlan>,
    },

    /// Project a set of expressions.
    Project {
        /// Expressions to evaluate for each output column.
        exprs: Vec<PhysicalExpr>,
        /// Output column names.
        output_names: Vec<String>,
        /// Child operator providing input rows.
        input: Box<PhysicalPlan>,
    },

    /// Hash join two inputs.
    HashJoin {
        /// Join type (inner, left, right, full, semi, anti).
        join_type: JoinType,
        /// Equi-join key column indices on the left (build) side.
        left_keys: Vec<usize>,
        /// Equi-join key column indices on the right (probe) side.
        right_keys: Vec<usize>,
        /// Optional non-equi condition evaluated after key match.
        condition: Option<PhysicalExpr>,
        /// Build side (smaller, used to construct hash table).
        left: Box<PhysicalPlan>,
        /// Probe side (larger, streamed through the hash table).
        right: Box<PhysicalPlan>,
    },

    /// Nested-loop join (fallback for non-equi joins).
    NestedLoopJoin {
        /// Join type.
        join_type: JoinType,
        /// The join predicate.
        condition: PhysicalExpr,
        /// Outer (left) input.
        left: Box<PhysicalPlan>,
        /// Inner (right) input.
        right: Box<PhysicalPlan>,
    },

    /// Hash-based aggregation.
    HashAggregate {
        /// Indices of grouping key columns in the input.
        group_by: Vec<usize>,
        /// Aggregate operations.
        aggregates: Vec<AggOp>,
        /// Child operator.
        input: Box<PhysicalPlan>,
    },

    /// Sort the input by one or more keys.
    Sort {
        /// Sort specifications.
        keys: Vec<SortSpec>,
        /// Maximum rows to materialize (for top-N optimization).
        limit: Option<u64>,
        /// Child operator.
        input: Box<PhysicalPlan>,
    },

    /// Limit output rows with optional offset.
    Limit {
        /// Maximum number of rows.
        count: u64,
        /// Rows to skip.
        offset: u64,
        /// Child operator.
        input: Box<PhysicalPlan>,
    },

    /// Set union of two inputs.
    UnionAll {
        /// Left input.
        left: Box<PhysicalPlan>,
        /// Right input.
        right: Box<PhysicalPlan>,
    },
}

/// A scalar expression in the physical plan.
///
/// Unlike [`ra_core::Expr`], physical expressions use column indices
/// rather than names, and carry resolved data types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PhysicalExpr {
    /// Reference a column by index in the input row.
    ColumnIndex(usize),

    /// A literal constant.
    Literal(Literal),

    /// A binary operation.
    BinaryOp {
        /// The operator.
        op: BinOp,
        /// Left operand.
        left: Box<PhysicalExpr>,
        /// Right operand.
        right: Box<PhysicalExpr>,
    },

    /// A unary operation.
    UnaryOp {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<PhysicalExpr>,
    },

    /// A CAST expression.
    Cast {
        /// The expression to cast.
        expr: Box<PhysicalExpr>,
        /// Target data type.
        target: DataType,
    },
}

/// A literal value with a known type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// SQL NULL.
    Null,
    /// Boolean literal.
    Bool(bool),
    /// 64-bit integer literal.
    Int64(i64),
    /// 64-bit float literal.
    Float64(f64),
    /// UTF-8 string literal.
    Utf8(String),
}

/// An aggregate operation in a hash aggregate node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AggOp {
    /// The aggregate function.
    pub function: AggregateFunction,
    /// Column index of the argument (None for COUNT(*)).
    pub arg_index: Option<usize>,
    /// Whether DISTINCT is specified.
    pub distinct: bool,
}

/// A sort key specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortSpec {
    /// Column index to sort on.
    pub column_index: usize,
    /// Sort direction.
    pub direction: SortDirection,
    /// NULL ordering.
    pub nulls: NullOrdering,
}

impl PhysicalPlan {
    /// Return the direct child plans of this node.
    #[must_use]
    pub fn children(&self) -> Vec<&PhysicalPlan> {
        match self {
            Self::TableScan { .. } => vec![],
            Self::Filter { input, .. }
            | Self::Project { input, .. }
            | Self::HashAggregate { input, .. }
            | Self::Sort { input, .. }
            | Self::Limit { input, .. } => vec![input],
            Self::HashJoin { left, right, .. }
            | Self::NestedLoopJoin { left, right, .. }
            | Self::UnionAll { left, right, .. } => vec![left, right],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_column_index_found() {
        let schema = Schema::new(vec![
            ColumnDef {
                name: "id".into(),
                table: Some("users".into()),
                data_type: DataType::Int64,
                nullable: false,
            },
            ColumnDef {
                name: "name".into(),
                table: Some("users".into()),
                data_type: DataType::Utf8,
                nullable: true,
            },
        ]);
        let col = ColumnRef::qualified("users", "name");
        assert_eq!(schema.column_index(&col), Some(1));
    }

    #[test]
    fn schema_column_index_unqualified() {
        let schema = Schema::new(vec![ColumnDef {
            name: "id".into(),
            table: None,
            data_type: DataType::Int64,
            nullable: false,
        }]);
        let col = ColumnRef::new("id");
        assert_eq!(schema.column_index(&col), Some(0));
    }

    #[test]
    fn schema_column_index_not_found() {
        let schema = Schema::new(vec![]);
        let col = ColumnRef::new("missing");
        assert_eq!(schema.column_index(&col), None);
    }

    #[test]
    fn physical_plan_children_scan() {
        let scan = PhysicalPlan::TableScan {
            table: "t".into(),
            schema: Schema::new(vec![]),
            pushed_filter: None,
        };
        assert!(scan.children().is_empty());
    }

    #[test]
    fn physical_plan_children_filter() {
        let plan = PhysicalPlan::Filter {
            predicate: PhysicalExpr::Literal(Literal::Bool(true)),
            input: Box::new(PhysicalPlan::TableScan {
                table: "t".into(),
                schema: Schema::new(vec![]),
                pushed_filter: None,
            }),
        };
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn data_type_display() {
        assert_eq!(DataType::Int64.to_string(), "int64");
        assert_eq!(DataType::Utf8.to_string(), "utf8");
        assert_eq!(DataType::Bool.to_string(), "bool");
    }

    #[test]
    fn schema_len() {
        let schema = Schema::new(vec![
            ColumnDef {
                name: "a".into(),
                table: None,
                data_type: DataType::Int64,
                nullable: false,
            },
            ColumnDef {
                name: "b".into(),
                table: None,
                data_type: DataType::Utf8,
                nullable: true,
            },
        ]);
        assert_eq!(schema.len(), 2);
        assert!(!schema.is_empty());
    }

    #[test]
    fn serialize_roundtrip() {
        let plan = PhysicalPlan::Filter {
            predicate: PhysicalExpr::BinaryOp {
                op: BinOp::Gt,
                left: Box::new(PhysicalExpr::ColumnIndex(0)),
                right: Box::new(PhysicalExpr::Literal(
                    Literal::Int64(100),
                )),
            },
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: Schema::new(vec![ColumnDef {
                    name: "amount".into(),
                    table: None,
                    data_type: DataType::Int64,
                    nullable: false,
                }]),
                pushed_filter: None,
            }),
        };
        let json = serde_json::to_string(&plan)
            .expect("serialization should succeed");
        let deserialized: PhysicalPlan = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(plan, deserialized);
    }
}
