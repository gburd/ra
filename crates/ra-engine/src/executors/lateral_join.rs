//! Executor for lateral joins with table-valued functions.
//!
//! A lateral join evaluates the right-hand side (a table-valued
//! function or UNNEST) once per row from the left-hand side,
//! passing the left row as context. The output is the cross
//! product of each left row with the corresponding right rows.

use ra_core::expr::Const;

use crate::recursive::{ExecutionError, Row};

use super::table_function::TableFunctionExecutor;
use super::unnest::UnnestExecutor;

/// The right-hand side of a lateral join.
#[derive(Debug)]
pub enum LateralRhs {
    /// Unnest an array column from the left row.
    Unnest(UnnestExecutor),
    /// Execute a table-valued function with left row context.
    TableFunction(TableFunctionExecutor),
}

/// Executor for lateral (correlated) joins.
///
/// For each left-side row, evaluates the right-hand table-valued
/// function and concatenates the results.
#[derive(Debug)]
pub struct LateralJoinExecutor {
    /// Right-hand side expression.
    rhs: LateralRhs,
    /// Whether to preserve left rows with no right matches
    /// (LEFT LATERAL JOIN semantics).
    preserve_left: bool,
}

impl LateralJoinExecutor {
    /// Create a lateral join executor.
    #[must_use]
    pub fn new(rhs: LateralRhs, preserve_left: bool) -> Self {
        Self { rhs, preserve_left }
    }

    /// Execute the lateral join.
    ///
    /// For each row in `left_rows`, evaluates the right-hand
    /// side and produces combined output rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the right-hand side evaluation fails.
    pub fn execute(&self, left_rows: &[Row]) -> Result<Vec<Row>, ExecutionError> {
        let mut output = Vec::new();

        for left_row in left_rows {
            let right_rows = self.evaluate_rhs(left_row)?;

            if right_rows.is_empty() && self.preserve_left {
                // LEFT LATERAL: emit left row with NULLs
                let null_width = self.rhs_width();
                let mut combined = left_row.values.clone();
                for _ in 0..null_width {
                    combined.push(Const::Null);
                }
                output.push(Row::new(combined));
            } else {
                for right_row in &right_rows {
                    let mut combined = left_row.values.clone();
                    combined.extend(right_row.values.iter().cloned());
                    output.push(Row::new(combined));
                }
            }
        }

        Ok(output)
    }

    /// Evaluate the right-hand side for a single left row.
    fn evaluate_rhs(&self, left_row: &Row) -> Result<Vec<Row>, ExecutionError> {
        let input_slice = std::slice::from_ref(left_row);
        match &self.rhs {
            LateralRhs::Unnest(executor) => {
                let rows = executor.execute(Some(input_slice))?;
                // Strip the input columns from unnest output
                // (they were appended by the unnest executor).
                let left_width = left_row.width();
                let trimmed: Vec<Row> = rows
                    .into_iter()
                    .map(|r| Row::new(r.values[left_width..].to_vec()))
                    .collect();
                Ok(trimmed)
            }
            LateralRhs::TableFunction(executor) => executor.execute(Some(input_slice)),
        }
    }

    /// Expected output width from the right-hand side.
    fn rhs_width(&self) -> usize {
        match &self.rhs {
            LateralRhs::Unnest(_) | LateralRhs::TableFunction(_) => 1,
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::expr::{ColumnRef, Expr};

    fn make_unnest_executor() -> UnnestExecutor {
        UnnestExecutor::new(
            Expr::Column(ColumnRef::new("arr")),
            Some("val".into()),
            false,
        )
    }

    #[test]
    fn lateral_unnest_basic() {
        let executor = LateralJoinExecutor::new(LateralRhs::Unnest(make_unnest_executor()), false);
        let left = vec![
            Row::new(vec![Const::Int(1), Const::String("{10,20}".into())]),
            Row::new(vec![Const::Int(2), Const::String("{30}".into())]),
        ];
        let rows = executor.execute(&left).expect("should succeed");
        // Row 1 expands to 2 rows, Row 2 to 1 row
        assert_eq!(rows.len(), 3);
        // First combined row: [1, {10,20}] + [10]
        assert_eq!(rows[0].values.len(), 3);
        assert_eq!(rows[0].values[0], Const::Int(1));
        assert_eq!(rows[0].values[2], Const::Int(10));
    }

    #[test]
    fn lateral_unnest_left_join_empty_array() {
        let executor = LateralJoinExecutor::new(
            LateralRhs::Unnest(make_unnest_executor()),
            true, // preserve_left
        );
        let left = vec![Row::new(vec![
            Const::Int(1),
            Const::Null, // NULL array
        ])];
        let rows = executor.execute(&left).expect("should succeed");
        // LEFT JOIN: should emit left row with NULL
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].values[0], Const::Int(1));
        assert_eq!(rows[0].values[2], Const::Null);
    }

    #[test]
    fn lateral_table_function() {
        let tvf = TableFunctionExecutor::new(
            "generate_series",
            vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(3))],
        );
        let executor = LateralJoinExecutor::new(LateralRhs::TableFunction(tvf), false);
        let left = vec![
            Row::new(vec![Const::String("a".into())]),
            Row::new(vec![Const::String("b".into())]),
        ];
        let rows = executor.execute(&left).expect("should succeed");
        // Each left row gets 3 right rows
        assert_eq!(rows.len(), 6);
        assert_eq!(
            rows[0].values,
            vec![Const::String("a".into()), Const::Int(1)]
        );
    }

    #[test]
    fn lateral_no_left_rows() {
        let tvf = TableFunctionExecutor::new(
            "generate_series",
            vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(3))],
        );
        let executor = LateralJoinExecutor::new(LateralRhs::TableFunction(tvf), false);
        let rows = executor.execute(&[]).expect("should succeed");
        assert!(rows.is_empty());
    }
}
