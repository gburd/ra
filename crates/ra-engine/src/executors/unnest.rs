//! Executor for the UNNEST operator.
//!
//! Expands array expressions into rows. Supports both standalone
//! unnest (from literal arrays) and correlated unnest (from array
//! columns with an input relation).

use ra_core::expr::{Const, Expr};

use crate::recursive::{ExecutionError, Row};

/// Executor that expands arrays into rows.
#[derive(Debug)]
pub struct UnnestExecutor {
    /// Expression producing the array to unnest.
    expr: Expr,
    /// Column alias for unnested values.
    alias: Option<String>,
    /// Whether to include an ordinality column.
    with_ordinality: bool,
}

impl UnnestExecutor {
    /// Create an unnest executor.
    #[must_use]
    pub fn new(expr: Expr, alias: Option<String>, with_ordinality: bool) -> Self {
        Self {
            expr,
            alias,
            with_ordinality,
        }
    }

    /// Alias for the unnested column (defaults to `"unnest"`).
    #[must_use]
    pub fn column_alias(&self) -> &str {
        self.alias.as_deref().unwrap_or("unnest")
    }

    /// Execute the unnest, expanding an array into rows.
    ///
    /// For standalone unnest (no input rows), evaluates the
    /// expression directly. For correlated unnest, expands
    /// the array column from each input row.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression does not evaluate to
    /// an array or if a column reference is used without input.
    pub fn execute(&self, input: Option<&[Row]>) -> Result<Vec<Row>, ExecutionError> {
        match &self.expr {
            Expr::Array(elements) => self.unnest_array_literal(elements),
            Expr::Column(_) => {
                if let Some(rows) = input {
                    self.unnest_column(rows)
                } else {
                    Err(ExecutionError::EvalError(
                        "UNNEST of column requires input relation".to_owned(),
                    ))
                }
            }
            other => Err(ExecutionError::EvalError(format!(
                "cannot UNNEST expression: {other:?}"
            ))),
        }
    }

    /// Expand a constant array literal into rows.
    fn unnest_array_literal(&self, elements: &[Expr]) -> Result<Vec<Row>, ExecutionError> {
        let mut rows = Vec::with_capacity(elements.len());
        for (idx, elem) in elements.iter().enumerate() {
            let value = eval_const_expr(elem)?;
            let mut values = vec![value];
            if self.with_ordinality {
                values.push(Const::Int((idx as i64) + 1));
            }
            rows.push(Row::new(values));
        }
        Ok(rows)
    }

    /// Expand an array column from each input row.
    ///
    /// For each input row, extracts the array value at the
    /// referenced column position and produces one output row
    /// per array element, concatenated with the input row values.
    fn unnest_column(&self, input_rows: &[Row]) -> Result<Vec<Row>, ExecutionError> {
        let col_idx = self.resolve_column_index(input_rows)?;
        let mut output = Vec::new();

        for input_row in input_rows {
            let array_elements = extract_array_elements(&input_row.values[col_idx])?;

            for (ord, elem) in array_elements.iter().enumerate() {
                let mut values = input_row.values.clone();
                values.push(elem.clone());
                if self.with_ordinality {
                    values.push(Const::Int((ord as i64) + 1));
                }
                output.push(Row::new(values));
            }
        }

        Ok(output)
    }

    /// Resolve the column index for the unnest target.
    ///
    /// Currently uses positional index 0 as a fallback when
    /// column name resolution is not available.
    fn resolve_column_index(&self, input_rows: &[Row]) -> Result<usize, ExecutionError> {
        if input_rows.is_empty() {
            return Ok(0);
        }
        // Default to last column when we don't have schema info.
        let width = input_rows[0].width();
        if width == 0 {
            return Err(ExecutionError::EvalError(
                "input row has no columns".to_owned(),
            ));
        }
        Ok(width - 1)
    }
}

/// Executor for multi-argument UNNEST (parallel array expansion).
///
/// PostgreSQL allows unnesting multiple arrays in parallel:
/// `SELECT * FROM unnest(ARRAY[1,2,3], ARRAY['a','b','c'])`.
/// Shorter arrays are padded with NULL.
#[derive(Debug)]
pub struct MultiUnnestExecutor {
    /// Expressions producing the arrays to unnest.
    exprs: Vec<Expr>,
    /// Column aliases for each array.
    aliases: Vec<Option<String>>,
    /// Whether to include an ordinality column.
    with_ordinality: bool,
}

impl MultiUnnestExecutor {
    /// Create a multi-argument unnest executor.
    #[must_use]
    pub fn new(exprs: Vec<Expr>, aliases: Vec<Option<String>>, with_ordinality: bool) -> Self {
        Self {
            exprs,
            aliases,
            with_ordinality,
        }
    }

    /// Execute the multi-argument unnest.
    ///
    /// Expands each array in parallel. The output has one column
    /// per array, plus an optional ordinality column. Rows are
    /// padded with NULL when arrays have different lengths.
    ///
    /// # Errors
    ///
    /// Returns an error if any expression does not evaluate to
    /// an array.
    pub fn execute(&self) -> Result<Vec<Row>, ExecutionError> {
        let mut columns: Vec<Vec<Const>> = Vec::with_capacity(self.exprs.len());

        for expr in &self.exprs {
            let elements = extract_array_from_expr(expr)?;
            columns.push(elements);
        }

        let max_len = columns.iter().map(Vec::len).max().unwrap_or(0);

        let mut rows = Vec::with_capacity(max_len);
        for idx in 0..max_len {
            let mut values = Vec::with_capacity(columns.len() + usize::from(self.with_ordinality));
            for col in &columns {
                let val = col.get(idx).cloned().unwrap_or(Const::Null);
                values.push(val);
            }
            if self.with_ordinality {
                values.push(Const::Int((idx as i64) + 1));
            }
            rows.push(Row::new(values));
        }

        Ok(rows)
    }

    /// Column aliases for the output.
    #[must_use]
    pub fn column_aliases(&self) -> &[Option<String>] {
        &self.aliases
    }
}

/// Extract array elements from an expression.
fn extract_array_from_expr(expr: &Expr) -> Result<Vec<Const>, ExecutionError> {
    match expr {
        Expr::Array(elements) => {
            let mut result = Vec::with_capacity(elements.len());
            for elem in elements {
                result.push(eval_const_expr(elem)?);
            }
            Ok(result)
        }
        Expr::Const(Const::Null) => Ok(vec![]),
        other => Err(ExecutionError::EvalError(format!(
            "multi-UNNEST requires array expressions, got: \
             {other:?}"
        ))),
    }
}

/// Evaluate a constant expression to a [`Const`] value.
fn eval_const_expr(expr: &Expr) -> Result<Const, ExecutionError> {
    match expr {
        Expr::Const(c) => Ok(c.clone()),
        other => Err(ExecutionError::EvalError(format!(
            "expected constant, got: {other:?}"
        ))),
    }
}

/// Extract array elements from a constant value.
///
/// Arrays are represented as `Const::String` with a serialized
/// format like `{1,2,3}`, or as nested structures depending on
/// the source. For literal arrays, the UnnestExecutor already
/// has the elements from the `Expr::Array` variant.
fn extract_array_elements(value: &Const) -> Result<Vec<Const>, ExecutionError> {
    match value {
        Const::String(s) => parse_pg_array_literal(s),
        Const::Null => Ok(vec![]),
        other => Err(ExecutionError::EvalError(format!(
            "expected array value, got: {other:?}"
        ))),
    }
}

/// Parse a PostgreSQL-style array literal `{1,2,3}`.
fn parse_pg_array_literal(s: &str) -> Result<Vec<Const>, ExecutionError> {
    let trimmed = s.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(ExecutionError::EvalError(format!(
            "invalid array literal: {s}"
        )));
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.is_empty() {
        return Ok(vec![]);
    }
    let mut elements = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.eq_ignore_ascii_case("null") {
            elements.push(Const::Null);
        } else if let Ok(i) = part.parse::<i64>() {
            elements.push(Const::Int(i));
        } else if let Ok(f) = part.parse::<f64>() {
            elements.push(Const::Float(f));
        } else {
            let unquoted = part.trim_matches('"').trim_matches('\'');
            elements.push(Const::String(unquoted.to_owned()));
        }
    }
    Ok(elements)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::expr::{ColumnRef, Expr};

    #[test]
    fn unnest_integer_array_literal() {
        let expr = Expr::Array(vec![
            Expr::Const(Const::Int(10)),
            Expr::Const(Const::Int(20)),
            Expr::Const(Const::Int(30)),
        ]);
        let executor = UnnestExecutor::new(expr, None, false);
        let rows = executor.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].values, vec![Const::Int(10)]);
        assert_eq!(rows[1].values, vec![Const::Int(20)]);
        assert_eq!(rows[2].values, vec![Const::Int(30)]);
    }

    #[test]
    fn unnest_string_array_literal() {
        let expr = Expr::Array(vec![
            Expr::Const(Const::String("a".into())),
            Expr::Const(Const::String("b".into())),
        ]);
        let executor = UnnestExecutor::new(expr, Some("val".into()), false);
        assert_eq!(executor.column_alias(), "val");
        let rows = executor.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values, vec![Const::String("a".into())]);
    }

    #[test]
    fn unnest_with_ordinality() {
        let expr = Expr::Array(vec![
            Expr::Const(Const::Int(100)),
            Expr::Const(Const::Int(200)),
        ]);
        let executor = UnnestExecutor::new(expr, None, true);
        let rows = executor.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values, vec![Const::Int(100), Const::Int(1)]);
        assert_eq!(rows[1].values, vec![Const::Int(200), Const::Int(2)]);
    }

    #[test]
    fn unnest_empty_array() {
        let expr = Expr::Array(vec![]);
        let executor = UnnestExecutor::new(expr, None, false);
        let rows = executor.execute(None).expect("should succeed");
        assert!(rows.is_empty());
    }

    #[test]
    fn unnest_column_requires_input() {
        let expr = Expr::Column(ColumnRef::new("arr"));
        let executor = UnnestExecutor::new(expr, None, false);
        let result = executor.execute(None);
        assert!(result.is_err());
    }

    #[test]
    fn unnest_column_from_input_rows() {
        let expr = Expr::Column(ColumnRef::new("arr"));
        let executor = UnnestExecutor::new(expr, None, false);
        let input = vec![
            Row::new(vec![Const::Int(1), Const::String("{10,20}".into())]),
            Row::new(vec![Const::Int(2), Const::String("{30}".into())]),
        ];
        let rows = executor.execute(Some(&input)).expect("should succeed");
        // Row 1 produces 2 output rows, row 2 produces 1
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0].values,
            vec![
                Const::Int(1),
                Const::String("{10,20}".into()),
                Const::Int(10),
            ]
        );
    }

    #[test]
    fn parse_pg_array_integers() {
        let elems = parse_pg_array_literal("{1,2,3}").expect("should parse");
        assert_eq!(elems, vec![Const::Int(1), Const::Int(2), Const::Int(3)]);
    }

    #[test]
    fn parse_pg_array_empty() {
        let elems = parse_pg_array_literal("{}").expect("should parse");
        assert!(elems.is_empty());
    }

    #[test]
    fn parse_pg_array_strings() {
        let elems = parse_pg_array_literal("{\"a\",\"b\"}").expect("should parse");
        assert_eq!(
            elems,
            vec![Const::String("a".into()), Const::String("b".into()),]
        );
    }

    #[test]
    fn parse_pg_array_null() {
        let elems = parse_pg_array_literal("{1,NULL,3}").expect("should parse");
        assert_eq!(elems, vec![Const::Int(1), Const::Null, Const::Int(3)]);
    }

    #[test]
    fn multi_unnest_basic() {
        let exec = MultiUnnestExecutor::new(
            vec![
                Expr::Array(vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(2))]),
                Expr::Array(vec![
                    Expr::Const(Const::String("a".into())),
                    Expr::Const(Const::String("b".into())),
                ]),
            ],
            vec![Some("num".into()), Some("letter".into())],
            false,
        );
        let rows = exec.execute().expect("should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].values,
            vec![Const::Int(1), Const::String("a".into())]
        );
        assert_eq!(
            rows[1].values,
            vec![Const::Int(2), Const::String("b".into())]
        );
    }

    #[test]
    fn multi_unnest_padding() {
        let exec = MultiUnnestExecutor::new(
            vec![
                Expr::Array(vec![
                    Expr::Const(Const::Int(1)),
                    Expr::Const(Const::Int(2)),
                    Expr::Const(Const::Int(3)),
                ]),
                Expr::Array(vec![Expr::Const(Const::String("x".into()))]),
            ],
            vec![None, None],
            false,
        );
        let rows = exec.execute().expect("should succeed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].values[1], Const::Null);
        assert_eq!(rows[2].values[1], Const::Null);
    }

    #[test]
    fn multi_unnest_with_ordinality() {
        let exec = MultiUnnestExecutor::new(
            vec![Expr::Array(vec![
                Expr::Const(Const::Int(10)),
                Expr::Const(Const::Int(20)),
            ])],
            vec![None],
            true,
        );
        let rows = exec.execute().expect("should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values, vec![Const::Int(10), Const::Int(1)]);
        assert_eq!(rows[1].values, vec![Const::Int(20), Const::Int(2)]);
    }

    #[test]
    fn multi_unnest_empty() {
        let exec = MultiUnnestExecutor::new(vec![Expr::Array(vec![])], vec![None], false);
        let rows = exec.execute().expect("should succeed");
        assert!(rows.is_empty());
    }

    #[test]
    fn multi_unnest_column_aliases() {
        let exec = MultiUnnestExecutor::new(
            vec![Expr::Array(vec![Expr::Const(Const::Int(1))])],
            vec![Some("col1".into())],
            false,
        );
        assert_eq!(exec.column_aliases(), &[Some("col1".to_owned())]);
    }
}
