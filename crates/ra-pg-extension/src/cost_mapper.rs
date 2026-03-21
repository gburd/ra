//! Cost calibration between RA and PostgreSQL cost units.
//!
//! Maps RA's multi-component cost model (CPU, I/O, network, memory)
//! to PostgreSQL's single-number startup/total cost model. Tracks
//! estimation errors over time to refine calibration factors.

use ra_core::Cost;

use crate::pg_constants::cost_defaults;

/// Calibration factors mapping RA cost components to PG cost units.
///
/// PostgreSQL costs are expressed in units where a sequential page
/// fetch = 1.0 (by convention). RA uses arbitrary units, so we
/// need calibration multipliers discovered through feedback.
pub struct CostCalibration {
    /// Multiplier for RA CPU cost -> PG cost units.
    pub cpu_factor: f64,
    /// Multiplier for RA I/O cost -> PG cost units.
    pub io_factor: f64,
    /// Multiplier for RA network cost -> PG cost units.
    pub network_factor: f64,
    /// Running count of comparisons for error tracking.
    pub sample_count: u64,
    /// Mean absolute percentage error of recent calibrations.
    pub mean_error: f64,
}

impl CostCalibration {
    /// Initial calibration using PostgreSQL default cost parameters.
    ///
    /// Maps RA cost components to PostgreSQL cost units based on
    /// PostgreSQL's defaults documented in `pg_constants::cost_defaults`.
    pub fn default_calibration() -> Self {
        Self {
            cpu_factor: cost_defaults::CPU_TUPLE_COST,
            io_factor: cost_defaults::SEQ_PAGE_COST,
            network_factor: 0.5, // Network has no PostgreSQL equivalent; use heuristic
            sample_count: 0,
            mean_error: 0.0,
        }
    }

    /// Convert an RA cost to a single PostgreSQL total-cost number.
    pub fn ra_to_pg_total(&self, ra_cost: &Cost) -> f64 {
        ra_cost.cpu * self.cpu_factor
            + ra_cost.io * self.io_factor
            + ra_cost.network * self.network_factor
    }

    /// Decompose an RA cost into PostgreSQL startup and total costs.
    ///
    /// Uses the startup cost fields from RA's Cost struct directly
    /// when available. Falls back to the fraction-based estimate
    /// when `startup_fraction` is provided for backward compat.
    pub fn ra_to_pg_costs(
        &self,
        ra_cost: &Cost,
        startup_fraction: f64,
    ) -> PgCost {
        let total = self.ra_to_pg_total(ra_cost);
        let ra_startup = ra_cost.startup_cpu * self.cpu_factor
            + ra_cost.startup_io * self.io_factor
            + ra_cost.startup_network * self.network_factor;
        // Use RA startup cost if any startup component is non-zero;
        // otherwise fall back to fraction-based estimate.
        let startup = if ra_startup > 0.0 {
            ra_startup.min(total)
        } else {
            total * startup_fraction.clamp(0.0, 1.0)
        };
        PgCost { startup, total }
    }

    /// Record a calibration sample: the RA-predicted cost vs the
    /// PostgreSQL planner's cost for the same plan.
    ///
    /// Updates the running mean error for monitoring.
    pub fn record_sample(
        &mut self,
        ra_predicted: f64,
        pg_actual: f64,
    ) {
        if pg_actual <= 0.0 {
            return;
        }
        let error = ((ra_predicted - pg_actual) / pg_actual).abs();
        self.sample_count += 1;
        let n = self.sample_count as f64;
        self.mean_error += (error - self.mean_error) / n;
    }
}

/// PostgreSQL-style cost pair.
pub struct PgCost {
    /// Cost incurred before the first tuple is returned.
    pub startup: f64,
    /// Total cost to process all tuples.
    pub total: f64,
}

/// Startup-cost fractions for common operator types.
///
/// These are approximations matching PostgreSQL's behavior.
pub mod startup_fractions {
    /// Sequential scan: negligible startup.
    pub const SEQ_SCAN: f64 = 0.0;
    /// Index scan: small startup for tree descent.
    pub const INDEX_SCAN: f64 = 0.01;
    /// Sort: must read all input before emitting.
    pub const SORT: f64 = 0.9;
    /// Hash join build side: must build hash table first.
    pub const HASH_JOIN_BUILD: f64 = 0.5;
    /// Merge join: must sort both inputs first.
    pub const MERGE_JOIN: f64 = 0.4;
    /// Nested loop: negligible startup.
    pub const NESTED_LOOP: f64 = 0.0;
    /// Aggregate (hash): must process all input.
    pub const HASH_AGGREGATE: f64 = 0.9;
    /// Aggregate (sorted/streaming): minimal startup.
    pub const SORTED_AGGREGATE: f64 = 0.01;
    /// Limit: no additional startup.
    pub const LIMIT: f64 = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_calibration_smoke() {
        let cal = CostCalibration::default_calibration();
        assert!(cal.cpu_factor > 0.0);
        assert!(cal.io_factor > 0.0);
        assert_eq!(cal.sample_count, 0);
    }

    #[test]
    fn ra_to_pg_total_basic() {
        let cal = CostCalibration::default_calibration();
        let cost = Cost::new(100.0, 10.0, 0.0, 0);
        let pg = cal.ra_to_pg_total(&cost);
        // 100 * 0.01 + 10 * 1.0 + 0 * 0.5 = 1.0 + 10.0 = 11.0
        assert!((pg - 11.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ra_to_pg_costs_split() {
        let cal = CostCalibration::default_calibration();
        let cost = Cost::new(0.0, 100.0, 0.0, 0);
        let pg = cal.ra_to_pg_costs(&cost, startup_fractions::SORT);
        assert!((pg.total - 100.0).abs() < f64::EPSILON);
        assert!((pg.startup - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn startup_fraction_clamped() {
        let cal = CostCalibration::default_calibration();
        let cost = Cost::new(0.0, 100.0, 0.0, 0);
        let pg = cal.ra_to_pg_costs(&cost, 2.0);
        assert!((pg.startup - pg.total).abs() < f64::EPSILON);
    }

    #[test]
    fn record_sample_updates_error() {
        let mut cal = CostCalibration::default_calibration();
        cal.record_sample(100.0, 100.0);
        assert!(cal.mean_error < f64::EPSILON);
        assert_eq!(cal.sample_count, 1);

        cal.record_sample(150.0, 100.0);
        assert!(cal.mean_error > 0.0);
        assert_eq!(cal.sample_count, 2);
    }

    #[test]
    fn record_sample_skips_zero_actual() {
        let mut cal = CostCalibration::default_calibration();
        cal.record_sample(100.0, 0.0);
        assert_eq!(cal.sample_count, 0);
    }

    #[test]
    fn zero_cost_maps_to_zero() {
        let cal = CostCalibration::default_calibration();
        let pg = cal.ra_to_pg_total(&Cost::ZERO);
        assert!((pg - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn network_cost_contributes() {
        let cal = CostCalibration::default_calibration();
        let cost = Cost::new(0.0, 0.0, 100.0, 0);
        let pg = cal.ra_to_pg_total(&cost);
        // 100 * 0.5 = 50.0
        assert!((pg - 50.0).abs() < f64::EPSILON);
    }
}
