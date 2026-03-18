//! Integration between ra-stats and ra-core cost models.
//!
//! This module bridges the statistics abstraction system with the
//! query optimizer's cost model, enabling statistics-aware planning.

use crate::profiles::StatisticsProfile;
use crate::types::{TableStats, ColumnStats};
use crate::accuracy::{StatisticsState, Staleness, RefreshThreshold};

/// Statistics adapter that implements the ra-core StatisticsProvider trait.
///
/// This bridges ra-stats types with the cost model interface.
#[derive(Debug)]
pub struct StatisticsAdapter {
    /// The active statistics profile
    profile: StatisticsProfile,
    /// Table statistics keyed by table name
    tables: std::collections::HashMap<String, TableStatistics>,
}

impl StatisticsAdapter {
    /// Create a new adapter with the given profile.
    pub fn new(profile: StatisticsProfile) -> Self {
        Self {
            profile,
            tables: std::collections::HashMap::new(),
        }
    }

    /// Register statistics for a table.
    pub fn add_table(&mut self, name: String, stats: TableStatistics) {
        self.tables.insert(name, stats);
    }

    /// Get statistics for a table, respecting profile constraints.
    pub fn get_table_stats(&self, table: &str) -> Option<&TableStatistics> {
        self.tables.get(table)
    }

    /// Convert ra-stats TableStatistics to ra-core Statistics.
    ///
    /// This applies staleness adjustments based on the active profile.
    pub fn to_core_statistics(
        &self,
        table_stats: &TableStatistics,
    ) -> ra_core::statistics::Statistics {
        let state = &table_stats.state;

        // Apply staleness penalty to cardinality estimate
        let adjusted_row_count = self.adjust_for_staleness(
            table_stats.row_count as f64,
            state,
        );

        let mut stats = ra_core::statistics::Statistics::new(adjusted_row_count);
        stats.avg_row_size = table_stats.avg_row_size;
        stats.total_size = table_stats.total_size;

        // Convert column statistics
        for (col_name, col_stats) in &table_stats.columns {
            let mut core_col = ra_core::statistics::ColumnStats::new(
                self.adjust_for_staleness(col_stats.distinct_count as f64, state)
            );
            core_col.null_fraction = col_stats.null_fraction;

            stats.columns.insert(col_name.clone(), core_col);
        }

        stats
    }

    /// Adjust a statistic value based on staleness and profile.
    ///
    /// Stale statistics increase uncertainty, modeled as a confidence interval.
    fn adjust_for_staleness(&self, value: f64, state: &StatisticsState) -> f64 {
        let staleness = state.staleness();

        match staleness {
            Staleness::Fresh => value,
            Staleness::Acceptable => {
                // Small uncertainty: ±5%
                value * 1.05
            }
            Staleness::Stale => {
                // Medium uncertainty: ±20%
                value * 1.2
            }
            Staleness::VeryStale => {
                // High uncertainty: ±50%
                value * 1.5
            }
        }
    }

    /// Check if the profile would reject using these statistics.
    ///
    /// Returns true if stats are too stale for the profile's threshold.
    pub fn should_reject_statistics(&self, state: &StatisticsState) -> bool {
        let staleness = state.staleness();
        let threshold = self.profile.refresh_threshold();

        match staleness {
            Staleness::Fresh => false,
            Staleness::Acceptable => threshold.max_staleness_acceptable(),
            Staleness::Stale => threshold.max_staleness_stale(),
            Staleness::VeryStale => true, // Always reject very stale
        }
    }

    /// Get the active statistics profile.
    pub fn profile(&self) -> &StatisticsProfile {
        &self.profile
    }
}

/// Simulates statistics staleness for testing.
///
/// This allows testing how stale statistics affect query plans.
#[derive(Debug, Clone)]
pub struct StatisticsSimulator {
    /// Staleness level to simulate
    staleness: Staleness,
    /// Modification count to simulate
    modifications: u64,
}

impl StatisticsSimulator {
    /// Create a simulator with fresh statistics.
    pub fn fresh() -> Self {
        Self {
            staleness: Staleness::Fresh,
            modifications: 0,
        }
    }

    /// Create a simulator with stale statistics.
    pub fn stale(modifications: u64) -> Self {
        Self {
            staleness: Staleness::Stale,
            modifications,
        }
    }

    /// Apply simulated staleness to table statistics.
    pub fn apply(&self, stats: &mut TableStatistics) {
        stats.state.record_modifications(self.modifications);
    }

    /// Create a simulated state matching this simulator.
    pub fn create_state(&self) -> StatisticsState {
        let mut state = StatisticsState::default();
        state.record_modifications(self.modifications);
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::StatisticsProfile;
    use crate::types::{TableStatistics, ColumnStatistics};

    #[test]
    fn adapter_fresh_statistics() {
        let profile = StatisticsProfile::standard();
        let adapter = StatisticsAdapter::new(profile);

        let mut table_stats = TableStatistics::new("users".to_string(), 1000);
        let state = StatisticsState::default(); // Fresh
        table_stats.state = state;

        let core_stats = adapter.to_core_statistics(&table_stats);

        // Fresh stats: no adjustment
        assert!((core_stats.row_count - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn adapter_stale_statistics_adjustment() {
        let profile = StatisticsProfile::standard();
        let adapter = StatisticsAdapter::new(profile);

        let mut table_stats = TableStatistics::new("orders".to_string(), 1000);
        let mut state = StatisticsState::default();
        state.record_modifications(1000); // Make stale
        table_stats.state = state;

        let core_stats = adapter.to_core_statistics(&table_stats);

        // Stale stats: should be adjusted upward
        assert!(core_stats.row_count > 1000.0);
        assert!(core_stats.row_count <= 1000.0 * 1.5);
    }

    #[test]
    fn adapter_column_statistics() {
        let profile = StatisticsProfile::standard();
        let adapter = StatisticsAdapter::new(profile);

        let mut table_stats = TableStatistics::new("products".to_string(), 1000);
        table_stats.columns.insert(
            "category".to_string(),
            ColumnStatistics {
                distinct_count: 50,
                null_fraction: 0.1,
                ..Default::default()
            },
        );

        let core_stats = adapter.to_core_statistics(&table_stats);

        let col = core_stats.columns.get("category").unwrap();
        assert!((col.distinct_count - 50.0).abs() < f64::EPSILON);
        assert!((col.null_fraction - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn simulator_fresh() {
        let sim = StatisticsSimulator::fresh();
        assert!(matches!(sim.staleness, Staleness::Fresh));
    }

    #[test]
    fn simulator_stale() {
        let sim = StatisticsSimulator::stale(1000);
        let state = sim.create_state();
        assert!(matches!(state.staleness(), Staleness::Stale));
    }

    #[test]
    fn profile_rejection() {
        let profile = StatisticsProfile::real_time(); // Strict
        let adapter = StatisticsAdapter::new(profile);

        let mut state = StatisticsState::default();
        state.record_modifications(100);

        // RealTime profile should reject stale stats
        assert!(adapter.should_reject_statistics(&state));
    }
}
