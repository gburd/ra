//! PostgreSQL compatibility tests for UNNEST.
//!
//! These tests verify that our UNNEST implementation produces
//! the same results as PostgreSQL for various query patterns.
//! Tests marked `#[ignore]` require a running PostgreSQL instance.

#![allow(clippy::expect_used)]

use ra_core::expr::{Const, Expr};
use ra_engine::executors::table_function::TableFunctionExecutor;
use ra_engine::executors::unnest::{MultiUnnestExecutor, UnnestExecutor};
use ra_engine::Row;

/// Verifies basic UNNEST of integer array matches PG semantics.
///
/// PostgreSQL:
/// ```sql
/// SELECT * FROM unnest(array[1,2,3]);
/// -- Returns: 1, 2, 3
/// ```
#[test]
fn compat_unnest_integer_array() {
    let expr = Expr::Array(vec![
        Expr::Const(Const::Int(1)),
        Expr::Const(Const::Int(2)),
        Expr::Const(Const::Int(3)),
    ]);
    let executor = UnnestExecutor::new(expr, None, false);
    let rows = executor.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 3);
    let values: Vec<&Const> =
        rows.iter().map(|r| &r.values[0]).collect();
    assert_eq!(
        values,
        vec![&Const::Int(1), &Const::Int(2), &Const::Int(3)]
    );
}

/// Verifies UNNEST with ordinality matches PG.
///
/// PostgreSQL:
/// ```sql
/// SELECT * FROM unnest(array['a','b','c'])
///     WITH ORDINALITY;
/// -- Returns: ('a',1), ('b',2), ('c',3)
/// ```
#[test]
fn compat_unnest_with_ordinality() {
    let expr = Expr::Array(vec![
        Expr::Const(Const::String("a".into())),
        Expr::Const(Const::String("b".into())),
        Expr::Const(Const::String("c".into())),
    ]);
    let executor = UnnestExecutor::new(expr, None, true);
    let rows = executor.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].values,
        vec![Const::String("a".into()), Const::Int(1)]
    );
    assert_eq!(
        rows[1].values,
        vec![Const::String("b".into()), Const::Int(2)]
    );
    assert_eq!(
        rows[2].values,
        vec![Const::String("c".into()), Const::Int(3)]
    );
}

/// Verifies multi-argument UNNEST matches PG.
///
/// PostgreSQL:
/// ```sql
/// SELECT * FROM unnest(
///   ARRAY[1,2,3],
///   ARRAY['a','b','c']
/// ) AS t(num, letter);
/// -- Returns: (1,'a'), (2,'b'), (3,'c')
/// ```
#[test]
fn compat_multi_unnest_parallel() {
    let executor = MultiUnnestExecutor::new(
        vec![
            Expr::Array(vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(2)),
                Expr::Const(Const::Int(3)),
            ]),
            Expr::Array(vec![
                Expr::Const(Const::String("a".into())),
                Expr::Const(Const::String("b".into())),
                Expr::Const(Const::String("c".into())),
            ]),
        ],
        vec![Some("num".into()), Some("letter".into())],
        false,
    );
    let rows = executor.execute().expect("should succeed");
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].values,
        vec![Const::Int(1), Const::String("a".into())]
    );
    assert_eq!(
        rows[1].values,
        vec![Const::Int(2), Const::String("b".into())]
    );
    assert_eq!(
        rows[2].values,
        vec![Const::Int(3), Const::String("c".into())]
    );
}

/// Verifies multi-arg UNNEST with different-length arrays
/// pads with NULL (PostgreSQL behavior).
///
/// PostgreSQL:
/// ```sql
/// SELECT * FROM unnest(
///   ARRAY[1,2,3],
///   ARRAY['a']
/// ) AS t(num, letter);
/// -- Returns: (1,'a'), (2,NULL), (3,NULL)
/// ```
#[test]
fn compat_multi_unnest_null_padding() {
    let executor = MultiUnnestExecutor::new(
        vec![
            Expr::Array(vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(2)),
                Expr::Const(Const::Int(3)),
            ]),
            Expr::Array(vec![Expr::Const(
                Const::String("a".into()),
            )]),
        ],
        vec![Some("num".into()), Some("letter".into())],
        false,
    );
    let rows = executor.execute().expect("should succeed");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].values[1], Const::Null);
    assert_eq!(rows[2].values[1], Const::Null);
}

/// Verifies generate_series matches PG.
///
/// PostgreSQL:
/// ```sql
/// SELECT * FROM generate_series(1, 5);
/// -- Returns: 1, 2, 3, 4, 5
/// ```
#[test]
fn compat_generate_series() {
    let exec = TableFunctionExecutor::new(
        "generate_series",
        vec![
            Expr::Const(Const::Int(1)),
            Expr::Const(Const::Int(5)),
        ],
    );
    let rows = exec.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 5);
    let values: Vec<i64> = rows
        .iter()
        .filter_map(|r| {
            if let Const::Int(i) = r.values[0] {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(values, vec![1, 2, 3, 4, 5]);
}

/// Verifies json_array_elements matches PG.
///
/// PostgreSQL:
/// ```sql
/// SELECT * FROM json_array_elements('[1,2,3]');
/// -- Returns: 1, 2, 3
/// ```
#[test]
fn compat_json_array_elements() {
    let exec = TableFunctionExecutor::new(
        "json_array_elements",
        vec![Expr::Const(Const::String("[1,2,3]".into()))],
    );
    let rows = exec.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].values[0], Const::Int(1));
    assert_eq!(rows[1].values[0], Const::Int(2));
    assert_eq!(rows[2].values[0], Const::Int(3));
}

/// Verifies json_to_recordset matches PG.
///
/// PostgreSQL:
/// ```sql
/// SELECT * FROM json_to_recordset(
///     '[{"a":1,"b":"foo"},{"a":2,"b":"bar"}]'
/// ) AS x(a int, b text);
/// -- Returns: (1,'foo'), (2,'bar')
/// ```
#[test]
fn compat_json_to_recordset() {
    let exec = TableFunctionExecutor::new(
        "json_to_recordset",
        vec![Expr::Const(Const::String(
            r#"[{"a":1,"b":"foo"},{"a":2,"b":"bar"}]"#.into(),
        ))],
    );
    let rows = exec.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].values.len(), 2);
}

#[test]
#[ignore] // Requires PostgreSQL
fn compare_unnest_with_postgres() {
    // This test would connect to PostgreSQL and compare results.
    // Placeholder for actual PG integration testing.
    //
    // let pg_result = run_in_postgres(
    //     "SELECT * FROM unnest(array[1,2,3])"
    // );
    // let ra_result = run_in_ra(
    //     "SELECT * FROM unnest(array[1,2,3])"
    // );
    // assert_eq!(pg_result, ra_result);
}
