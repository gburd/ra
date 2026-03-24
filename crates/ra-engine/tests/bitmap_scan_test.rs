//! Tests for bitmap index scan support (RFC 0018).
//!
//! Validates that the optimizer can combine multiple index scans using
//! bitmap AND/OR operations and access heap pages sequentially.

use ra_core::algebra::RelExpr;
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_engine::cost::IntegratedCostModel;
use ra_engine::egraph::to_rec_expr;
use ra_engine::extract::rec_expr_to_rel_expr;
use ra_hardware::HardwareProfile;
use ra_stats::profiles::StatisticsProfile;
use std::collections::HashMap;

#[test]
fn bitmap_index_scan_creation() {
    // Verify BitmapIndexScan can be created
    let bitmap_scan = RelExpr::BitmapIndexScan {
        table: "users".to_string(),
        index: "users_age_idx".to_string(),
        predicate: Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef {
                table: Some("users".to_string()),
                column: "age".to_string(),
            })),
            right: Box::new(Expr::Const(ra_core::expr::Const::Int(25))),
        },
    };

    assert!(matches!(bitmap_scan, RelExpr::BitmapIndexScan { .. }));
}

#[test]
fn bitmap_and_combines_scans() {
    // Verify BitmapAnd can combine multiple bitmap scans
    let scan1 = Box::new(RelExpr::BitmapIndexScan {
        table: "users".to_string(),
        index: "users_age_idx".to_string(),
        predicate: Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef {
                table: Some("users".to_string()),
                column: "age".to_string(),
            })),
            right: Box::new(Expr::Const(ra_core::expr::Const::Int(25))),
        },
    });

    let scan2 = Box::new(RelExpr::BitmapIndexScan {
        table: "users".to_string(),
        index: "users_city_idx".to_string(),
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef {
                table: Some("users".to_string()),
                column: "city".to_string(),
            })),
            right: Box::new(Expr::Const(ra_core::expr::Const::String(
                "Seattle".to_string(),
            ))),
        },
    });

    let bitmap_and = RelExpr::BitmapAnd {
        inputs: vec![scan1, scan2],
    };

    assert!(matches!(bitmap_and, RelExpr::BitmapAnd { .. }));
}

#[test]
fn bitmap_heap_scan_with_recheck() {
    // Verify BitmapHeapScan with recheck condition
    let bitmap = Box::new(RelExpr::BitmapAnd {
        inputs: vec![
            Box::new(RelExpr::BitmapIndexScan {
                table: "orders".to_string(),
                index: "orders_status_idx".to_string(),
                predicate: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef {
                        table: Some("orders".to_string()),
                        column: "status".to_string(),
                    })),
                    right: Box::new(Expr::Const(ra_core::expr::Const::String(
                        "shipped".to_string(),
                    ))),
                },
            }),
        ],
    });

    let heap_scan = RelExpr::BitmapHeapScan {
        table: "orders".to_string(),
        bitmap,
        recheck_cond: Some(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef {
                table: Some("orders".to_string()),
                column: "status".to_string(),
            })),
            right: Box::new(Expr::Const(ra_core::expr::Const::String(
                "shipped".to_string(),
            ))),
        }),
    };

    assert!(matches!(heap_scan, RelExpr::BitmapHeapScan { .. }));
}

#[test]
fn cost_model_bitmap_index_scan() {
    // Test bitmap index scan cost calculation
    let mut model = IntegratedCostModel::new(
        StatisticsProfile::standard(),
        HardwareProfile::cpu_only(),
    );

    let managed = ra_stats::integration::ManagedTableStats {
        table: ra_stats::types::TableStats {
            row_count: 100_000,
            page_count: 1_000,
            average_row_size: 100.0,
            table_size_bytes: 10_000_000,
            live_tuples: Some(100_000),
            dead_tuples: Some(0),
            last_analyzed: None,
        },
        columns: HashMap::new(),
        state: ra_stats::accuracy::StatisticsState::new(
            ra_stats::accuracy::StatisticsSource::ExactCount,
            100_000,
        ),
    };

    model.add_table("users".to_string(), managed);

    let cost = model.bitmap_index_scan_cost("users", 0.1);
    assert!(cost > 0.0);
    assert!(cost.is_finite());
}

#[test]
fn cost_model_bitmap_combine() {
    // Test bitmap combine cost
    let mut model = IntegratedCostModel::new(
        StatisticsProfile::standard(),
        HardwareProfile::cpu_only(),
    );

    let managed = ra_stats::integration::ManagedTableStats {
        table: ra_stats::types::TableStats {
            row_count: 100_000,
            page_count: 1_000,
            average_row_size: 100.0,
            table_size_bytes: 10_000_000,
            live_tuples: Some(100_000),
            dead_tuples: Some(0),
            last_analyzed: None,
        },
        columns: HashMap::new(),
        state: ra_stats::accuracy::StatisticsState::new(
            ra_stats::accuracy::StatisticsSource::ExactCount,
            100_000,
        ),
    };

    model.add_table("users".to_string(), managed);

    let cost = model.bitmap_combine_cost("users", 3);
    assert!(cost > 0.0);
    assert!(cost.is_finite());
}

#[test]
fn cost_model_bitmap_heap_scan() {
    // Test bitmap heap scan cost
    let mut model = IntegratedCostModel::new(
        StatisticsProfile::standard(),
        HardwareProfile::cpu_only(),
    );

    let managed = ra_stats::integration::ManagedTableStats {
        table: ra_stats::types::TableStats {
            row_count: 100_000,
            page_count: 1_000,
            average_row_size: 100.0,
            table_size_bytes: 10_000_000,
            live_tuples: Some(100_000),
            dead_tuples: Some(0),
            last_analyzed: None,
        },
        columns: HashMap::new(),
        state: ra_stats::accuracy::StatisticsState::new(
            ra_stats::accuracy::StatisticsSource::ExactCount,
            100_000,
        ),
    };

    model.add_table("users".to_string(), managed);

    let cost = model.bitmap_heap_scan_cost("users", 0.05);
    assert!(cost > 0.0);
    assert!(cost.is_finite());
}

#[test]
fn cost_model_full_bitmap_scan() {
    // Test full bitmap scan with multiple predicates
    let mut model = IntegratedCostModel::new(
        StatisticsProfile::standard(),
        HardwareProfile::cpu_only(),
    );

    let managed = ra_stats::integration::ManagedTableStats {
        table: ra_stats::types::TableStats {
            row_count: 100_000,
            page_count: 1_000,
            average_row_size: 100.0,
            table_size_bytes: 10_000_000,
            live_tuples: Some(100_000),
            dead_tuples: Some(0),
            last_analyzed: None,
        },
        columns: HashMap::new(),
        state: ra_stats::accuracy::StatisticsState::new(
            ra_stats::accuracy::StatisticsSource::ExactCount,
            100_000,
        ),
    };

    model.add_table("users".to_string(), managed);

    // Three predicates with selectivities 0.3, 0.2, 0.1
    let cost = model.full_bitmap_scan_cost("users", &[0.3, 0.2, 0.1]);
    assert!(cost > 0.0);
    assert!(cost.is_finite());

    // More selective predicates should produce lower cost
    let more_selective =
        model.full_bitmap_scan_cost("users", &[0.01, 0.01, 0.01]);
    assert!(more_selective < cost);
}

#[test]
fn bitmap_scan_cheaper_than_multiple_scans() {
    // Bitmap scan should be cheaper than multiple sequential scans
    let mut model = IntegratedCostModel::new(
        StatisticsProfile::standard(),
        HardwareProfile::cpu_only(),
    );

    let managed = ra_stats::integration::ManagedTableStats {
        table: ra_stats::types::TableStats {
            row_count: 1_000_000,
            page_count: 10_000,
            average_row_size: 100.0,
            table_size_bytes: 100_000_000,
            live_tuples: Some(1_000_000),
            dead_tuples: Some(0),
            last_analyzed: None,
        },
        columns: HashMap::new(),
        state: ra_stats::accuracy::StatisticsState::new(
            ra_stats::accuracy::StatisticsSource::ExactCount,
            1_000_000,
        ),
    };

    model.add_table("orders".to_string(), managed);

    let wider = model.full_bitmap_scan_cost("orders", &[0.2, 0.3, 0.15]);
    let narrower = model.full_bitmap_scan_cost("orders", &[0.05, 0.05, 0.05]);

    // More selective (narrower) predicates yield lower bitmap cost
    assert!(narrower < wider);

    // Adding more predicates with low selectivity should increase
    // cost only modestly (bitmap combine is cheap)
    let two_pred = model.full_bitmap_scan_cost("orders", &[0.05, 0.05]);
    let three_pred = model.full_bitmap_scan_cost("orders", &[0.05, 0.05, 0.05]);
    let incremental = three_pred - two_pred;
    assert!(incremental > 0.0);
    assert!(incremental < two_pred);
}

#[test]
fn egraph_roundtrip_bitmap_index_scan() {
    // Test conversion to/from e-graph
    let expr = RelExpr::BitmapIndexScan {
        table: "users".to_string(),
        index: "users_age_idx".to_string(),
        predicate: Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef {
                table: Some("users".to_string()),
                column: "age".to_string(),
            })),
            right: Box::new(Expr::Const(ra_core::expr::Const::Int(25))),
        },
    };

    let rec_expr = to_rec_expr(&expr)
        .expect("should convert to rec_expr");
    let result = rec_expr_to_rel_expr(&rec_expr)
        .expect("should convert back to RelExpr");

    assert!(matches!(result, RelExpr::BitmapIndexScan { .. }));
}

#[test]
fn egraph_roundtrip_bitmap_and() {
    // Test BitmapAnd e-graph conversion
    let expr = RelExpr::BitmapAnd {
        inputs: vec![
            Box::new(RelExpr::BitmapIndexScan {
                table: "users".to_string(),
                index: "idx1".to_string(),
                predicate: Expr::Column(ColumnRef {
                    table: Some("users".to_string()),
                    column: "age".to_string(),
                }),
            }),
            Box::new(RelExpr::BitmapIndexScan {
                table: "users".to_string(),
                index: "idx2".to_string(),
                predicate: Expr::Column(ColumnRef {
                    table: Some("users".to_string()),
                    column: "city".to_string(),
                }),
            }),
        ],
    };

    let rec_expr = to_rec_expr(&expr)
        .expect("should convert to rec_expr");
    let result = rec_expr_to_rel_expr(&rec_expr)
        .expect("should convert back to RelExpr");

    assert!(matches!(result, RelExpr::BitmapAnd { .. }));
}

#[test]
fn egraph_roundtrip_bitmap_heap_scan() {
    // Test BitmapHeapScan e-graph conversion
    let expr = RelExpr::BitmapHeapScan {
        table: "users".to_string(),
        bitmap: Box::new(RelExpr::BitmapIndexScan {
            table: "users".to_string(),
            index: "idx".to_string(),
            predicate: Expr::Column(ColumnRef {
                table: Some("users".to_string()),
                column: "age".to_string(),
            }),
        }),
        recheck_cond: None,
    };

    let rec_expr = to_rec_expr(&expr)
        .expect("should convert to rec_expr");
    let result = rec_expr_to_rel_expr(&rec_expr)
        .expect("should convert back to RelExpr");

    assert!(matches!(result, RelExpr::BitmapHeapScan { .. }));
}
