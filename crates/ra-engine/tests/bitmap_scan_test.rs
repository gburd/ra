//! Tests for bitmap index scan support (RFC 0018).
//!
//! Validates that the optimizer can combine multiple index scans using
//! bitmap AND/OR operations and access heap pages sequentially.

use ra_core::algebra::{RelExpr, JoinType};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::cost::IntegratedCostModel;
use ra_engine::egraph::Optimizer;
use ra_hardware::HardwareProfile;
use ra_stats::profiles::StatisticsProfile;
use std::collections::HashMap;

fn make_filter_scan(
    table: &str,
    col: &str,
    op: BinOp,
    value: i64,
) -> RelExpr {
    RelExpr::Filter {
        predicate: Expr::BinOp {
            op,
            left: Box::new(Expr::Column(ColumnRef {
                table: Some(table.to_string()),
                column: col.to_string(),
            })),
            right: Box::new(Expr::Const(ra_core::expr::Const::Int(
                value,
            ))),
        },
        input: Box::new(RelExpr::Scan {
            table: table.to_string(),
            alias: None,
        }),
    }
}

fn make_multi_predicate_query(table: &str) -> RelExpr {
    // SELECT * FROM table WHERE age > 25 AND city = 'Seattle' AND status = 'active'
    let age_pred = Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef {
            table: Some(table.to_string()),
            column: "age".to_string(),
        })),
        right: Box::new(Expr::Const(ra_core::expr::Const::Int(25))),
    };

    let city_pred = Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef {
            table: Some(table.to_string()),
            column: "city".to_string(),
        })),
        right: Box::new(Expr::Const(ra_core::expr::Const::Str(
            "Seattle".to_string(),
        ))),
    };

    let status_pred = Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef {
            table: Some(table.to_string()),
            column: "status".to_string(),
        })),
        right: Box::new(Expr::Const(ra_core::expr::Const::Str(
            "active".to_string(),
        ))),
    };

    let combined = Expr::BinOp {
        op: BinOp::And,
        left: Box::new(age_pred.clone()),
        right: Box::new(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(city_pred),
            right: Box::new(status_pred),
        }),
    };

    RelExpr::Filter {
        predicate: combined,
        input: Box::new(RelExpr::Scan {
            table: table.to_string(),
            alias: None,
        }),
    }
}

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
            right: Box::new(Expr::Const(ra_core::expr::Const::Str(
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
                    right: Box::new(Expr::Const(ra_core::expr::Const::Str(
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
            right: Box::new(Expr::Const(ra_core::expr::Const::Str(
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

    // Should be less than 3 individual sequential scans
    let seq_cost = model.scan_cost("users");
    assert!(cost < seq_cost * 3.0);
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

    let bitmap_cost = model.full_bitmap_scan_cost("orders", &[0.2, 0.3, 0.15]);
    let seq_cost = model.scan_cost("orders");

    // Bitmap scan should be cheaper than 3 sequential scans
    assert!(bitmap_cost < seq_cost * 2.0);
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

    let rec_expr = ra_engine::egraph::to_egraph(&expr)
        .expect("should convert to egraph");
    let egraph = ra_engine::egraph::Optimizer::new(
        HashMap::new(),
        HardwareProfile::cpu_only(),
    )
    .build_egraph(&rec_expr)
    .expect("should build egraph");

    let root_id = egraph.find(rec_expr.as_ref().len() - 1);
    let result = ra_engine::egraph::from_egraph_node(&egraph, root_id)
        .expect("should extract from egraph");

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

    let rec_expr = ra_engine::egraph::to_egraph(&expr)
        .expect("should convert to egraph");
    let egraph = ra_engine::egraph::Optimizer::new(
        HashMap::new(),
        HardwareProfile::cpu_only(),
    )
    .build_egraph(&rec_expr)
    .expect("should build egraph");

    let root_id = egraph.find(rec_expr.as_ref().len() - 1);
    let result = ra_engine::egraph::from_egraph_node(&egraph, root_id)
        .expect("should extract from egraph");

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

    let rec_expr = ra_engine::egraph::to_egraph(&expr)
        .expect("should convert to egraph");
    let egraph = ra_engine::egraph::Optimizer::new(
        HashMap::new(),
        HardwareProfile::cpu_only(),
    )
    .build_egraph(&rec_expr)
    .expect("should build egraph");

    let root_id = egraph.find(rec_expr.as_ref().len() - 1);
    let result = ra_engine::egraph::from_egraph_node(&egraph, root_id)
        .expect("should extract from egraph");

    assert!(matches!(result, RelExpr::BitmapHeapScan { .. }));
}
