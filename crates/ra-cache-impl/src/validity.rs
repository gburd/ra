//! Statistics drift detection for cached plans.
//!
//! Supports multi-dimensional drift detection (RFC 0059):
//! row counts, column distinct counts, histogram shapes,
//! and index presence.

use ra_cache_api::{CachedPlan, DriftDimension, DriftStatus, PlanDrift, TableDrift};
use ra_core::cost::StatisticsProvider;
use ra_core::statistics::Histogram;

/// Compute fractional drift between two positive values.
fn fractional_drift(cached: f64, current: f64) -> f64 {
    if cached.abs() < f64::EPSILON {
        if current.abs() < f64::EPSILON {
            0.0
        } else {
            1.0
        }
    } else {
        ((current - cached) / cached).abs()
    }
}

/// Count histogram buckets.
fn bucket_count(hist: &Histogram) -> usize {
    match hist {
        Histogram::EquiWidth(h) => h.buckets.len(),
        Histogram::EquiDepth(h) => h.buckets.len(),
    }
}

/// Check whether a cached plan's statistics have drifted.
///
/// Now checks row counts, column NDVs, histogram shapes,
/// and index presence (RFC 0059).
#[expect(clippy::too_many_lines, reason = "drift check across multiple stat dimensions")]
pub(crate) fn check_plan_drift(
    plan: &CachedPlan,
    current_stats: &dyn StatisticsProvider,
    threshold: f64,
) -> PlanDrift {
    let mut table_drifts = Vec::new();
    let mut all_dimensions = Vec::new();
    let mut max_drift: f64 = 0.0;
    let mut any_stale = false;
    let mut any_unknown = false;

    for (table, cached_stats) in &plan.statistics_snapshot {
        let cached_rows = cached_stats.row_count;
        let mut dims = Vec::new();

        if let Some(current) = current_stats.get_statistics(table) {
            // Row count drift
            let current_rows = current.row_count;
            let drift = fractional_drift(cached_rows, current_rows);

            if drift > max_drift {
                max_drift = drift;
            }
            if drift > threshold {
                any_stale = true;
                dims.push(DriftDimension::RowCount {
                    table: table.clone(),
                    old_count: cached_rows,
                    new_count: current_rows,
                    drift,
                });
            }

            // Column distinct count drift
            for (col, cached_col) in &cached_stats.columns {
                if let Some(current_col) = current.columns.get(col) {
                    let ndv_drift =
                        fractional_drift(cached_col.distinct_count, current_col.distinct_count);
                    if ndv_drift > max_drift {
                        max_drift = ndv_drift;
                    }
                    if ndv_drift > threshold {
                        any_stale = true;
                        dims.push(DriftDimension::DistinctCount {
                            table: table.clone(),
                            column: col.clone(),
                            old_ndv: cached_col.distinct_count,
                            new_ndv: current_col.distinct_count,
                            drift: ndv_drift,
                        });
                    }

                    // Histogram shape change
                    if let (Some(old_h), Some(new_h)) =
                        (&cached_col.histogram, &current_col.histogram)
                    {
                        let old_bc = bucket_count(old_h);
                        let new_bc = bucket_count(new_h);
                        if old_bc != new_bc {
                            any_stale = true;
                            dims.push(DriftDimension::HistogramShape {
                                table: table.clone(),
                                column: col.clone(),
                                old_buckets: old_bc,
                                new_buckets: new_bc,
                            });
                        }
                    }
                }
            }

            // Index presence changes
            for idx_name in current.indexes.keys() {
                if !cached_stats.indexes.contains_key(idx_name) {
                    any_stale = true;
                    dims.push(DriftDimension::IndexPresence {
                        table: table.clone(),
                        index_name: idx_name.clone(),
                        added: true,
                    });
                }
            }
            for idx_name in cached_stats.indexes.keys() {
                if !current.indexes.contains_key(idx_name) {
                    any_stale = true;
                    dims.push(DriftDimension::IndexPresence {
                        table: table.clone(),
                        index_name: idx_name.clone(),
                        added: false,
                    });
                }
            }

            all_dimensions.extend(dims.iter().cloned());

            table_drifts.push(TableDrift {
                table: table.clone(),
                cached_row_count: cached_rows,
                current_row_count: Some(current_rows),
                drift_fraction: Some(drift),
                drifted_dimensions: dims,
            });
        } else {
            any_unknown = true;
            table_drifts.push(TableDrift {
                table: table.clone(),
                cached_row_count: cached_rows,
                current_row_count: None,
                drift_fraction: None,
                drifted_dimensions: Vec::new(),
            });
        }
    }

    let status = if any_stale {
        DriftStatus::Stale
    } else if any_unknown {
        DriftStatus::Unknown
    } else {
        DriftStatus::Fresh
    };

    PlanDrift {
        status,
        table_drifts,
        max_drift,
        dimensions: all_dimensions,
    }
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "exact float literals in tests")]
mod tests {
    use super::*;
    use ra_core::statistics::{
        ColumnStats, EquiWidthHistogram, Histogram, HistogramBucket, IndexStats, Statistics,
    };
    use std::collections::HashMap;

    #[derive(Debug)]
    struct TestProvider {
        tables: HashMap<String, Statistics>,
    }

    impl StatisticsProvider for TestProvider {
        fn get_statistics(&self, table: &str) -> Option<&Statistics> {
            self.tables.get(table)
        }
    }

    fn make_plan(tables: &[(&str, f64)]) -> CachedPlan {
        let mut snapshot = HashMap::new();
        let first_table = tables.first().map_or("t", |t| t.0);
        for &(name, rows) in tables {
            snapshot.insert(name.to_owned(), Statistics::new(rows));
        }
        CachedPlan::new(
            ra_core::algebra::RelExpr::scan(first_table),
            ra_core::cost::Cost::ZERO,
            snapshot,
            "SELECT 1".to_owned(),
        )
    }

    fn make_provider(pairs: &[(&str, f64)]) -> TestProvider {
        let mut tables = HashMap::new();
        for &(name, rows) in pairs {
            tables.insert(name.to_owned(), Statistics::new(rows));
        }
        TestProvider { tables }
    }

    #[test]
    fn fresh_within_threshold() {
        let plan = make_plan(&[("users", 1000.0)]);
        let provider = make_provider(&[("users", 1100.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Fresh);
        assert!(drift.max_drift < 0.2);
        assert!(drift.dimensions.is_empty());
    }

    #[test]
    fn stale_beyond_threshold() {
        let plan = make_plan(&[("users", 1000.0)]);
        let provider = make_provider(&[("users", 1500.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        assert!(drift.max_drift >= 0.2);
        assert!(!drift.dimensions.is_empty());
    }

    #[test]
    fn unknown_missing_stats() {
        let plan = make_plan(&[("users", 1000.0)]);
        let provider = make_provider(&[]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Unknown);
    }

    #[test]
    fn multiple_tables_worst_case() {
        let plan = make_plan(&[("users", 1000.0), ("orders", 5000.0)]);
        let provider = make_provider(&[("users", 1050.0), ("orders", 10000.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        assert!(drift.max_drift >= 0.9);
    }

    #[test]
    fn zero_cached_rows_current_nonzero() {
        let plan = make_plan(&[("empty", 0.0)]);
        let provider = make_provider(&[("empty", 100.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
    }

    #[test]
    fn zero_cached_rows_current_zero() {
        let plan = make_plan(&[("empty", 0.0)]);
        let provider = make_provider(&[("empty", 0.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Fresh);
    }

    #[test]
    fn table_drift_details() {
        let plan = make_plan(&[("users", 1000.0)]);
        let provider = make_provider(&[("users", 1300.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.table_drifts.len(), 1);
        let td = &drift.table_drifts[0];
        assert_eq!(td.table, "users");
        assert_eq!(td.cached_row_count, 1000.0);
        assert_eq!(td.current_row_count, Some(1300.0));
        let frac = td.drift_fraction.expect("should have drift");
        assert!((frac - 0.3).abs() < 1e-10);
    }

    // ── RFC 0059: Multi-dimensional drift tests ─────────

    #[test]
    fn ndv_drift_detected() {
        let mut snapshot = HashMap::new();
        let mut stats = Statistics::new(1000.0);
        stats.columns.insert("city".into(), ColumnStats::new(100.0));
        snapshot.insert("users".to_owned(), stats);

        let plan = CachedPlan::new(
            ra_core::algebra::RelExpr::scan("users"),
            ra_core::cost::Cost::ZERO,
            snapshot,
            "SELECT 1".to_owned(),
        );

        let mut current_stats = Statistics::new(1000.0);
        current_stats
            .columns
            .insert("city".into(), ColumnStats::new(500.0));
        let provider = TestProvider {
            tables: [("users".into(), current_stats)].into_iter().collect(),
        };

        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        assert!(drift.dimensions.iter().any(|d| matches!(
            d,
            DriftDimension::DistinctCount {
                column, ..
            } if column == "city"
        )));
    }

    #[test]
    fn histogram_shape_drift_detected() {
        let mut snapshot = HashMap::new();
        let mut stats = Statistics::new(1000.0);
        let mut col = ColumnStats::new(100.0);
        col.histogram = Some(Histogram::EquiWidth(EquiWidthHistogram {
            buckets: vec![
                HistogramBucket {
                    upper_bound: "50".into(),
                    row_count: 500.0,
                    distinct_count: 50.0,
                },
                HistogramBucket {
                    upper_bound: "100".into(),
                    row_count: 500.0,
                    distinct_count: 50.0,
                },
            ],
        }));
        stats.columns.insert("age".into(), col);
        snapshot.insert("users".to_owned(), stats);

        let plan = CachedPlan::new(
            ra_core::algebra::RelExpr::scan("users"),
            ra_core::cost::Cost::ZERO,
            snapshot,
            "SELECT 1".to_owned(),
        );

        let mut current_stats = Statistics::new(1000.0);
        let mut curr_col = ColumnStats::new(100.0);
        curr_col.histogram = Some(Histogram::EquiWidth(EquiWidthHistogram {
            buckets: vec![
                HistogramBucket {
                    upper_bound: "33".into(),
                    row_count: 333.0,
                    distinct_count: 33.0,
                },
                HistogramBucket {
                    upper_bound: "66".into(),
                    row_count: 333.0,
                    distinct_count: 33.0,
                },
                HistogramBucket {
                    upper_bound: "100".into(),
                    row_count: 334.0,
                    distinct_count: 34.0,
                },
            ],
        }));
        current_stats.columns.insert("age".into(), curr_col);
        let provider = TestProvider {
            tables: [("users".into(), current_stats)].into_iter().collect(),
        };

        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        assert!(drift
            .dimensions
            .iter()
            .any(|d| matches!(d, DriftDimension::HistogramShape { .. })));
    }

    #[test]
    fn index_added_detected() {
        let plan = make_plan(&[("orders", 5000.0)]);

        let mut current_stats = Statistics::new(5000.0);
        current_stats.indexes.insert(
            "idx_orders_date".into(),
            IndexStats::new(vec!["order_date".into()], ra_core::facts::IndexType::BTree),
        );
        let provider = TestProvider {
            tables: [("orders".into(), current_stats)].into_iter().collect(),
        };

        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        assert!(drift
            .dimensions
            .iter()
            .any(|d| matches!(d, DriftDimension::IndexPresence { added: true, .. })));
    }

    #[test]
    fn index_dropped_detected() {
        let mut snapshot = HashMap::new();
        let mut stats = Statistics::new(5000.0);
        stats.indexes.insert(
            "idx_orders_date".into(),
            IndexStats::new(vec!["order_date".into()], ra_core::facts::IndexType::BTree),
        );
        snapshot.insert("orders".to_owned(), stats);

        let plan = CachedPlan::new(
            ra_core::algebra::RelExpr::scan("orders"),
            ra_core::cost::Cost::ZERO,
            snapshot,
            "SELECT 1".to_owned(),
        );

        let provider = make_provider(&[("orders", 5000.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        assert!(drift
            .dimensions
            .iter()
            .any(|d| matches!(d, DriftDimension::IndexPresence { added: false, .. })));
    }
}
