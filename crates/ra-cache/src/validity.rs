//! Statistics drift detection for cached plans.

use ra_core::cost::StatisticsProvider;

use crate::plan::CachedPlan;

/// Whether a cached plan's statistics are still fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftStatus {
    /// Statistics are within the drift threshold.
    Fresh,
    /// At least one table has drifted beyond the threshold.
    Stale,
    /// Statistics are missing for a referenced table (conservative:
    /// treat as stale).
    Unknown,
}

/// Drift information for a single table.
#[derive(Debug, Clone)]
pub struct TableDrift {
    /// Table name.
    pub table: String,
    /// Row count at optimization time.
    pub cached_row_count: f64,
    /// Current row count (if available).
    pub current_row_count: Option<f64>,
    /// Absolute fractional drift: `|current - cached| / cached`.
    pub drift_fraction: Option<f64>,
}

/// Aggregated drift report for a plan.
#[derive(Debug, Clone)]
pub struct PlanDrift {
    /// Overall drift status.
    pub status: DriftStatus,
    /// Per-table drift details.
    pub table_drifts: Vec<TableDrift>,
    /// Maximum drift fraction observed.
    pub max_drift: f64,
}

/// Drift report across all cached plans.
#[derive(Debug, Clone)]
pub struct DriftReport {
    /// Plans that have drifted beyond the threshold.
    pub stale_plans:
        Vec<(crate::key::QueryKey, PlanDrift)>,
}

impl DriftReport {
    /// Create an empty drift report.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stale_plans: Vec::new(),
        }
    }
}

impl Default for DriftReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Check whether a cached plan's statistics have drifted.
pub(crate) fn check_plan_drift(
    plan: &CachedPlan,
    current_stats: &dyn StatisticsProvider,
    threshold: f64,
) -> PlanDrift {
    let mut table_drifts = Vec::new();
    let mut max_drift: f64 = 0.0;
    let mut any_stale = false;
    let mut any_unknown = false;

    for (table, cached_stats) in &plan.statistics_snapshot {
        let cached_rows = cached_stats.row_count;

        if let Some(current) = current_stats.get_statistics(table)
        {
            let current_rows = current.row_count;
            let drift = if cached_rows.abs() < f64::EPSILON {
                if current_rows.abs() < f64::EPSILON {
                    0.0
                } else {
                    1.0
                }
            } else {
                ((current_rows - cached_rows) / cached_rows).abs()
            };

            if drift > max_drift {
                max_drift = drift;
            }
            if drift > threshold {
                any_stale = true;
            }

            table_drifts.push(TableDrift {
                table: table.clone(),
                cached_row_count: cached_rows,
                current_row_count: Some(current_rows),
                drift_fraction: Some(drift),
            });
        } else {
            any_unknown = true;
            table_drifts.push(TableDrift {
                table: table.clone(),
                cached_row_count: cached_rows,
                current_row_count: None,
                drift_fraction: None,
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
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use ra_core::statistics::Statistics;
    use std::collections::HashMap;

    #[derive(Debug)]
    struct TestProvider {
        tables: HashMap<String, Statistics>,
    }

    impl StatisticsProvider for TestProvider {
        fn get_statistics(
            &self,
            table: &str,
        ) -> Option<&Statistics> {
            self.tables.get(table)
        }
    }

    fn make_plan(tables: &[(&str, f64)]) -> CachedPlan {
        let mut snapshot = HashMap::new();
        let first_table =
            tables.first().map_or("t", |t| t.0);
        for &(name, rows) in tables {
            snapshot.insert(
                name.to_owned(),
                Statistics::new(rows),
            );
        }
        CachedPlan::new(
            ra_core::algebra::RelExpr::scan(first_table),
            ra_core::cost::Cost::ZERO,
            snapshot,
            "SELECT 1".to_owned(),
        )
    }

    fn make_provider(
        pairs: &[(&str, f64)],
    ) -> TestProvider {
        let mut tables = HashMap::new();
        for &(name, rows) in pairs {
            tables.insert(
                name.to_owned(),
                Statistics::new(rows),
            );
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
    }

    #[test]
    fn stale_beyond_threshold() {
        let plan = make_plan(&[("users", 1000.0)]);
        let provider = make_provider(&[("users", 1500.0)]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        assert!(drift.max_drift >= 0.2);
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
        let plan = make_plan(&[
            ("users", 1000.0),
            ("orders", 5000.0),
        ]);
        let provider = make_provider(&[
            ("users", 1050.0),
            ("orders", 10000.0),
        ]);
        let drift = check_plan_drift(&plan, &provider, 0.2);
        assert_eq!(drift.status, DriftStatus::Stale);
        // orders drifted 100%
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
}
