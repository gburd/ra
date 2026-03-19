//! Volcano-style (iterator model) query execution engine.
//!
//! Each operator implements [`Operator`], a pull-based iterator that
//! produces one row at a time via [`Operator::next`]. The engine
//! compiles a [`PhysicalPlan`] into a tree of operators and pulls
//! rows from the root.

use std::collections::HashMap;

use crate::ir::{
    AggOp, Literal, PhysicalExpr, PhysicalPlan, Schema, SortSpec,
};
use ra_core::{AggregateFunction, BinOp, JoinType, UnaryOp};
use thiserror::Error;

/// A runtime value produced by expression evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// SQL NULL.
    Null,
    /// Boolean.
    Bool(bool),
    /// 64-bit signed integer.
    Int64(i64),
    /// 64-bit float.
    Float64(f64),
    /// UTF-8 string.
    Utf8(String),
}

impl Value {
    /// Return `true` if this value is truthy for filter evaluation.
    fn is_truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Null => false,
            Self::Int64(i) => *i != 0,
            Self::Float64(f) => *f != 0.0,
            Self::Utf8(s) => !s.is_empty(),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float64(f) => Some(*f),
            Self::Int64(i) => Some(*i as f64),
            _ => None,
        }
    }
}

/// A row is a vector of values.
pub type Row = Vec<Value>;

/// Errors during query execution.
#[derive(Debug, Error)]
pub enum ExecError {
    /// A table referenced by the plan was not found.
    #[error("table not found: {0}")]
    TableNotFound(String),

    /// Column index out of bounds.
    #[error("column index {index} out of bounds (row has {width} columns)")]
    ColumnOutOfBounds {
        /// The requested column index.
        index: usize,
        /// Actual row width.
        width: usize,
    },

    /// Type error during expression evaluation.
    #[error("type error in {op}: cannot apply to {left:?} and {right:?}")]
    TypeError {
        /// The operation that failed.
        op: String,
        /// Left operand value.
        left: Value,
        /// Right operand value.
        right: Value,
    },

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

/// A table data source providing in-memory rows.
#[derive(Debug, Clone)]
pub struct TableData {
    /// The table schema.
    pub schema: Schema,
    /// The rows.
    pub rows: Vec<Row>,
}

/// A catalog mapping table names to their data.
pub type Catalog = HashMap<String, TableData>;

/// Evaluate a physical expression against a row.
///
/// # Errors
///
/// Returns `ExecError` if a column index is out of bounds or a type
/// error occurs during evaluation.
pub fn eval_expr(
    expr: &PhysicalExpr,
    row: &Row,
) -> Result<Value, ExecError> {
    match expr {
        PhysicalExpr::ColumnIndex(idx) => {
            row.get(*idx).cloned().ok_or(
                ExecError::ColumnOutOfBounds {
                    index: *idx,
                    width: row.len(),
                },
            )
        }
        PhysicalExpr::Literal(lit) => Ok(literal_to_value(lit)),
        PhysicalExpr::BinaryOp { op, left, right } => {
            let lv = eval_expr(left, row)?;
            let rv = eval_expr(right, row)?;
            eval_binop(*op, &lv, &rv)
        }
        PhysicalExpr::UnaryOp { op, operand } => {
            let v = eval_expr(operand, row)?;
            eval_unaryop(*op, &v)
        }
        PhysicalExpr::Cast { expr, target } => {
            let v = eval_expr(expr, row)?;
            eval_cast(v, *target)
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

fn eval_binop(
    op: BinOp,
    left: &Value,
    right: &Value,
) -> Result<Value, ExecError> {
    if matches!(left, Value::Null) || matches!(right, Value::Null) {
        return match op {
            BinOp::And => {
                // NULL AND false = false; NULL AND true = NULL
                match (left, right) {
                    (Value::Bool(false), _)
                    | (_, Value::Bool(false)) => {
                        Ok(Value::Bool(false))
                    }
                    _ => Ok(Value::Null),
                }
            }
            BinOp::Or => {
                // NULL OR true = true; NULL OR false = NULL
                match (left, right) {
                    (Value::Bool(true), _)
                    | (_, Value::Bool(true)) => {
                        Ok(Value::Bool(true))
                    }
                    _ => Ok(Value::Null),
                }
            }
            _ => Ok(Value::Null),
        };
    }
    match op {
        BinOp::Add => numeric_op(left, right, |a, b| a + b, |a, b| a + b),
        BinOp::Sub => numeric_op(left, right, |a, b| a - b, |a, b| a - b),
        BinOp::Mul => numeric_op(left, right, |a, b| a * b, |a, b| a * b),
        BinOp::Div => numeric_op(left, right, |a, b| a / b, |a, b| a / b),
        BinOp::Mod => numeric_op(left, right, |a, b| a % b, |a, b| a % b),
        BinOp::Eq => Ok(Value::Bool(left == right)),
        BinOp::Ne => Ok(Value::Bool(left != right)),
        BinOp::Lt => compare_op(left, right, |o| {
            matches!(o, std::cmp::Ordering::Less)
        }),
        BinOp::Le => compare_op(left, right, |o| {
            matches!(
                o,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            )
        }),
        BinOp::Gt => compare_op(left, right, |o| {
            matches!(o, std::cmp::Ordering::Greater)
        }),
        BinOp::Ge => compare_op(left, right, |o| {
            matches!(
                o,
                std::cmp::Ordering::Greater
                    | std::cmp::Ordering::Equal
            )
        }),
        BinOp::And => {
            Ok(Value::Bool(left.is_truthy() && right.is_truthy()))
        }
        BinOp::Or => {
            Ok(Value::Bool(left.is_truthy() || right.is_truthy()))
        }
        BinOp::Concat => {
            // String concatenation
            match (left, right) {
                (Value::Utf8(l), Value::Utf8(r)) => {
                    Ok(Value::Utf8(format!("{}{}", l, r)))
                }
                _ => Ok(Value::Null),
            }
        }
        BinOp::JsonAccess => {
            // JSON field access - unimplemented for now
            Ok(Value::Null)
        }
    }
}

fn numeric_op(
    left: &Value,
    right: &Value,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value, ExecError> {
    match (left, right) {
        (Value::Int64(a), Value::Int64(b)) => {
            Ok(Value::Int64(int_op(*a, *b)))
        }
        (Value::Float64(a), Value::Float64(b)) => {
            Ok(Value::Float64(float_op(*a, *b)))
        }
        #[allow(clippy::cast_precision_loss)]
        (Value::Int64(a), Value::Float64(b)) => {
            Ok(Value::Float64(float_op(*a as f64, *b)))
        }
        #[allow(clippy::cast_precision_loss)]
        (Value::Float64(a), Value::Int64(b)) => {
            Ok(Value::Float64(float_op(*a, *b as f64)))
        }
        _ => Err(ExecError::TypeError {
            op: "arithmetic".into(),
            left: left.clone(),
            right: right.clone(),
        }),
    }
}

fn compare_op(
    left: &Value,
    right: &Value,
    check: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<Value, ExecError> {
    let ordering = match (left, right) {
        (Value::Int64(a), Value::Int64(b)) => a.cmp(b),
        (Value::Float64(a), Value::Float64(b)) => {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        }
        #[allow(clippy::cast_precision_loss)]
        (Value::Int64(a), Value::Float64(b)) => {
            (*a as f64)
                .partial_cmp(b)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        #[allow(clippy::cast_precision_loss)]
        (Value::Float64(a), Value::Int64(b)) => {
            a.partial_cmp(&(*b as f64))
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Utf8(a), Value::Utf8(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => {
            return Err(ExecError::TypeError {
                op: "comparison".into(),
                left: left.clone(),
                right: right.clone(),
            });
        }
    };
    Ok(Value::Bool(check(ordering)))
}

fn eval_unaryop(op: UnaryOp, val: &Value) -> Result<Value, ExecError> {
    match op {
        UnaryOp::Not => Ok(Value::Bool(!val.is_truthy())),
        UnaryOp::IsNull => Ok(Value::Bool(matches!(val, Value::Null))),
        UnaryOp::IsNotNull => {
            Ok(Value::Bool(!matches!(val, Value::Null)))
        }
        UnaryOp::Neg => match val {
            Value::Int64(i) => Ok(Value::Int64(-i)),
            Value::Float64(f) => Ok(Value::Float64(-f)),
            Value::Null => Ok(Value::Null),
            other => Err(ExecError::TypeError {
                op: "negation".into(),
                left: other.clone(),
                right: Value::Null,
            }),
        },
    }
}

fn eval_cast(
    val: Value,
    target: crate::ir::DataType,
) -> Result<Value, ExecError> {
    use crate::ir::DataType;
    match (&val, target) {
        (Value::Null, _) => Ok(Value::Null),
        #[allow(clippy::cast_precision_loss)]
        (Value::Int64(i), DataType::Float64) => {
            Ok(Value::Float64(*i as f64))
        }
        #[allow(clippy::cast_possible_truncation)]
        (Value::Float64(f), DataType::Int64) => {
            Ok(Value::Int64(*f as i64))
        }
        (Value::Int64(i), DataType::Utf8) => {
            Ok(Value::Utf8(i.to_string()))
        }
        (Value::Float64(f), DataType::Utf8) => {
            Ok(Value::Utf8(f.to_string()))
        }
        (Value::Bool(b), DataType::Utf8) => {
            Ok(Value::Utf8(b.to_string()))
        }
        (Value::Utf8(s), DataType::Int64) => {
            s.parse::<i64>().map(Value::Int64).map_err(|_| {
                ExecError::Internal(format!(
                    "cannot cast '{s}' to int64"
                ))
            })
        }
        _ => Ok(val),
    }
}

/// Execute a physical plan against an in-memory catalog.
///
/// Returns all result rows. For large datasets, use the streaming
/// [`Operator`] interface instead.
///
/// # Errors
///
/// Returns `ExecError` if a table is not found, a column index is
/// out of bounds, or a type error occurs during evaluation.
pub fn execute(
    plan: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    match plan {
        PhysicalPlan::TableScan {
            table,
            pushed_filter,
            ..
        } => execute_scan(table, pushed_filter.as_ref(), catalog),
        PhysicalPlan::Filter { predicate, input } => {
            execute_filter(predicate, input, catalog)
        }
        PhysicalPlan::Project { exprs, input, .. } => {
            execute_project(exprs, input, catalog)
        }
        PhysicalPlan::HashJoin {
            join_type, left_keys, right_keys,
            condition, left, right,
        } => execute_hash_join(
            *join_type, left_keys, right_keys,
            condition.as_ref(), left, right, catalog,
        ),
        PhysicalPlan::NestedLoopJoin {
            join_type, condition, left, right,
        } => execute_nested_loop_join(
            *join_type, condition, left, right, catalog,
        ),
        PhysicalPlan::HashAggregate {
            group_by, aggregates, input,
        } => execute_hash_aggregate(
            group_by, aggregates, input, catalog,
        ),
        PhysicalPlan::Sort { keys, limit, input } => {
            execute_sort(keys, *limit, input, catalog)
        }
        PhysicalPlan::Limit { count, offset, input } => {
            execute_limit(*count, *offset, input, catalog)
        }
        PhysicalPlan::UnionAll { left, right } => {
            let mut rows = execute(left, catalog)?;
            rows.extend(execute(right, catalog)?);
            Ok(rows)
        }
    }
}

fn execute_scan(
    table: &str,
    pushed_filter: Option<&PhysicalExpr>,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let data = catalog.get(table).ok_or_else(|| {
        ExecError::TableNotFound(table.to_owned())
    })?;
    let mut rows = data.rows.clone();
    if let Some(filter) = pushed_filter {
        rows.retain(|row| {
            eval_expr(filter, row)
                .map(|v| v.is_truthy())
                .unwrap_or(false)
        });
    }
    Ok(rows)
}

fn execute_filter(
    predicate: &PhysicalExpr,
    input: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let rows = execute(input, catalog)?;
    let mut result = Vec::new();
    for row in rows {
        if eval_expr(predicate, &row)?.is_truthy() {
            result.push(row);
        }
    }
    Ok(result)
}

fn execute_project(
    exprs: &[PhysicalExpr],
    input: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let rows = execute(input, catalog)?;
    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut out_row = Vec::with_capacity(exprs.len());
        for expr in exprs {
            out_row.push(eval_expr(expr, row)?);
        }
        result.push(out_row);
    }
    Ok(result)
}

fn execute_sort(
    keys: &[SortSpec],
    limit: Option<u64>,
    input: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let mut rows = execute(input, catalog)?;
    sort_rows(&mut rows, keys);
    if let Some(n) = limit {
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        rows.truncate(n);
    }
    Ok(rows)
}

fn execute_limit(
    count: u64,
    offset: u64,
    input: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let rows = execute(input, catalog)?;
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(rows.len());
    let end = usize::try_from(count)
        .unwrap_or(usize::MAX)
        .saturating_add(start)
        .min(rows.len());
    Ok(rows[start..end].to_vec())
}

fn extract_keys(row: &Row, indices: &[usize]) -> Vec<Value> {
    indices.iter().filter_map(|&i| row.get(i).cloned()).collect()
}

fn value_hash_key(values: &[Value]) -> String {
    values
        .iter()
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join("|")
}

fn execute_hash_join(
    join_type: JoinType,
    left_keys: &[usize],
    right_keys: &[usize],
    condition: Option<&PhysicalExpr>,
    left: &PhysicalPlan,
    right: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let left_rows = execute(left, catalog)?;
    let right_rows = execute(right, catalog)?;

    let mut hash_table: HashMap<String, Vec<Row>> = HashMap::new();
    for row in &left_rows {
        let key = value_hash_key(&extract_keys(row, left_keys));
        hash_table.entry(key).or_default().push(row.clone());
    }

    match join_type {
        JoinType::Inner | JoinType::Semi | JoinType::Anti => {
            join_inner_semi_anti(
                join_type, &hash_table, &right_rows,
                right_keys, condition,
            )
        }
        JoinType::LeftOuter => Ok(join_left_outer(
            &hash_table, &left_rows, &right_rows, right_keys,
        )),
        JoinType::Cross => {
            Ok(join_cross(&left_rows, &right_rows))
        }
        _ => Ok(join_full_or_right(
            join_type, &left_rows, &right_rows,
            left_keys, right_keys,
        )),
    }
}

fn join_inner_semi_anti(
    join_type: JoinType,
    hash_table: &HashMap<String, Vec<Row>>,
    right_rows: &[Row],
    right_keys: &[usize],
    condition: Option<&PhysicalExpr>,
) -> Result<Vec<Row>, ExecError> {
    let mut result = Vec::new();
    for right_row in right_rows {
        let key =
            value_hash_key(&extract_keys(right_row, right_keys));
        let matched = hash_table.get(&key);
        match join_type {
            JoinType::Inner => {
                if let Some(left_matches) = matched {
                    for left_row in left_matches {
                        let mut combined = left_row.clone();
                        combined.extend(right_row.clone());
                        if let Some(cond) = condition {
                            if eval_expr(cond, &combined)?
                                .is_truthy()
                            {
                                result.push(combined);
                            }
                        } else {
                            result.push(combined);
                        }
                    }
                }
            }
            JoinType::Semi => {
                if matched.is_some() {
                    result.push(right_row.clone());
                }
            }
            JoinType::Anti => {
                if matched.is_none() {
                    result.push(right_row.clone());
                }
            }
            _ => {}
        }
    }
    Ok(result)
}

fn join_left_outer(
    hash_table: &HashMap<String, Vec<Row>>,
    left_rows: &[Row],
    right_rows: &[Row],
    right_keys: &[usize],
) -> Vec<Row> {
    let right_width = right_rows.first().map_or(0, Vec::len);
    let mut left_matched = vec![false; left_rows.len()];
    let mut result = Vec::new();

    for right_row in right_rows {
        let key =
            value_hash_key(&extract_keys(right_row, right_keys));
        if let Some(left_matches) = hash_table.get(&key) {
            for left_row in left_matches {
                let idx = left_rows
                    .iter()
                    .position(|r| r == left_row)
                    .unwrap_or(0);
                left_matched[idx] = true;
                let mut combined = left_row.clone();
                combined.extend(right_row.clone());
                result.push(combined);
            }
        }
    }
    for (i, row) in left_rows.iter().enumerate() {
        if !left_matched[i] {
            let mut combined = row.clone();
            combined.extend(vec![Value::Null; right_width]);
            result.push(combined);
        }
    }
    result
}

fn join_cross(left_rows: &[Row], right_rows: &[Row]) -> Vec<Row> {
    let mut result = Vec::new();
    for left_row in left_rows {
        for right_row in right_rows {
            let mut combined = left_row.clone();
            combined.extend(right_row.clone());
            result.push(combined);
        }
    }
    result
}

fn join_full_or_right(
    join_type: JoinType,
    left_rows: &[Row],
    right_rows: &[Row],
    left_keys: &[usize],
    right_keys: &[usize],
) -> Vec<Row> {
    let left_width = left_rows.first().map_or(0, Vec::len);
    let null_left = vec![Value::Null; left_width];
    let right_width = right_rows.first().map_or(0, Vec::len);
    let null_right = vec![Value::Null; right_width];

    let mut result = Vec::new();
    let mut right_matched = vec![false; right_rows.len()];
    for left_row in left_rows {
        let mut any_match = false;
        for (j, right_row) in right_rows.iter().enumerate() {
            let lk = extract_keys(left_row, left_keys);
            let rk = extract_keys(right_row, right_keys);
            if lk == rk {
                any_match = true;
                right_matched[j] = true;
                let mut combined = left_row.clone();
                combined.extend(right_row.clone());
                result.push(combined);
            }
        }
        if !any_match
            && matches!(
                join_type,
                JoinType::FullOuter | JoinType::LeftOuter
            )
        {
            let mut combined = left_row.clone();
            combined.extend(null_right.clone());
            result.push(combined);
        }
    }
    if matches!(
        join_type,
        JoinType::FullOuter | JoinType::RightOuter
    ) {
        for (j, right_row) in right_rows.iter().enumerate() {
            if !right_matched[j] {
                let mut combined = null_left.clone();
                combined.extend(right_row.clone());
                result.push(combined);
            }
        }
    }
    result
}

fn execute_nested_loop_join(
    join_type: JoinType,
    condition: &PhysicalExpr,
    left: &PhysicalPlan,
    right: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let left_rows = execute(left, catalog)?;
    let right_rows = execute(right, catalog)?;
    let mut result = Vec::new();

    let left_width = left_rows.first().map_or(0, Vec::len);
    let right_width = right_rows.first().map_or(0, Vec::len);

    match join_type {
        JoinType::Inner | JoinType::Cross => {
            for left_row in &left_rows {
                for right_row in &right_rows {
                    let mut combined = left_row.clone();
                    combined.extend(right_row.clone());
                    if eval_expr(condition, &combined)?.is_truthy() {
                        result.push(combined);
                    }
                }
            }
        }
        JoinType::LeftOuter => {
            for left_row in &left_rows {
                let mut any_match = false;
                for right_row in &right_rows {
                    let mut combined = left_row.clone();
                    combined.extend(right_row.clone());
                    if eval_expr(condition, &combined)?.is_truthy() {
                        any_match = true;
                        result.push(combined);
                    }
                }
                if !any_match {
                    let mut combined = left_row.clone();
                    combined.extend(vec![Value::Null; right_width]);
                    result.push(combined);
                }
            }
        }
        _ => {
            // Fallback for other join types
            let null_left = vec![Value::Null; left_width];
            let mut right_matched =
                vec![false; right_rows.len()];
            for left_row in &left_rows {
                let mut any_match = false;
                for (j, right_row) in
                    right_rows.iter().enumerate()
                {
                    let mut combined = left_row.clone();
                    combined.extend(right_row.clone());
                    if eval_expr(condition, &combined)?.is_truthy() {
                        any_match = true;
                        right_matched[j] = true;
                        result.push(combined);
                    }
                }
                if !any_match
                    && matches!(
                        join_type,
                        JoinType::FullOuter | JoinType::LeftOuter
                    )
                {
                    let mut combined = left_row.clone();
                    combined.extend(
                        vec![Value::Null; right_width],
                    );
                    result.push(combined);
                }
            }
            if matches!(
                join_type,
                JoinType::FullOuter | JoinType::RightOuter
            ) {
                for (j, right_row) in
                    right_rows.iter().enumerate()
                {
                    if !right_matched[j] {
                        let mut combined = null_left.clone();
                        combined.extend(right_row.clone());
                        result.push(combined);
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Accumulator for aggregate state.
#[derive(Debug, Clone)]
enum AggState {
    Count(i64),
    Sum(f64),
    Min(Option<Value>),
    Max(Option<Value>),
    Avg { sum: f64, count: i64 },
}

fn init_agg_states(aggregates: &[AggOp]) -> Vec<AggState> {
    aggregates
        .iter()
        .map(|agg| match agg.function {
            AggregateFunction::Count => AggState::Count(0),
            AggregateFunction::Sum
            | AggregateFunction::StdDev
            | AggregateFunction::Variance => AggState::Sum(0.0),
            AggregateFunction::Min => AggState::Min(None),
            AggregateFunction::Max => AggState::Max(None),
            AggregateFunction::Avg => {
                AggState::Avg { sum: 0.0, count: 0 }
            }
            AggregateFunction::StringAgg
            | AggregateFunction::ArrayAgg => AggState::Count(0),
        })
        .collect()
}

fn accumulate_agg(
    state: &mut AggState,
    val: Option<Value>,
) {
    match state {
        AggState::Count(c) => {
            let is_countable = match val.as_ref() {
                None => true,
                Some(v) => !matches!(v, Value::Null),
            };
            if is_countable {
                *c += 1;
            }
        }
        AggState::Sum(s) => {
            if let Some(v) = val.as_ref().and_then(Value::as_f64)
            {
                *s += v;
            }
        }
        AggState::Min(current) => {
            if let Some(v) = val {
                if !matches!(v, Value::Null) {
                    *current = Some(match current.take() {
                        None => v,
                        Some(prev) => {
                            if compare_values(&v, &prev).is_lt() {
                                v
                            } else {
                                prev
                            }
                        }
                    });
                }
            }
        }
        AggState::Max(current) => {
            if let Some(v) = val {
                if !matches!(v, Value::Null) {
                    *current = Some(match current.take() {
                        None => v,
                        Some(prev) => {
                            if compare_values(&v, &prev).is_gt() {
                                v
                            } else {
                                prev
                            }
                        }
                    });
                }
            }
        }
        AggState::Avg { sum, count } => {
            if let Some(v) = val.as_ref().and_then(Value::as_f64)
            {
                *sum += v;
                *count += 1;
            }
        }
    }
}

fn execute_hash_aggregate(
    group_by: &[usize],
    aggregates: &[AggOp],
    input: &PhysicalPlan,
    catalog: &Catalog,
) -> Result<Vec<Row>, ExecError> {
    let rows = execute(input, catalog)?;

    let mut groups: HashMap<String, (Row, Vec<AggState>)> =
        HashMap::new();

    for row in &rows {
        let key_vals: Vec<Value> = group_by
            .iter()
            .filter_map(|&i| row.get(i).cloned())
            .collect();
        let key = value_hash_key(&key_vals);

        let entry = groups.entry(key).or_insert_with(|| {
            (key_vals.clone(), init_agg_states(aggregates))
        });

        for (i, agg) in aggregates.iter().enumerate() {
            let val = agg
                .arg_index
                .and_then(|idx| row.get(idx).cloned());
            accumulate_agg(&mut entry.1[i], val);
        }
    }

    if groups.is_empty() && group_by.is_empty() {
        groups.insert(
            String::new(),
            (vec![], init_agg_states(aggregates)),
        );
    }

    let mut result = Vec::with_capacity(groups.len());
    for (_, (key_vals, states)) in groups {
        let mut row = key_vals;
        for state in states {
            row.push(finalize_agg(state));
        }
        result.push(row);
    }

    Ok(result)
}

fn finalize_agg(state: AggState) -> Value {
    match state {
        AggState::Count(c) => Value::Int64(c),
        AggState::Sum(s) => Value::Float64(s),
        AggState::Min(v) | AggState::Max(v) => {
            v.unwrap_or(Value::Null)
        }
        #[allow(clippy::cast_precision_loss)]
        AggState::Avg { sum, count } => {
            if count == 0 {
                Value::Null
            } else {
                Value::Float64(sum / count as f64)
            }
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int64(x), Value::Int64(y)) => x.cmp(y),
        (Value::Float64(x), Value::Float64(y)) => {
            x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Int64(x), Value::Float64(y)) => {
            (*x as f64)
                .partial_cmp(y)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Float64(x), Value::Int64(y)) => {
            x.partial_cmp(&(*y as f64))
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Utf8(x), Value::Utf8(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => std::cmp::Ordering::Less,
        (_, Value::Null) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

fn sort_rows(rows: &mut [Row], keys: &[SortSpec]) {
    rows.sort_by(|a, b| {
        for spec in keys {
            let av = a.get(spec.column_index).cloned().unwrap_or(Value::Null);
            let bv = b.get(spec.column_index).cloned().unwrap_or(Value::Null);

            let ord = match (&av, &bv) {
                (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
                (Value::Null, _) => match spec.nulls {
                    ra_core::NullOrdering::First => {
                        std::cmp::Ordering::Less
                    }
                    ra_core::NullOrdering::Last => {
                        std::cmp::Ordering::Greater
                    }
                },
                (_, Value::Null) => match spec.nulls {
                    ra_core::NullOrdering::First => {
                        std::cmp::Ordering::Greater
                    }
                    ra_core::NullOrdering::Last => {
                        std::cmp::Ordering::Less
                    }
                },
                _ => compare_values(&av, &bv),
            };

            let ord = match spec.direction {
                ra_core::SortDirection::Asc => ord,
                ra_core::SortDirection::Desc => ord.reverse(),
            };

            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ColumnDef, DataType, Literal, PhysicalExpr};

    fn test_schema() -> Schema {
        Schema::new(vec![
            ColumnDef {
                name: "id".into(),
                table: None,
                data_type: DataType::Int64,
                nullable: false,
            },
            ColumnDef {
                name: "name".into(),
                table: None,
                data_type: DataType::Utf8,
                nullable: true,
            },
            ColumnDef {
                name: "amount".into(),
                table: None,
                data_type: DataType::Int64,
                nullable: false,
            },
        ])
    }

    fn test_catalog() -> Catalog {
        let mut catalog = Catalog::new();
        catalog.insert(
            "orders".into(),
            TableData {
                schema: test_schema(),
                rows: vec![
                    vec![
                        Value::Int64(1),
                        Value::Utf8("alice".into()),
                        Value::Int64(100),
                    ],
                    vec![
                        Value::Int64(2),
                        Value::Utf8("bob".into()),
                        Value::Int64(200),
                    ],
                    vec![
                        Value::Int64(3),
                        Value::Utf8("alice".into()),
                        Value::Int64(50),
                    ],
                ],
            },
        );
        catalog
    }

    #[test]
    fn execute_table_scan() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::TableScan {
            table: "orders".into(),
            schema: test_schema(),
            pushed_filter: None,
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn execute_table_scan_not_found() {
        let catalog = Catalog::new();
        let plan = PhysicalPlan::TableScan {
            table: "missing".into(),
            schema: Schema::new(vec![]),
            pushed_filter: None,
        };
        let err = execute(&plan, &catalog).unwrap_err();
        assert!(
            err.to_string().contains("table not found"),
            "got: {err}"
        );
    }

    #[test]
    fn execute_filter() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::Filter {
            predicate: PhysicalExpr::BinaryOp {
                op: BinOp::Gt,
                left: Box::new(PhysicalExpr::ColumnIndex(2)),
                right: Box::new(PhysicalExpr::Literal(
                    Literal::Int64(75),
                )),
            },
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 2); // 100 and 200
    }

    #[test]
    fn execute_project() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::Project {
            exprs: vec![
                PhysicalExpr::ColumnIndex(1),
                PhysicalExpr::BinaryOp {
                    op: BinOp::Mul,
                    left: Box::new(PhysicalExpr::ColumnIndex(2)),
                    right: Box::new(PhysicalExpr::Literal(
                        Literal::Int64(2),
                    )),
                },
            ],
            output_names: vec!["name".into(), "double_amount".into()],
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][1], Value::Int64(200));
    }

    #[test]
    fn execute_limit() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::Limit {
            count: 2,
            offset: 0,
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn execute_limit_with_offset() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::Limit {
            count: 1,
            offset: 1,
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], Value::Int64(2));
    }

    #[test]
    fn execute_sort() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::Sort {
            keys: vec![SortSpec {
                column_index: 2,
                direction: ra_core::SortDirection::Desc,
                nulls: ra_core::NullOrdering::Last,
            }],
            limit: None,
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows[0][2], Value::Int64(200));
        assert_eq!(rows[1][2], Value::Int64(100));
        assert_eq!(rows[2][2], Value::Int64(50));
    }

    #[test]
    fn execute_hash_aggregate_grouped() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::HashAggregate {
            group_by: vec![1], // group by name
            aggregates: vec![
                AggOp {
                    function: AggregateFunction::Sum,
                    arg_index: Some(2),
                    distinct: false,
                },
                AggOp {
                    function: AggregateFunction::Count,
                    arg_index: None,
                    distinct: false,
                },
            ],
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 2); // alice and bob

        for row in &rows {
            match &row[0] {
                Value::Utf8(name) if name == "alice" => {
                    assert_eq!(row[1], Value::Float64(150.0)); // 100+50
                    assert_eq!(row[2], Value::Int64(2));
                }
                Value::Utf8(name) if name == "bob" => {
                    assert_eq!(row[1], Value::Float64(200.0));
                    assert_eq!(row[2], Value::Int64(1));
                }
                other => {
                    panic!("unexpected group key: {other:?}")
                }
            }
        }
    }

    #[test]
    fn execute_hash_aggregate_scalar() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::HashAggregate {
            group_by: vec![],
            aggregates: vec![AggOp {
                function: AggregateFunction::Count,
                arg_index: None,
                distinct: false,
            }],
            input: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], Value::Int64(3));
    }

    #[test]
    fn execute_hash_join_inner() {
        let mut catalog = test_catalog();
        catalog.insert(
            "customers".into(),
            TableData {
                schema: Schema::new(vec![
                    ColumnDef {
                        name: "cid".into(),
                        table: None,
                        data_type: DataType::Int64,
                        nullable: false,
                    },
                    ColumnDef {
                        name: "cname".into(),
                        table: None,
                        data_type: DataType::Utf8,
                        nullable: false,
                    },
                ]),
                rows: vec![
                    vec![
                        Value::Int64(1),
                        Value::Utf8("Customer A".into()),
                    ],
                    vec![
                        Value::Int64(3),
                        Value::Utf8("Customer C".into()),
                    ],
                ],
            },
        );

        let plan = PhysicalPlan::HashJoin {
            join_type: JoinType::Inner,
            left_keys: vec![0],  // orders.id
            right_keys: vec![0], // customers.cid
            condition: None,
            left: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
            right: Box::new(PhysicalPlan::TableScan {
                table: "customers".into(),
                schema: Schema::new(vec![]),
                pushed_filter: None,
            }),
        };

        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        // Orders 1 and 3 match customers
        assert_eq!(rows.len(), 2);
        // Each result row has 5 columns (3 from orders + 2 from customers)
        assert_eq!(rows[0].len(), 5);
    }

    #[test]
    fn execute_union_all() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::UnionAll {
            left: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
            right: Box::new(PhysicalPlan::TableScan {
                table: "orders".into(),
                schema: test_schema(),
                pushed_filter: None,
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 6);
    }

    #[test]
    fn eval_null_propagation() {
        let result = eval_binop(BinOp::Add, &Value::Null, &Value::Int64(1))
            .expect("should not error");
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn eval_null_and_logic() {
        // NULL AND false = false
        let result =
            eval_binop(BinOp::And, &Value::Null, &Value::Bool(false))
                .expect("should not error");
        assert_eq!(result, Value::Bool(false));

        // NULL AND true = NULL
        let result =
            eval_binop(BinOp::And, &Value::Null, &Value::Bool(true))
                .expect("should not error");
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn execute_pushed_filter_scan() {
        let catalog = test_catalog();
        let plan = PhysicalPlan::TableScan {
            table: "orders".into(),
            schema: test_schema(),
            pushed_filter: Some(PhysicalExpr::BinaryOp {
                op: BinOp::Eq,
                left: Box::new(PhysicalExpr::ColumnIndex(0)),
                right: Box::new(PhysicalExpr::Literal(
                    Literal::Int64(2),
                )),
            }),
        };
        let rows = execute(&plan, &catalog)
            .expect("execution should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], Value::Int64(2));
    }
}
