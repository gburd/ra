//! Edge case tests for UNNEST WITH ORDINALITY.
//!
//! Verifies correct behavior for empty arrays, NULL elements,
//! and multi-argument unnest with ordinality.

#![allow(clippy::expect_used)]

use ra_core::expr::{Const, Expr};
use ra_engine::executors::unnest::{MultiUnnestExecutor, UnnestExecutor};
use ra_engine::Row;

#[test]
fn unnest_empty_array_with_ordinality() {
    let expr = Expr::Array(vec![]);
    let executor = UnnestExecutor::new(expr, None, true);
    let rows = executor.execute(None).expect("should succeed");
    assert!(rows.is_empty());
}

#[test]
fn unnest_single_element_with_ordinality() {
    let expr = Expr::Array(vec![Expr::Const(Const::Int(42))]);
    let executor = UnnestExecutor::new(expr, None, true);
    let rows = executor.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].values,
        vec![Const::Int(42), Const::Int(1)]
    );
}

#[test]
fn unnest_nulls_with_ordinality() {
    let expr = Expr::Array(vec![
        Expr::Const(Const::Int(1)),
        Expr::Const(Const::Null),
        Expr::Const(Const::Int(3)),
    ]);
    let executor = UnnestExecutor::new(expr, None, true);
    let rows = executor.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].values,
        vec![Const::Int(1), Const::Int(1)]
    );
    assert_eq!(
        rows[1].values,
        vec![Const::Null, Const::Int(2)]
    );
    assert_eq!(
        rows[2].values,
        vec![Const::Int(3), Const::Int(3)]
    );
}

#[test]
fn unnest_mixed_types_with_ordinality() {
    let expr = Expr::Array(vec![
        Expr::Const(Const::Int(1)),
        Expr::Const(Const::String("hello".into())),
        Expr::Const(Const::Bool(true)),
    ]);
    let executor = UnnestExecutor::new(expr, None, true);
    let rows = executor.execute(None).expect("should succeed");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].values[1], Const::Int(1));
    assert_eq!(rows[1].values[1], Const::Int(2));
    assert_eq!(rows[2].values[1], Const::Int(3));
}

#[test]
fn multi_unnest_with_ordinality() {
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
        true,
    );
    let rows = executor.execute().expect("should succeed");
    assert_eq!(rows.len(), 3);
    // (1, 'a', 1)
    assert_eq!(rows[0].values[0], Const::Int(1));
    assert_eq!(
        rows[0].values[1],
        Const::String("a".into())
    );
    assert_eq!(rows[0].values[2], Const::Int(1));
    // (3, 'c', 3)
    assert_eq!(rows[2].values[0], Const::Int(3));
    assert_eq!(
        rows[2].values[1],
        Const::String("c".into())
    );
    assert_eq!(rows[2].values[2], Const::Int(3));
}

#[test]
fn multi_unnest_different_lengths_padded_with_null() {
    let executor = MultiUnnestExecutor::new(
        vec![
            Expr::Array(vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(2)),
                Expr::Const(Const::Int(3)),
            ]),
            Expr::Array(vec![
                Expr::Const(Const::String("a".into())),
            ]),
        ],
        vec![None, None],
        false,
    );
    let rows = executor.execute().expect("should succeed");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].values[0], Const::Int(1));
    assert_eq!(
        rows[0].values[1],
        Const::String("a".into())
    );
    // Shorter array is padded with NULL.
    assert_eq!(rows[1].values[0], Const::Int(2));
    assert_eq!(rows[1].values[1], Const::Null);
    assert_eq!(rows[2].values[0], Const::Int(3));
    assert_eq!(rows[2].values[1], Const::Null);
}

#[test]
fn multi_unnest_empty_arrays() {
    let executor = MultiUnnestExecutor::new(
        vec![Expr::Array(vec![]), Expr::Array(vec![])],
        vec![None, None],
        true,
    );
    let rows = executor.execute().expect("should succeed");
    assert!(rows.is_empty());
}

#[test]
fn multi_unnest_all_null_array() {
    let executor = MultiUnnestExecutor::new(
        vec![
            Expr::Const(Const::Null),
            Expr::Array(vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(2)),
            ]),
        ],
        vec![None, None],
        false,
    );
    let rows = executor.execute().expect("should succeed");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].values[0], Const::Null);
    assert_eq!(rows[0].values[1], Const::Int(1));
    assert_eq!(rows[1].values[0], Const::Null);
    assert_eq!(rows[1].values[1], Const::Int(2));
}

#[test]
fn multi_unnest_ordinality_applies_to_position() {
    let executor = MultiUnnestExecutor::new(
        vec![
            Expr::Array(vec![
                Expr::Const(Const::Int(10)),
                Expr::Const(Const::Int(20)),
            ]),
            Expr::Array(vec![
                Expr::Const(Const::String("x".into())),
                Expr::Const(Const::String("y".into())),
            ]),
        ],
        vec![None, None],
        true,
    );
    let rows = executor.execute().expect("should succeed");
    assert_eq!(rows.len(), 2);
    // Ordinality is the global row position, not per-array.
    assert_eq!(rows[0].values[2], Const::Int(1));
    assert_eq!(rows[1].values[2], Const::Int(2));
}
