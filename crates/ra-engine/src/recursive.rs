//! Recursive CTE execution via fixpoint iteration.
//!
//! Implements the SQL standard semantics for `WITH RECURSIVE`:
//! 1. Execute the base case (anchor member) once
//! 2. Iteratively execute the recursive case, binding the CTE
//!    name to the working table from the previous iteration
//! 3. Stop when a fixpoint is reached (no new rows), the maximum
//!    iteration count is hit, or a cycle is detected

use std::collections::{HashMap, HashSet};
use std::fmt;

use ra_core::algebra::RelExpr;
use ra_core::expr::Const;

/// A single row of data, represented as a vector of constants.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Column values in positional order.
    pub values: Vec<Const>,
}

impl Eq for Row {}

impl std::hash::Hash for Row {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.values.len().hash(state);
        for v in &self.values {
            std::mem::discriminant(v).hash(state);
            match v {
                Const::Null => {}
                Const::Bool(b) => b.hash(state),
                Const::Int(i) => i.hash(state),
                Const::Float(f) => f.to_bits().hash(state),
                Const::String(s) => s.hash(state),
            }
        }
    }
}

impl Row {
    /// Create a row from a vector of values.
    #[must_use]
    pub fn new(values: Vec<Const>) -> Self {
        Self { values }
    }

    /// Number of columns.
    #[must_use]
    pub fn width(&self) -> usize {
        self.values.len()
    }
}

/// Errors during recursive CTE execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    /// The expression evaluator returned an error.
    #[error("evaluation error: {0}")]
    EvalError(String),

    /// The base case and recursive case produce incompatible schemas.
    #[error("schema mismatch: base has {base} columns, recursive has {recursive}")]
    SchemaMismatch {
        /// Column count from base case.
        base: usize,
        /// Column count from recursive case.
        recursive: usize,
    },

    /// CTE name was not bound in the execution context.
    #[error("unbound CTE reference: {0}")]
    UnboundCTE(String),
}

/// Why the recursive iteration terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationReason {
    /// No new rows were produced (empty working table).
    Fixpoint,
    /// The configured maximum iteration count was reached.
    MaxIterations,
    /// A cycle was detected in the output tuples.
    CycleDetected,
}

impl fmt::Display for TerminationReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixpoint => write!(f, "fixpoint"),
            Self::MaxIterations => write!(f, "max iterations"),
            Self::CycleDetected => write!(f, "cycle detected"),
        }
    }
}

/// The result of executing a recursive CTE.
#[derive(Debug, Clone)]
pub struct RecursionResult {
    /// All accumulated rows across all iterations.
    pub rows: Vec<Row>,
    /// Number of iterations executed.
    pub iterations: u32,
    /// Reason the iteration stopped.
    pub terminated_by: TerminationReason,
}

/// Execution context carrying CTE name bindings.
///
/// During recursive CTE evaluation each iteration binds
/// the CTE name to the working table so the recursive case
/// can reference it via a Scan on the CTE name.
#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    cte_bindings: HashMap<String, Vec<Row>>,
}

impl ExecutionContext {
    /// Create an empty context with no bindings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a CTE name to a set of rows.
    pub fn bind_cte(&mut self, name: &str, rows: Vec<Row>) {
        self.cte_bindings.insert(name.to_owned(), rows);
    }

    /// Look up the rows bound to a CTE name.
    #[must_use]
    pub fn get_cte(&self, name: &str) -> Option<&[Row]> {
        self.cte_bindings.get(name).map(Vec::as_slice)
    }

    /// Check whether a CTE name is bound.
    #[must_use]
    pub fn has_cte(&self, name: &str) -> bool {
        self.cte_bindings.contains_key(name)
    }
}

/// Trait for evaluating a relational expression to rows.
///
/// Implementors provide the concrete execution logic. The
/// recursive CTE executor delegates expression evaluation
/// to this trait, handling only the fixpoint iteration loop.
pub trait ExprEvaluator {
    /// Evaluate a relational expression in the given context.
    ///
    /// # Errors
    ///
    /// Returns an error if evaluation fails.
    fn evaluate(&self, expr: &RelExpr, ctx: &ExecutionContext) -> Result<Vec<Row>, ExecutionError>;
}

/// Configuration for the recursive CTE executor.
#[derive(Debug, Clone)]
pub struct RecursiveCTEConfig {
    /// Maximum number of iterations before stopping.
    pub max_iterations: u32,
    /// Whether to track seen tuples and skip duplicates.
    pub cycle_detection: bool,
}

impl Default for RecursiveCTEConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            cycle_detection: true,
        }
    }
}

/// Executor for recursive CTEs using fixpoint iteration.
///
/// Follows the SQL standard semantics:
/// 1. Evaluate the base case to produce the initial working table
/// 2. Repeatedly evaluate the recursive case with the CTE bound
///    to the current working table
/// 3. Accumulate all rows; stop on fixpoint, max iterations, or cycle
#[derive(Debug)]
pub struct RecursiveCTEExecutor {
    config: RecursiveCTEConfig,
}

impl RecursiveCTEExecutor {
    /// Create an executor with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: RecursiveCTEConfig::default(),
        }
    }

    /// Create an executor with the given configuration.
    #[must_use]
    pub fn with_config(config: RecursiveCTEConfig) -> Self {
        Self { config }
    }

    /// Set the maximum iteration count.
    #[must_use]
    pub fn max_iterations(mut self, max: u32) -> Self {
        self.config.max_iterations = max;
        self
    }

    /// Enable or disable cycle detection.
    #[must_use]
    pub fn cycle_detection(mut self, enabled: bool) -> Self {
        self.config.cycle_detection = enabled;
        self
    }

    /// Execute a recursive CTE using fixpoint iteration.
    ///
    /// # Arguments
    ///
    /// * `base_case` - The anchor member expression
    /// * `recursive_case` - The recursive member expression
    /// * `cte_name` - The name the recursive case references
    /// * `evaluator` - Expression evaluator for executing subplans
    ///
    /// # Errors
    ///
    /// Returns an error if expression evaluation fails or if
    /// the base and recursive schemas are incompatible.
    pub fn execute_fixpoint(
        &self,
        base_case: &RelExpr,
        recursive_case: &RelExpr,
        cte_name: &str,
        evaluator: &dyn ExprEvaluator,
    ) -> Result<RecursionResult, ExecutionError> {
        let ctx = ExecutionContext::new();

        // Step 1: evaluate the base case (anchor member)
        let mut working_table = evaluator.evaluate(base_case, &ctx)?;
        let mut result = working_table.clone();

        // Validate schema compatibility on first iteration
        let base_width = working_table.first().map_or(0, Row::width);

        let mut seen: Option<HashSet<Row>> = if self.config.cycle_detection {
            let mut set = HashSet::new();
            for row in &working_table {
                set.insert(row.clone());
            }
            Some(set)
        } else {
            None
        };

        // Step 2: iterate until fixpoint
        for iteration in 0..self.config.max_iterations {
            if working_table.is_empty() {
                return Ok(RecursionResult {
                    rows: result,
                    iterations: iteration,
                    terminated_by: TerminationReason::Fixpoint,
                });
            }

            // Bind CTE name to the current working table
            let mut iter_ctx = ExecutionContext::new();
            iter_ctx.bind_cte(cte_name, working_table);

            // Evaluate the recursive case
            let delta = evaluator.evaluate(recursive_case, &iter_ctx)?;

            // Validate schema
            if base_width > 0 {
                if let Some(first) = delta.first() {
                    if first.width() != base_width {
                        return Err(ExecutionError::SchemaMismatch {
                            base: base_width,
                            recursive: first.width(),
                        });
                    }
                }
            }

            // Filter out duplicates if cycle detection is on
            let new_tuples: Vec<Row> = if let Some(ref mut set) = seen {
                delta
                    .into_iter()
                    .filter(|row| set.insert(row.clone()))
                    .collect()
            } else {
                delta
            };

            if new_tuples.is_empty() {
                return Ok(RecursionResult {
                    rows: result,
                    iterations: iteration + 1,
                    terminated_by: TerminationReason::Fixpoint,
                });
            }

            result.extend(new_tuples.clone());
            working_table = new_tuples;
        }

        Ok(RecursionResult {
            rows: result,
            iterations: self.config.max_iterations,
            terminated_by: TerminationReason::MaxIterations,
        })
    }
}

impl Default for RecursiveCTEExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::Const;

    /// A mock evaluator that simulates counting from 1 to N.
    /// Base case: returns row [1].
    /// Recursive case: for each row [n] in working table,
    ///   returns [n+1] if n < limit.
    struct CountingEvaluator {
        limit: i64,
    }

    impl ExprEvaluator for CountingEvaluator {
        fn evaluate(
            &self,
            expr: &RelExpr,
            ctx: &ExecutionContext,
        ) -> Result<Vec<Row>, ExecutionError> {
            match expr {
                // Base case: return [1]
                RelExpr::Scan { table, .. } if !ctx.has_cte(table) => {
                    Ok(vec![Row::new(vec![Const::Int(1)])])
                }
                // Recursive case: increment each row
                RelExpr::Scan { table, .. } => {
                    let rows = ctx
                        .get_cte(table)
                        .ok_or_else(|| ExecutionError::UnboundCTE(table.clone()))?;
                    let mut out = Vec::new();
                    for row in rows {
                        if let Some(Const::Int(n)) = row.values.first() {
                            if *n < self.limit {
                                out.push(Row::new(vec![Const::Int(n + 1)]));
                            }
                        }
                    }
                    Ok(out)
                }
                _ => Ok(Vec::new()),
            }
        }
    }

    /// A mock evaluator that always produces the same delta,
    /// simulating an infinite loop.
    struct InfiniteEvaluator;

    impl ExprEvaluator for InfiniteEvaluator {
        fn evaluate(
            &self,
            _expr: &RelExpr,
            _ctx: &ExecutionContext,
        ) -> Result<Vec<Row>, ExecutionError> {
            Ok(vec![Row::new(vec![Const::Int(1)])])
        }
    }

    /// Evaluator that returns nothing (fixpoint on first iteration).
    struct EmptyEvaluator {
        first_call: std::cell::Cell<bool>,
    }

    impl EmptyEvaluator {
        fn new() -> Self {
            Self {
                first_call: std::cell::Cell::new(true),
            }
        }
    }

    impl ExprEvaluator for EmptyEvaluator {
        fn evaluate(
            &self,
            _expr: &RelExpr,
            _ctx: &ExecutionContext,
        ) -> Result<Vec<Row>, ExecutionError> {
            if self.first_call.get() {
                self.first_call.set(false);
                Ok(vec![Row::new(vec![Const::Int(42)])])
            } else {
                Ok(Vec::new())
            }
        }
    }

    #[test]
    fn counting_to_five() {
        let executor = RecursiveCTEExecutor::new();
        let evaluator = CountingEvaluator { limit: 5 };
        let base = RelExpr::scan("counter");
        let recursive = RelExpr::scan("counter");

        let result = executor
            .execute_fixpoint(&base, &recursive, "counter", &evaluator)
            .expect("execution should succeed");

        assert_eq!(result.rows.len(), 5);
        assert_eq!(result.terminated_by, TerminationReason::Fixpoint);

        let values: Vec<i64> = result
            .rows
            .iter()
            .filter_map(|r| match r.values.first() {
                Some(Const::Int(n)) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn infinite_loop_hits_max_iterations() {
        let executor = RecursiveCTEExecutor::new()
            .max_iterations(10)
            .cycle_detection(false);
        let evaluator = InfiniteEvaluator;
        let base = RelExpr::scan("inf");
        let recursive = RelExpr::scan("inf");

        let result = executor
            .execute_fixpoint(&base, &recursive, "inf", &evaluator)
            .expect("execution should succeed");

        assert_eq!(result.terminated_by, TerminationReason::MaxIterations);
        assert_eq!(result.iterations, 10);
    }

    #[test]
    fn cycle_detection_stops_duplicates() {
        let executor = RecursiveCTEExecutor::new()
            .max_iterations(50)
            .cycle_detection(true);
        let evaluator = InfiniteEvaluator;
        let base = RelExpr::scan("cycle");
        let recursive = RelExpr::scan("cycle");

        let result = executor
            .execute_fixpoint(&base, &recursive, "cycle", &evaluator)
            .expect("execution should succeed");

        // Cycle detection filters out duplicate [1] rows,
        // so the second iteration produces nothing new -> fixpoint
        assert_eq!(result.terminated_by, TerminationReason::Fixpoint);
        assert_eq!(result.rows.len(), 1);
    }

    #[test]
    fn empty_recursive_case_terminates_immediately() {
        let executor = RecursiveCTEExecutor::new();
        let evaluator = EmptyEvaluator::new();
        let base = RelExpr::scan("empty");
        let recursive = RelExpr::scan("empty");

        let result = executor
            .execute_fixpoint(&base, &recursive, "empty", &evaluator)
            .expect("execution should succeed");

        assert_eq!(result.terminated_by, TerminationReason::Fixpoint);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.iterations, 1);
    }

    #[test]
    fn default_config_values() {
        let config = RecursiveCTEConfig::default();
        assert_eq!(config.max_iterations, 100);
        assert!(config.cycle_detection);
    }

    #[test]
    fn execution_context_bind_and_lookup() {
        let mut ctx = ExecutionContext::new();
        assert!(!ctx.has_cte("test"));
        assert!(ctx.get_cte("test").is_none());

        ctx.bind_cte(
            "test",
            vec![Row::new(vec![Const::Int(1), Const::String("a".into())])],
        );

        assert!(ctx.has_cte("test"));
        let rows = ctx.get_cte("test").expect("should be bound");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].width(), 2);
    }

    #[test]
    fn row_equality_and_hashing() {
        let r1 = Row::new(vec![Const::Int(1), Const::String("x".into())]);
        let r2 = Row::new(vec![Const::Int(1), Const::String("x".into())]);
        let r3 = Row::new(vec![Const::Int(2)]);

        assert_eq!(r1, r2);
        assert_ne!(r1, r3);

        let mut set = HashSet::new();
        set.insert(r1.clone());
        assert!(set.contains(&r2));
        assert!(!set.contains(&r3));
    }

    #[test]
    fn termination_reason_display() {
        assert_eq!(TerminationReason::Fixpoint.to_string(), "fixpoint");
        assert_eq!(
            TerminationReason::MaxIterations.to_string(),
            "max iterations"
        );
        assert_eq!(
            TerminationReason::CycleDetected.to_string(),
            "cycle detected"
        );
    }
}
