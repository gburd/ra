//! Integration between ra-stats and ra-core cost models.
//!
//! Bridges the statistics abstraction system with the query optimizer's
//! cost model, enabling statistics-aware planning.

use crate::accuracy::{StatisticsState, Staleness};
use crate::profiles::StatisticsProfile;
use crate::types::{ColumnStats, TableStats};
use std::collections::HashMap;

/// Bundled table statistics with accuracy metadata.
#[derive(Debug, Clone)]
pub struct ManagedTableStats {
    /// Core table-level statistics.
    pub table: TableStats,
    /// Per-column statistics keyed by column name.
    pub columns: HashMap<String, ColumnStats>,
    /// Accuracy state tracking staleness and confidence.
    pub state: StatisticsState,
}

/// Statistics adapter bridging ra-stats with ra-core.
///
/// Applies staleness adjustments when converting to core statistics
/// types, ensuring the optimizer accounts for uncertainty.
#[derive(Debug, Clone)]
pub struct StatisticsAdapter {
    profile: StatisticsProfile,
    tables: HashMap<String, ManagedTableStats>,
}

impl StatisticsAdapter {
    /// Create a new adapter with the given profile.
    pub fn new(profile: StatisticsProfile) -> Self {
        Self {
            profile,
            tables: HashMap::new(),
        }
    }

    /// Register statistics for a table.
    pub fn add_table(&mut self, name: String, stats: ManagedTableStats) {
        self.tables.insert(name, stats);
    }

    /// Get managed statistics for a table.
    pub fn get_table_stats(&self, table: &str) -> Option<&ManagedTableStats> {
        self.tables.get(table)
    }

    /// Get mutable managed statistics for a table.
    pub fn get_table_stats_mut(
        &mut self,
        table: &str,
    ) -> Option<&mut ManagedTableStats> {
        self.tables.get_mut(table)
    }

    /// Convert to ra-core Statistics, applying staleness adjustments.
    pub fn to_core_statistics(
        &self,
        managed: &ManagedTableStats,
    ) -> ra_core::statistics::Statistics {
        let factor = Self::staleness_factor(&managed.state);
        let adjusted_rows = managed.table.row_count as f64 * factor;

        let mut stats = ra_core::statistics::Statistics::new(adjusted_rows);
        stats.avg_row_size = managed.table.average_row_size as u64;
        stats.total_size = managed.table.table_size_bytes;

        for (col_name, col_stats) in &managed.columns {
            let adjusted_ndv = col_stats.ndv as f64 * factor;
            let mut core_col =
                ra_core::statistics::ColumnStats::new(adjusted_ndv);
            core_col.null_fraction = col_stats.null_fraction;
            stats.columns.insert(col_name.clone(), core_col);
        }

        stats
    }

    /// Staleness multiplier: fresh = 1.0, increasingly inflated as
    /// statistics become stale to account for uncertainty.
    fn staleness_factor(state: &StatisticsState) -> f64 {
        match state.staleness() {
            Staleness::Fresh => 1.0,
            Staleness::SlightlyStale => 1.05,
            Staleness::ModeratelyStale => 1.2,
            Staleness::VeryStale => 1.5,
            Staleness::Unknown => 2.0,
        }
    }

    /// Whether the profile would reject these statistics as too stale.
    pub fn should_reject(&self, state: &StatisticsState) -> bool {
        state.should_refresh(self.profile.refresh_threshold.clone())
    }

    /// Get the active statistics profile.
    pub fn profile(&self) -> &StatisticsProfile {
        &self.profile
    }

    /// Number of registered tables.
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::accuracy::StatisticsSource;
    use crate::profiles::StatisticsProfile;
    use crate::types::TableStats;

    fn sample_table() -> ManagedTableStats {
        ManagedTableStats {
            table: TableStats {
                row_count: 10_000,
                page_count: 100,
                average_row_size: 100.0,
                table_size_bytes: 1_000_000,
                live_tuples: Some(9_500),
                dead_tuples: Some(500),
                last_analyzed: Some(1_000_000),
            },
            columns: HashMap::new(),
            state: StatisticsState::new(
                StatisticsSource::ExactCount,
                10_000,
            ),
        }
    }

    fn sample_with_columns() -> ManagedTableStats {
        let mut managed = sample_table();
        managed.columns.insert(
            "id".to_string(),
            ColumnStats {
                column_id: "id".to_string(),
                ndv: 10_000,
                null_fraction: 0.0,
                avg_width: 8.0,
                mcv: None,
                histogram: None,
                correlation: Some(1.0),
            },
        );
        managed.columns.insert(
            "status".to_string(),
            ColumnStats {
                column_id: "status".to_string(),
                ndv: 5,
                null_fraction: 0.02,
                avg_width: 12.0,
                mcv: None,
                histogram: None,
                correlation: None,
            },
        );
        managed
    }

    #[test]
    fn adapter_fresh_stats_no_adjustment() {
        let adapter = StatisticsAdapter::new(StatisticsProfile::standard());
        let managed = sample_table();
        let core = adapter.to_core_statistics(&managed);
        assert!((core.row_count - 10_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn adapter_stale_stats_inflated() {
        let adapter = StatisticsAdapter::new(StatisticsProfile::standard());
        let mut managed = sample_table();
        managed.state.record_modifications(5_000);
        let core = adapter.to_core_statistics(&managed);
        assert!(core.row_count > 10_000.0);
    }

    #[test]
    fn adapter_very_stale_large_inflation() {
        let adapter = StatisticsAdapter::new(StatisticsProfile::standard());
        let mut managed = sample_table();
        managed.state.record_modifications(50_000);
        let core = adapter.to_core_statistics(&managed);
        assert!((core.row_count - 15_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn adapter_column_stats_converted() {
        let adapter = StatisticsAdapter::new(StatisticsProfile::standard());
        let managed = sample_with_columns();
        let core = adapter.to_core_statistics(&managed);
        assert_eq!(core.columns.len(), 2);
        let id = core.columns.get("id").expect("id column");
        assert!((id.distinct_count - 10_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn adapter_column_null_fraction_preserved() {
        let adapter = StatisticsAdapter::new(StatisticsProfile::standard());
        let managed = sample_with_columns();
        let core = adapter.to_core_statistics(&managed);
        let status = core.columns.get("status").expect("status column");
        assert!((status.null_fraction - 0.02).abs() < f64::EPSILON);
    }

    #[test]
    fn adapter_should_reject_fresh_stats() {
        let adapter =
            StatisticsAdapter::new(StatisticsProfile::real_time());
        let state = StatisticsState::new(
            StatisticsSource::ExactCount,
            10_000,
        );
        assert!(!adapter.should_reject(&state));
    }

    #[test]
    fn adapter_should_reject_stale_stats() {
        let adapter =
            StatisticsAdapter::new(StatisticsProfile::real_time());
        let mut state = StatisticsState::new(
            StatisticsSource::ExactCount,
            10_000,
        );
        state.record_modifications(5_000);
        assert!(adapter.should_reject(&state));
    }

    #[test]
    fn adapter_table_count() {
        let mut adapter =
            StatisticsAdapter::new(StatisticsProfile::standard());
        assert_eq!(adapter.table_count(), 0);
        adapter.add_table("users".to_string(), sample_table());
        assert_eq!(adapter.table_count(), 1);
    }

    #[test]
    fn adapter_get_table_stats() {
        let mut adapter =
            StatisticsAdapter::new(StatisticsProfile::standard());
        adapter.add_table("users".to_string(), sample_table());
        assert!(adapter.get_table_stats("users").is_some());
        assert!(adapter.get_table_stats("orders").is_none());
    }

    #[test]
    fn adapter_profile_accessor() {
        let adapter =
            StatisticsAdapter::new(StatisticsProfile::analytical());
        assert_eq!(adapter.profile().name, "Analytical");
    }
}
