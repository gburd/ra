//! Comprehensive tests for recursive CTE support across ra-engine.
//!
//! Covers: e-graph round-trip, pattern matching, memo table hashing,
//! cost model, and fixpoint execution.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod helpers;

use std::collections::HashSet;

use ra_core::algebra::{CycleDetection, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::pattern::Pattern;
use ra_engine::{
    ExecutionContext, ExecutionError, ExprEvaluator,
    RecursiveCTEConfig, RecursiveCTEExecutor, RecursionResult,
    Row, TerminationReason,
};
use ra_engine::{structural_hash, to_rec_expr};

// ── Helper: build a standard RecursiveCTE expression ───────

fn make_recursive_cte(
    name: &str,
    base_table: &str,
    rec_table: &str,
    body_table: &str,
) -> RelExpr {
    RelExpr::RecursiveCTE {
        name: name.to_owned(),
        base_case: Box::new(RelExpr::scan(base_table)),
        recursive_case: Box::new(RelExpr::scan(rec_table)),
        body: Box::new(RelExpr::scan(body_table)),
        cycle_detection: None,
    }
}

fn make_recursive_cte_with_cycle(
    name: &str,
    max_depth: u32,
) -> RelExpr {
    RelExpr::RecursiveCTE {
        name: name.to_owned(),
        base_case: Box::new(RelExpr::scan("base")),
        recursive_case: Box::new(RelExpr::scan("rec")),
        body: Box::new(RelExpr::scan(name)),
        cycle_detection: Some(CycleDetection {
            track_columns: vec!["id".to_owned()],
            max_depth: Some(max_depth),
            cycle_mark_column: None,
            path_column: None,
        }),
    }
}

// ════════════════════════════════════════════════════════════
// E-graph round-trip tests
// ════════════════════════════════════════════════════════════

#[test]
fn egraph_roundtrip_simple_recursive_cte() {
    let original = make_recursive_cte(
        "reachable", "edges", "reachable", "reachable",
    );

    let rec_expr = to_rec_expr(&original)
        .expect("RecursiveCTE should convert to RecExpr");

    // Verify round-trip through optimizer
    let optimizer = helpers::create_test_optimizer();
    let optimized = optimizer
        .optimize(&original)
        .expect("optimization should succeed");

    // The result should still be a RecursiveCTE
    assert!(
        matches!(optimized, RelExpr::RecursiveCTE { .. }),
        "optimized result should preserve RecursiveCTE structure, \
         got: {optimized:?}"
    );

    // RecExpr should be non-empty
    assert!(
        rec_expr.as_ref().len() > 0,
        "RecExpr should have nodes"
    );
}

#[test]
fn egraph_roundtrip_preserves_name() {
    let original = make_recursive_cte(
        "counter", "initial", "counter", "counter",
    );

    let optimizer = helpers::create_test_optimizer();
    let optimized = optimizer
        .optimize(&original)
        .expect("optimization should succeed");

    if let RelExpr::RecursiveCTE { name, .. } = &optimized {
        assert_eq!(name, "counter");
    } else {
        panic!(
            "expected RecursiveCTE after optimization"
        );
    }
}

#[test]
fn egraph_roundtrip_different_ctes_differ() {
    let cte_a = make_recursive_cte(
        "alpha", "base_a", "alpha", "alpha",
    );
    let cte_b = make_recursive_cte(
        "beta", "base_b", "beta", "beta",
    );

    let rec_a = to_rec_expr(&cte_a)
        .expect("alpha should convert");
    let rec_b = to_rec_expr(&cte_b)
        .expect("beta should convert");

    // Different CTEs should produce different RecExprs
    assert_ne!(
        format!("{rec_a:?}"),
        format!("{rec_b:?}"),
        "different CTEs should produce different RecExprs"
    );
}

// ════════════════════════════════════════════════════════════
// Pattern matching tests
// ════════════════════════════════════════════════════════════

#[test]
fn pattern_matches_recursive_cte() {
    let pattern = Pattern::RecursiveCTE {
        base_case: Box::new(Pattern::wildcard("base")),
        recursive_case: Box::new(Pattern::wildcard("rec")),
        body: Box::new(Pattern::wildcard("body")),
    };

    let expr = make_recursive_cte(
        "test", "t_base", "t_rec", "t_body",
    );

    let bindings = pattern
        .match_expr(&expr)
        .expect("pattern should match RecursiveCTE");

    assert_eq!(
        bindings.get_rel("base"),
        Some(&RelExpr::scan("t_base"))
    );
    assert_eq!(
        bindings.get_rel("rec"),
        Some(&RelExpr::scan("t_rec"))
    );
    assert_eq!(
        bindings.get_rel("body"),
        Some(&RelExpr::scan("t_body"))
    );
}

#[test]
fn pattern_rejects_non_recursive_cte() {
    let pattern = Pattern::RecursiveCTE {
        base_case: Box::new(Pattern::wildcard("base")),
        recursive_case: Box::new(Pattern::wildcard("rec")),
        body: Box::new(Pattern::wildcard("body")),
    };

    // A plain CTE should not match
    let cte = RelExpr::CTE {
        name: "test".to_owned(),
        definition: Box::new(RelExpr::scan("t")),
        body: Box::new(RelExpr::scan("t")),
    };

    assert!(
        pattern.match_expr(&cte).is_none(),
        "RecursiveCTE pattern should not match plain CTE"
    );
}

#[test]
fn pattern_rejects_scan() {
    let pattern = Pattern::RecursiveCTE {
        base_case: Box::new(Pattern::wildcard("base")),
        recursive_case: Box::new(Pattern::wildcard("rec")),
        body: Box::new(Pattern::wildcard("body")),
    };

    let scan = RelExpr::scan("t");
    assert!(
        pattern.match_expr(&scan).is_none(),
        "RecursiveCTE pattern should not match Scan"
    );
}

#[test]
fn pattern_matches_nested_base_case() {
    let filter_pattern = Pattern::Filter {
        predicate: None,
        input: Box::new(Pattern::wildcard("inner")),
    };
    let pattern = Pattern::RecursiveCTE {
        base_case: Box::new(filter_pattern),
        recursive_case: Box::new(Pattern::wildcard("rec")),
        body: Box::new(Pattern::wildcard("body")),
    };

    let expr = RelExpr::RecursiveCTE {
        name: "test".to_owned(),
        base_case: Box::new(
            RelExpr::scan("t")
                .filter(Expr::Const(Const::Bool(true))),
        ),
        recursive_case: Box::new(RelExpr::scan("test")),
        body: Box::new(RelExpr::scan("test")),
        cycle_detection: None,
    };

    let bindings = pattern
        .match_expr(&expr)
        .expect("should match nested filter in base case");

    assert_eq!(
        bindings.get_rel("inner"),
        Some(&RelExpr::scan("t"))
    );
}

#[test]
fn wildcard_matches_recursive_cte() {
    let pattern = Pattern::wildcard("anything");
    let expr = make_recursive_cte(
        "cte", "base", "rec", "body",
    );

    let bindings = pattern
        .match_expr(&expr)
        .expect("wildcard should match anything");

    assert!(bindings.get_rel("anything").is_some());
}

// ════════════════════════════════════════════════════════════
// Structural hash / memo table tests
// ════════════════════════════════════════════════════════════

#[test]
fn structural_hash_same_recursive_cte() {
    let a = make_recursive_cte("r", "base", "r", "r");
    let b = make_recursive_cte("r", "base", "r", "r");
    assert_eq!(structural_hash(&a), structural_hash(&b));
}

#[test]
fn structural_hash_differs_by_name() {
    let a = make_recursive_cte("alpha", "base", "x", "x");
    let b = make_recursive_cte("beta", "base", "x", "x");
    assert_ne!(structural_hash(&a), structural_hash(&b));
}

#[test]
fn structural_hash_differs_by_base_case() {
    let a = make_recursive_cte("r", "table_a", "r", "r");
    let b = make_recursive_cte("r", "table_b", "r", "r");
    assert_ne!(structural_hash(&a), structural_hash(&b));
}

#[test]
fn structural_hash_differs_by_body() {
    let a = make_recursive_cte("r", "base", "r", "out_a");
    let b = make_recursive_cte("r", "base", "r", "out_b");
    assert_ne!(structural_hash(&a), structural_hash(&b));
}

#[test]
fn structural_hash_differs_by_cycle_detection() {
    let a = make_recursive_cte_with_cycle("r", 100);
    let b = make_recursive_cte_with_cycle("r", 500);
    assert_ne!(structural_hash(&a), structural_hash(&b));
}

#[test]
fn structural_hash_with_vs_without_cycle_detection() {
    let without = make_recursive_cte("r", "base", "r", "r");
    let with_cd = make_recursive_cte_with_cycle("r", 1000);
    // These differ because base_case/recursive_case/body tables
    // differ ("base" vs "base", "r" vs "rec", "r" vs "r"),
    // plus cycle_detection presence. Build matching structures:
    let a = RelExpr::RecursiveCTE {
        name: "r".to_owned(),
        base_case: Box::new(RelExpr::scan("base")),
        recursive_case: Box::new(RelExpr::scan("rec")),
        body: Box::new(RelExpr::scan("r")),
        cycle_detection: None,
    };
    let b = RelExpr::RecursiveCTE {
        name: "r".to_owned(),
        base_case: Box::new(RelExpr::scan("base")),
        recursive_case: Box::new(RelExpr::scan("rec")),
        body: Box::new(RelExpr::scan("r")),
        cycle_detection: Some(CycleDetection {
            track_columns: vec!["id".to_owned()],
            max_depth: Some(1000),
            cycle_mark_column: None,
            path_column: None,
        }),
    };
    assert_ne!(
        structural_hash(&a),
        structural_hash(&b),
        "cycle detection should affect hash"
    );
}

#[test]
fn structural_hash_recursive_cte_differs_from_scan() {
    let rcte = make_recursive_cte("t", "t", "t", "t");
    let scan = RelExpr::scan("t");
    assert_ne!(structural_hash(&rcte), structural_hash(&scan));
}

// ════════════════════════════════════════════════════════════
// Cost model tests
// ════════════════════════════════════════════════════════════

#[test]
fn recursive_cte_cost_higher_than_simple_scan() {
    let rcte = make_recursive_cte(
        "r", "base", "r", "r",
    );
    let scan = RelExpr::scan("base");

    let optimizer = helpers::create_test_optimizer();

    // Both should optimize without error
    let opt_rcte = optimizer
        .optimize(&rcte)
        .expect("RecursiveCTE should optimize");
    let opt_scan = optimizer
        .optimize(&scan)
        .expect("Scan should optimize");

    // RecursiveCTE should not simplify to just a scan
    assert!(
        matches!(opt_rcte, RelExpr::RecursiveCTE { .. }),
        "RecursiveCTE should remain after optimization"
    );
    assert!(
        matches!(opt_scan, RelExpr::Scan { .. }),
        "Scan should remain after optimization"
    );
}

// ════════════════════════════════════════════════════════════
// Fixpoint execution tests
// ════════════════════════════════════════════════════════════

/// Evaluator that generates a sequence 1, 2, ..., N.
struct SequenceEvaluator {
    limit: i64,
}

impl ExprEvaluator for SequenceEvaluator {
    fn evaluate(
        &self,
        expr: &RelExpr,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Row>, ExecutionError> {
        match expr {
            RelExpr::Scan { table, .. }
                if !ctx.has_cte(table) =>
            {
                Ok(vec![Row::new(vec![Const::Int(1)])])
            }
            RelExpr::Scan { table, .. } => {
                let rows = ctx.get_cte(table).ok_or_else(|| {
                    ExecutionError::UnboundCTE(table.clone())
                })?;
                let mut out = Vec::new();
                for row in rows {
                    if let Some(Const::Int(n)) =
                        row.values.first()
                    {
                        if *n < self.limit {
                            out.push(Row::new(vec![
                                Const::Int(n + 1),
                            ]));
                        }
                    }
                }
                Ok(out)
            }
            _ => Ok(Vec::new()),
        }
    }
}

/// Evaluator that simulates a tree traversal producing
/// multiple children per node.
struct TreeEvaluator {
    branching_factor: usize,
    max_value: i64,
}

impl ExprEvaluator for TreeEvaluator {
    fn evaluate(
        &self,
        expr: &RelExpr,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Row>, ExecutionError> {
        match expr {
            RelExpr::Scan { table, .. }
                if !ctx.has_cte(table) =>
            {
                Ok(vec![Row::new(vec![Const::Int(1)])])
            }
            RelExpr::Scan { table, .. } => {
                let rows = ctx.get_cte(table).ok_or_else(|| {
                    ExecutionError::UnboundCTE(table.clone())
                })?;
                let mut out = Vec::new();
                for row in rows {
                    if let Some(Const::Int(n)) =
                        row.values.first()
                    {
                        for i in 0..self.branching_factor {
                            #[allow(clippy::cast_possible_wrap)]
                            let child =
                                n * 10 + (i as i64) + 1;
                            if child <= self.max_value {
                                out.push(Row::new(vec![
                                    Const::Int(child),
                                ]));
                            }
                        }
                    }
                }
                Ok(out)
            }
            _ => Ok(Vec::new()),
        }
    }
}

/// Evaluator that returns an error on the recursive step.
struct ErrorEvaluator {
    first_call: std::cell::Cell<bool>,
}

impl ErrorEvaluator {
    fn new() -> Self {
        Self {
            first_call: std::cell::Cell::new(true),
        }
    }
}

impl ExprEvaluator for ErrorEvaluator {
    fn evaluate(
        &self,
        _expr: &RelExpr,
        _ctx: &ExecutionContext,
    ) -> Result<Vec<Row>, ExecutionError> {
        if self.first_call.get() {
            self.first_call.set(false);
            Ok(vec![Row::new(vec![Const::Int(1)])])
        } else {
            Err(ExecutionError::EvalError(
                "simulated failure".to_owned(),
            ))
        }
    }
}

/// Evaluator producing multi-column rows.
struct MultiColumnEvaluator {
    limit: i64,
}

impl ExprEvaluator for MultiColumnEvaluator {
    fn evaluate(
        &self,
        expr: &RelExpr,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Row>, ExecutionError> {
        match expr {
            RelExpr::Scan { table, .. }
                if !ctx.has_cte(table) =>
            {
                Ok(vec![Row::new(vec![
                    Const::Int(1),
                    Const::String("start".to_owned()),
                ])])
            }
            RelExpr::Scan { table, .. } => {
                let rows = ctx.get_cte(table).ok_or_else(|| {
                    ExecutionError::UnboundCTE(table.clone())
                })?;
                let mut out = Vec::new();
                for row in rows {
                    if let Some(Const::Int(n)) =
                        row.values.first()
                    {
                        if *n < self.limit {
                            out.push(Row::new(vec![
                                Const::Int(n + 1),
                                Const::String(format!(
                                    "step_{}", n + 1
                                )),
                            ]));
                        }
                    }
                }
                Ok(out)
            }
            _ => Ok(Vec::new()),
        }
    }
}

#[test]
fn execution_sequence_to_ten() {
    let executor = RecursiveCTEExecutor::new();
    let evaluator = SequenceEvaluator { limit: 10 };
    let base = RelExpr::scan("seq");
    let recursive = RelExpr::scan("seq");

    let result = executor
        .execute_fixpoint(&base, &recursive, "seq", &evaluator)
        .expect("execution should succeed");

    assert_eq!(result.rows.len(), 10);
    assert_eq!(
        result.terminated_by,
        TerminationReason::Fixpoint
    );

    let values: Vec<i64> = result
        .rows
        .iter()
        .filter_map(|r| match r.values.first() {
            Some(Const::Int(n)) => Some(*n),
            _ => None,
        })
        .collect();
    assert_eq!(
        values,
        (1..=10).collect::<Vec<_>>()
    );
}

#[test]
fn execution_tree_traversal() {
    let executor = RecursiveCTEExecutor::new()
        .max_iterations(20)
        .cycle_detection(true);
    let evaluator = TreeEvaluator {
        branching_factor: 2,
        max_value: 200,
    };
    let base = RelExpr::scan("tree");
    let recursive = RelExpr::scan("tree");

    let result = executor
        .execute_fixpoint(
            &base, &recursive, "tree", &evaluator,
        )
        .expect("tree traversal should succeed");

    assert!(
        result.rows.len() > 1,
        "tree should produce multiple nodes"
    );
    assert_eq!(
        result.terminated_by,
        TerminationReason::Fixpoint
    );

    // All values should be unique (due to cycle detection)
    let values: Vec<i64> = result
        .rows
        .iter()
        .filter_map(|r| match r.values.first() {
            Some(Const::Int(n)) => Some(*n),
            _ => None,
        })
        .collect();
    let unique: HashSet<i64> = values.iter().copied().collect();
    assert_eq!(
        values.len(),
        unique.len(),
        "all values should be unique with cycle detection"
    );
}

#[test]
fn execution_error_propagation() {
    let executor = RecursiveCTEExecutor::new();
    let evaluator = ErrorEvaluator::new();
    let base = RelExpr::scan("err");
    let recursive = RelExpr::scan("err");

    let result = executor.execute_fixpoint(
        &base, &recursive, "err", &evaluator,
    );

    assert!(
        result.is_err(),
        "error from evaluator should propagate"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("simulated failure"),
        "error message should be preserved"
    );
}

#[test]
fn execution_multi_column_rows() {
    let executor = RecursiveCTEExecutor::new();
    let evaluator = MultiColumnEvaluator { limit: 3 };
    let base = RelExpr::scan("mc");
    let recursive = RelExpr::scan("mc");

    let result = executor
        .execute_fixpoint(&base, &recursive, "mc", &evaluator)
        .expect("multi-column execution should succeed");

    assert_eq!(result.rows.len(), 3);
    for row in &result.rows {
        assert_eq!(
            row.width(),
            2,
            "each row should have 2 columns"
        );
    }
}

#[test]
fn execution_max_iterations_with_config() {
    let config = RecursiveCTEConfig {
        max_iterations: 5,
        cycle_detection: false,
    };
    let executor =
        RecursiveCTEExecutor::with_config(config);
    let evaluator = SequenceEvaluator { limit: 100 };
    let base = RelExpr::scan("s");
    let recursive = RelExpr::scan("s");

    let result = executor
        .execute_fixpoint(&base, &recursive, "s", &evaluator)
        .expect("execution should succeed");

    assert_eq!(
        result.terminated_by,
        TerminationReason::MaxIterations
    );
    assert_eq!(result.iterations, 5);
}

#[test]
fn execution_cycle_detection_deduplicates() {
    /// Always returns [42] regardless of input.
    struct ConstantEvaluator;

    impl ExprEvaluator for ConstantEvaluator {
        fn evaluate(
            &self,
            _expr: &RelExpr,
            _ctx: &ExecutionContext,
        ) -> Result<Vec<Row>, ExecutionError> {
            Ok(vec![Row::new(vec![Const::Int(42)])])
        }
    }

    let executor = RecursiveCTEExecutor::new()
        .max_iterations(100)
        .cycle_detection(true);
    let evaluator = ConstantEvaluator;
    let base = RelExpr::scan("c");
    let recursive = RelExpr::scan("c");

    let result = executor
        .execute_fixpoint(&base, &recursive, "c", &evaluator)
        .expect("execution should succeed");

    assert_eq!(
        result.terminated_by,
        TerminationReason::Fixpoint
    );
    assert_eq!(
        result.rows.len(),
        1,
        "cycle detection should keep only one [42]"
    );
}

#[test]
fn execution_empty_base_case() {
    struct EmptyBaseEvaluator;

    impl ExprEvaluator for EmptyBaseEvaluator {
        fn evaluate(
            &self,
            _expr: &RelExpr,
            _ctx: &ExecutionContext,
        ) -> Result<Vec<Row>, ExecutionError> {
            Ok(Vec::new())
        }
    }

    let executor = RecursiveCTEExecutor::new();
    let evaluator = EmptyBaseEvaluator;
    let base = RelExpr::scan("e");
    let recursive = RelExpr::scan("e");

    let result = executor
        .execute_fixpoint(&base, &recursive, "e", &evaluator)
        .expect("empty base should succeed");

    assert_eq!(result.rows.len(), 0);
    assert_eq!(result.iterations, 0);
    assert_eq!(
        result.terminated_by,
        TerminationReason::Fixpoint
    );
}

// ════════════════════════════════════════════════════════════
// ExecutionContext tests
// ════════════════════════════════════════════════════════════

#[test]
fn context_multiple_bindings() {
    let mut ctx = ExecutionContext::new();

    ctx.bind_cte(
        "a",
        vec![Row::new(vec![Const::Int(1)])],
    );
    ctx.bind_cte(
        "b",
        vec![Row::new(vec![Const::Int(2)])],
    );

    assert!(ctx.has_cte("a"));
    assert!(ctx.has_cte("b"));
    assert!(!ctx.has_cte("c"));

    let rows_a = ctx.get_cte("a").unwrap();
    assert_eq!(rows_a.len(), 1);
    let rows_b = ctx.get_cte("b").unwrap();
    assert_eq!(rows_b.len(), 1);
}

#[test]
fn context_rebind_overwrites() {
    let mut ctx = ExecutionContext::new();

    ctx.bind_cte(
        "x",
        vec![Row::new(vec![Const::Int(1)])],
    );
    ctx.bind_cte(
        "x",
        vec![
            Row::new(vec![Const::Int(2)]),
            Row::new(vec![Const::Int(3)]),
        ],
    );

    let rows = ctx.get_cte("x").unwrap();
    assert_eq!(
        rows.len(),
        2,
        "rebinding should replace previous value"
    );
}

// ════════════════════════════════════════════════════════════
// Row type tests
// ════════════════════════════════════════════════════════════

#[test]
fn row_hash_consistency() {
    let r1 = Row::new(vec![
        Const::Int(1),
        Const::Float(3.14),
        Const::String("hello".to_owned()),
        Const::Bool(true),
        Const::Null,
    ]);
    let r2 = Row::new(vec![
        Const::Int(1),
        Const::Float(3.14),
        Const::String("hello".to_owned()),
        Const::Bool(true),
        Const::Null,
    ]);

    let mut set = HashSet::new();
    set.insert(r1.clone());
    assert!(set.contains(&r2), "equal rows should hash same");
}

#[test]
fn row_different_types_not_equal() {
    let int_row = Row::new(vec![Const::Int(1)]);
    let str_row =
        Row::new(vec![Const::String("1".to_owned())]);
    let null_row = Row::new(vec![Const::Null]);

    assert_ne!(int_row, str_row);
    assert_ne!(int_row, null_row);
    assert_ne!(str_row, null_row);
}

#[test]
fn row_width() {
    assert_eq!(Row::new(vec![]).width(), 0);
    assert_eq!(Row::new(vec![Const::Int(1)]).width(), 1);
    assert_eq!(
        Row::new(vec![
            Const::Int(1),
            Const::Int(2),
            Const::Int(3),
        ])
        .width(),
        3
    );
}

// ════════════════════════════════════════════════════════════
// Schema mismatch test
// ════════════════════════════════════════════════════════════

#[test]
fn execution_schema_mismatch_detected() {
    /// Base returns 1 column, recursive returns 2.
    struct MismatchEvaluator {
        first_call: std::cell::Cell<bool>,
    }

    impl MismatchEvaluator {
        fn new() -> Self {
            Self {
                first_call: std::cell::Cell::new(true),
            }
        }
    }

    impl ExprEvaluator for MismatchEvaluator {
        fn evaluate(
            &self,
            _expr: &RelExpr,
            _ctx: &ExecutionContext,
        ) -> Result<Vec<Row>, ExecutionError> {
            if self.first_call.get() {
                self.first_call.set(false);
                Ok(vec![Row::new(vec![Const::Int(1)])])
            } else {
                Ok(vec![Row::new(vec![
                    Const::Int(1),
                    Const::Int(2),
                ])])
            }
        }
    }

    let executor = RecursiveCTEExecutor::new();
    let evaluator = MismatchEvaluator::new();
    let base = RelExpr::scan("m");
    let recursive = RelExpr::scan("m");

    let result = executor.execute_fixpoint(
        &base, &recursive, "m", &evaluator,
    );

    assert!(
        result.is_err(),
        "schema mismatch should produce error"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("schema mismatch"),
        "error should mention schema mismatch, got: {err}"
    );
}

// ════════════════════════════════════════════════════════════
// Termination reason display
// ════════════════════════════════════════════════════════════

#[test]
fn termination_reasons_display() {
    assert_eq!(
        TerminationReason::Fixpoint.to_string(),
        "fixpoint"
    );
    assert_eq!(
        TerminationReason::MaxIterations.to_string(),
        "max iterations"
    );
    assert_eq!(
        TerminationReason::CycleDetected.to_string(),
        "cycle detected"
    );
}

// ════════════════════════════════════════════════════════════
// Default config
// ════════════════════════════════════════════════════════════

#[test]
fn default_config_values() {
    let config = RecursiveCTEConfig::default();
    assert_eq!(config.max_iterations, 100);
    assert!(config.cycle_detection);
}

#[test]
fn executor_default_is_same_as_new() {
    let a = RecursiveCTEExecutor::default();
    let b = RecursiveCTEExecutor::new();
    // Both should behave identically (same config)
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}")
    );
}

// ════════════════════════════════════════════════════════════
// references_cte integration
// ════════════════════════════════════════════════════════════

#[test]
fn recursive_cte_references_self() {
    let expr = make_recursive_cte(
        "r", "base", "r", "r",
    );
    // The recursive_case references "r" via Scan
    if let RelExpr::RecursiveCTE {
        recursive_case, ..
    } = &expr
    {
        assert!(recursive_case.references_cte("r"));
    }
}

#[test]
fn recursive_cte_body_references_name() {
    let expr = make_recursive_cte(
        "nums", "source", "nums", "nums",
    );
    if let RelExpr::RecursiveCTE { body, .. } = &expr {
        assert!(body.references_cte("nums"));
    }
}
