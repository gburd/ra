//! Cost model traits and types for query optimization.
//!
//! The cost model assigns a numeric cost to each relational expression,
//! allowing the optimizer to compare alternative plans and choose the
//! cheapest one.
//!
//! Following `PostgreSQL`'s approach, each cost has both a **startup**
//! component (cost to produce the first row) and a **total** component
//! (cost to process all rows). This distinction is critical for:
//! - LIMIT queries: prefer plans with low startup cost
//! - Nested loop inner side: startup cost is paid on every rescan
//! - Pipelined vs blocking operators: pipelined operators have
//!   near-zero startup cost

use serde::{Deserialize, Serialize};

use crate::algebra::RelExpr;
use crate::statistics::Statistics;

/// A cost estimate for a query plan or sub-plan.
///
/// Costs are broken into CPU and I/O components so that different
/// hardware profiles can weight them differently. Each component
/// tracks both startup cost (to produce the first row) and total
/// cost (to process all rows).
///
/// Invariant: startup <= total for each component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Estimated CPU cost (arbitrary units) -- total.
    pub cpu: f64,
    /// Estimated I/O cost (arbitrary units) -- total.
    pub io: f64,
    /// Estimated network cost (arbitrary units) -- total.
    pub network: f64,
    /// Estimated memory usage in bytes.
    pub memory: u64,
    /// Startup CPU cost (cost before first row is produced).
    #[serde(default)]
    pub startup_cpu: f64,
    /// Startup I/O cost (cost before first row is produced).
    #[serde(default)]
    pub startup_io: f64,
    /// Startup network cost (cost before first row is produced).
    #[serde(default)]
    pub startup_network: f64,
}

impl Cost {
    /// A zero cost.
    pub const ZERO: Self = Self {
        cpu: 0.0,
        io: 0.0,
        network: 0.0,
        memory: 0,
        startup_cpu: 0.0,
        startup_io: 0.0,
        startup_network: 0.0,
    };

    /// Create a new cost estimate with zero startup cost.
    ///
    /// This is the common case for pipelined operators (scans,
    /// filters, projections) that can produce rows immediately.
    #[must_use]
    pub fn new(cpu: f64, io: f64, network: f64, memory: u64) -> Self {
        Self {
            cpu,
            io,
            network,
            memory,
            startup_cpu: 0.0,
            startup_io: 0.0,
            startup_network: 0.0,
        }
    }

    /// Create a cost estimate with explicit startup costs.
    ///
    /// Use this for blocking operators (sort, hash build, aggregate)
    /// where significant work happens before the first row is
    /// produced.
    #[must_use]
    pub fn with_startup(
        cpu: f64,
        io: f64,
        network: f64,
        memory: u64,
        startup_cpu: f64,
        startup_io: f64,
        startup_network: f64,
    ) -> Self {
        Self {
            cpu,
            io,
            network,
            memory,
            startup_cpu: startup_cpu.min(cpu),
            startup_io: startup_io.min(io),
            startup_network: startup_network.min(network),
        }
    }

    /// Return the total cost as a single scalar.
    ///
    /// Uses default weights of 1.0 for CPU, 4.0 for I/O, and 2.0
    /// for network. For custom weighting, use [`Self::weighted_total`].
    #[must_use]
    pub fn total(&self) -> f64 {
        self.weighted_total(1.0, 4.0, 2.0)
    }

    /// Return the startup cost as a single scalar.
    ///
    /// Uses the same default weights as [`Self::total`].
    #[must_use]
    pub fn startup(&self) -> f64 {
        self.weighted_startup(1.0, 4.0, 2.0)
    }

    /// Return the run cost (total minus startup) as a single scalar.
    #[must_use]
    pub fn run(&self) -> f64 {
        self.total() - self.startup()
    }

    /// Return a weighted total cost.
    #[must_use]
    pub fn weighted_total(&self, cpu_weight: f64, io_weight: f64, network_weight: f64) -> f64 {
        self.cpu * cpu_weight + self.io * io_weight + self.network * network_weight
    }

    /// Return a weighted startup cost.
    #[must_use]
    pub fn weighted_startup(&self, cpu_weight: f64, io_weight: f64, network_weight: f64) -> f64 {
        self.startup_cpu * cpu_weight
            + self.startup_io * io_weight
            + self.startup_network * network_weight
    }

    /// Add two costs component-wise.
    ///
    /// Both startup and total components are summed. This models
    /// sequential execution of two plan nodes.
    #[must_use]
    pub fn add(&self, other: &Self) -> Self {
        Self {
            cpu: self.cpu + other.cpu,
            io: self.io + other.io,
            network: self.network + other.network,
            memory: self.memory.saturating_add(other.memory),
            startup_cpu: self.startup_cpu + other.startup_cpu,
            startup_io: self.startup_io + other.startup_io,
            startup_network: self.startup_network + other.startup_network,
        }
    }

    /// Compute cost for a LIMIT operator over this plan.
    ///
    /// For LIMIT k over n rows, the effective cost is:
    ///   startup + run * (k / n)
    ///
    /// This correctly models early termination: blocking operators
    /// pay full startup cost, then only a fraction of run cost.
    #[must_use]
    pub fn limit_cost(&self, limit_rows: f64, total_rows: f64) -> Self {
        if total_rows <= 0.0 || limit_rows >= total_rows {
            return self.clone();
        }
        let fraction = limit_rows / total_rows;
        let run_cpu = self.cpu - self.startup_cpu;
        let run_io = self.io - self.startup_io;
        let run_network = self.network - self.startup_network;
        Self {
            cpu: self.startup_cpu + run_cpu * fraction,
            io: self.startup_io + run_io * fraction,
            network: self.startup_network + run_network * fraction,
            memory: self.memory,
            startup_cpu: self.startup_cpu,
            startup_io: self.startup_io,
            startup_network: self.startup_network,
        }
    }

    /// Compute cost for nested loop inner side.
    ///
    /// The inner side is rescanned `outer_rows` times. Each rescan
    /// pays the full total cost, but the first scan also pays
    /// startup cost. Following `PostgreSQL`:
    ///   `total = outer.total + outer_rows * inner.total`
    ///   `startup = outer.startup`
    #[must_use]
    pub fn nested_loop_inner_cost(&self, outer_rows: f64) -> Self {
        Self {
            cpu: self.cpu * outer_rows,
            io: self.io * outer_rows,
            network: self.network * outer_rows,
            memory: self.memory,
            startup_cpu: self.startup_cpu,
            startup_io: self.startup_io,
            startup_network: self.startup_network,
        }
    }
}

/// A cost model that estimates the cost of executing a relational
/// expression given statistics about its inputs.
pub trait CostModel: std::fmt::Debug + Send + Sync {
    /// Estimate the cost of the given expression.
    ///
    /// The `statistics` function is called to look up statistics for
    /// base tables referenced by the expression.
    fn estimate(&self, expr: &RelExpr, statistics: &dyn StatisticsProvider) -> Cost;
}

/// Provides statistics for base tables.
pub trait StatisticsProvider: std::fmt::Debug + Send + Sync {
    /// Look up statistics for the named table.
    fn get_statistics(&self, table: &str) -> Option<&Statistics>;
}

/// A cost function that can be used as a comparator.
pub trait CostFunction: std::fmt::Debug + Send + Sync {
    /// Compare two costs, returning true if `a` is cheaper than `b`.
    fn is_cheaper(&self, a: &Cost, b: &Cost) -> bool;
}

/// Default cost comparator that uses [`Cost::total`].
#[derive(Debug, Clone, Copy)]
pub struct DefaultCostFunction;

impl CostFunction for DefaultCostFunction {
    fn is_cheaper(&self, a: &Cost, b: &Cost) -> bool {
        a.total() < b.total()
    }
}

/// Cost comparator that prefers low startup cost.
///
/// Used when the query has a LIMIT or when evaluating the inner
/// side of a nested loop join. Plans that can produce the first
/// row sooner are preferred.
#[derive(Debug, Clone, Copy)]
pub struct StartupCostFunction;

impl CostFunction for StartupCostFunction {
    fn is_cheaper(&self, a: &Cost, b: &Cost) -> bool {
        a.startup() < b.startup()
    }
}

/// Cost comparator for LIMIT queries.
///
/// Blends startup and total cost based on the fraction of rows
/// the LIMIT will consume. When `fraction` is small (e.g., LIMIT
/// 10 on a million-row table), startup cost dominates. When
/// `fraction` is close to 1.0, total cost dominates.
#[derive(Debug, Clone, Copy)]
pub struct LimitCostFunction {
    /// Fraction of rows consumed (`limit_rows / total_rows`).
    pub fraction: f64,
}

impl CostFunction for LimitCostFunction {
    fn is_cheaper(&self, a: &Cost, b: &Cost) -> bool {
        let f = self.fraction.clamp(0.0, 1.0);
        let cost_a = a.startup() + a.run() * f;
        let cost_b = b.startup() + b.run() * f;
        cost_a < cost_b
    }
}

#[cfg(test)]
#[expect(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn cost_zero() {
        let c = Cost::ZERO;
        assert_eq!(c.cpu, 0.0);
        assert_eq!(c.io, 0.0);
        assert_eq!(c.network, 0.0);
        assert_eq!(c.memory, 0);
        assert_eq!(c.startup_cpu, 0.0);
        assert_eq!(c.startup_io, 0.0);
        assert_eq!(c.startup_network, 0.0);
    }

    #[test]
    fn cost_new_has_zero_startup() {
        let c = Cost::new(10.0, 5.0, 2.0, 1024);
        assert_eq!(c.startup_cpu, 0.0);
        assert_eq!(c.startup_io, 0.0);
        assert_eq!(c.startup_network, 0.0);
        assert_eq!(c.startup(), 0.0);
    }

    #[test]
    fn cost_total() {
        let c = Cost::new(10.0, 5.0, 2.0, 1024);
        // 10*1 + 5*4 + 2*2 = 10 + 20 + 4 = 34
        let total = c.total();
        assert!((total - 34.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_weighted_total() {
        let c = Cost::new(10.0, 5.0, 2.0, 0);
        let total = c.weighted_total(2.0, 1.0, 0.5);
        // 10*2 + 5*1 + 2*0.5 = 20 + 5 + 1 = 26
        assert!((total - 26.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_add() {
        let a = Cost::new(1.0, 2.0, 3.0, 100);
        let b = Cost::new(4.0, 5.0, 6.0, 200);
        let sum = a.add(&b);
        assert_eq!(sum.cpu, 5.0);
        assert_eq!(sum.io, 7.0);
        assert_eq!(sum.network, 9.0);
        assert_eq!(sum.memory, 300);
    }

    #[test]
    fn cost_add_memory_saturates() {
        let a = Cost::new(0.0, 0.0, 0.0, u64::MAX);
        let b = Cost::new(0.0, 0.0, 0.0, 1);
        let sum = a.add(&b);
        assert_eq!(sum.memory, u64::MAX);
    }

    #[test]
    fn cost_add_sums_startup() {
        let a = Cost::with_startup(10.0, 0.0, 0.0, 0, 5.0, 0.0, 0.0);
        let b = Cost::with_startup(20.0, 0.0, 0.0, 0, 8.0, 0.0, 0.0);
        let sum = a.add(&b);
        assert_eq!(sum.startup_cpu, 13.0);
        assert_eq!(sum.cpu, 30.0);
    }

    #[test]
    fn default_cost_function() {
        let cf = DefaultCostFunction;
        let cheap = Cost::new(1.0, 1.0, 0.0, 0);
        let expensive = Cost::new(100.0, 100.0, 0.0, 0);
        assert!(cf.is_cheaper(&cheap, &expensive));
        assert!(!cf.is_cheaper(&expensive, &cheap));
    }

    #[test]
    #[expect(clippy::expect_used)]
    fn serialize_roundtrip() {
        let cost = Cost::new(1.5, 2.5, 0.0, 4096);
        let json = serde_json::to_string(&cost).expect("serialization should succeed");
        let deserialized: Cost =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(cost, deserialized);
    }

    // ---- startup cost ----

    #[test]
    fn with_startup_sets_fields() {
        let c = Cost::with_startup(100.0, 50.0, 10.0, 1024, 80.0, 40.0, 5.0);
        assert_eq!(c.cpu, 100.0);
        assert_eq!(c.io, 50.0);
        assert_eq!(c.startup_cpu, 80.0);
        assert_eq!(c.startup_io, 40.0);
        assert_eq!(c.startup_network, 5.0);
    }

    #[test]
    fn with_startup_clamps_to_total() {
        let c = Cost::with_startup(10.0, 5.0, 2.0, 0, 999.0, 999.0, 999.0);
        assert_eq!(c.startup_cpu, 10.0);
        assert_eq!(c.startup_io, 5.0);
        assert_eq!(c.startup_network, 2.0);
    }

    #[test]
    fn startup_accessor() {
        let c = Cost::with_startup(100.0, 50.0, 10.0, 0, 80.0, 40.0, 5.0);
        // startup = 80*1 + 40*4 + 5*2 = 80 + 160 + 10 = 250
        assert!((c.startup() - 250.0).abs() < f64::EPSILON);
    }

    #[test]
    fn run_cost_is_total_minus_startup() {
        let c = Cost::with_startup(100.0, 50.0, 10.0, 0, 80.0, 40.0, 5.0);
        let expected = c.total() - c.startup();
        assert!((c.run() - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn pipelined_operator_zero_startup() {
        let c = Cost::new(100.0, 50.0, 0.0, 0);
        assert_eq!(c.startup(), 0.0);
        assert!((c.run() - c.total()).abs() < f64::EPSILON);
    }

    // ---- startup cost function ----

    #[test]
    fn startup_cost_function_prefers_low_startup() {
        let cf = StartupCostFunction;
        let pipelined = Cost::new(1000.0, 0.0, 0.0, 0);
        let blocking = Cost::with_startup(500.0, 0.0, 0.0, 0, 400.0, 0.0, 0.0);
        // pipelined has startup=0, blocking has startup=400
        assert!(cf.is_cheaper(&pipelined, &blocking));
        assert!(!cf.is_cheaper(&blocking, &pipelined));
    }

    // ---- limit cost function ----

    #[test]
    fn limit_cost_function_small_fraction_prefers_pipelined() {
        let cf = LimitCostFunction { fraction: 0.001 };
        let pipelined = Cost::new(1000.0, 0.0, 0.0, 0);
        let blocking = Cost::with_startup(500.0, 0.0, 0.0, 0, 400.0, 0.0, 0.0);
        // pipelined: 0 + 1000 * 0.001 = 1.0
        // blocking: 400 + 100 * 0.001 = 400.1
        assert!(cf.is_cheaper(&pipelined, &blocking));
    }

    #[test]
    fn limit_cost_function_full_fraction_uses_total() {
        let cf = LimitCostFunction { fraction: 1.0 };
        let a = Cost::new(100.0, 0.0, 0.0, 0);
        let b = Cost::new(200.0, 0.0, 0.0, 0);
        assert!(cf.is_cheaper(&a, &b));
    }

    // ---- limit_cost ----

    #[test]
    fn limit_cost_pipelined_scales_linearly() {
        let c = Cost::new(1000.0, 0.0, 0.0, 0);
        let limited = c.limit_cost(10.0, 1000.0);
        // startup=0, run=1000, fraction=0.01
        // total = 0 + 1000*0.01 = 10.0
        assert!((limited.cpu - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn limit_cost_blocking_pays_full_startup() {
        let c = Cost::with_startup(1000.0, 0.0, 0.0, 0, 900.0, 0.0, 0.0);
        let limited = c.limit_cost(10.0, 1000.0);
        // startup=900, run=100, fraction=0.01
        // total = 900 + 100*0.01 = 901.0
        assert!((limited.cpu - 901.0).abs() < f64::EPSILON);
        assert_eq!(limited.startup_cpu, 900.0);
    }

    #[test]
    fn limit_cost_limit_exceeds_total_rows() {
        let c = Cost::new(100.0, 0.0, 0.0, 0);
        let limited = c.limit_cost(2000.0, 1000.0);
        assert_eq!(limited.cpu, c.cpu);
    }

    #[test]
    fn limit_cost_zero_total_rows() {
        let c = Cost::new(100.0, 0.0, 0.0, 0);
        let limited = c.limit_cost(10.0, 0.0);
        assert_eq!(limited.cpu, c.cpu);
    }

    // ---- nested_loop_inner_cost ----

    #[test]
    fn nested_loop_inner_cost_multiplies() {
        let inner = Cost::with_startup(10.0, 5.0, 0.0, 100, 2.0, 1.0, 0.0);
        let rescanned = inner.nested_loop_inner_cost(100.0);
        assert!((rescanned.cpu - 1000.0).abs() < f64::EPSILON);
        assert!((rescanned.io - 500.0).abs() < f64::EPSILON);
        assert_eq!(rescanned.startup_cpu, 2.0);
        assert_eq!(rescanned.startup_io, 1.0);
    }

    // ---- weighted_startup ----

    #[test]
    fn weighted_startup() {
        let c = Cost::with_startup(100.0, 50.0, 10.0, 0, 80.0, 40.0, 5.0);
        let ws = c.weighted_startup(2.0, 1.0, 0.5);
        // 80*2 + 40*1 + 5*0.5 = 160 + 40 + 2.5 = 202.5
        assert!((ws - 202.5).abs() < f64::EPSILON);
    }

    // ---- serialize with startup fields ----

    #[test]
    #[expect(clippy::expect_used)]
    fn serialize_roundtrip_with_startup() {
        let cost = Cost::with_startup(100.0, 50.0, 10.0, 4096, 80.0, 40.0, 5.0);
        let json = serde_json::to_string(&cost).expect("serialization should succeed");
        let deserialized: Cost =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(cost, deserialized);
    }

    // ---- backward compat: old JSON without startup fields ----

    #[test]
    #[expect(clippy::expect_used)]
    fn deserialize_legacy_json_without_startup() {
        let json = r#"{"cpu":10.0,"io":5.0,"network":2.0,"memory":1024}"#;
        let cost: Cost = serde_json::from_str(json).expect("should deserialize legacy format");
        assert_eq!(cost.cpu, 10.0);
        assert_eq!(cost.startup_cpu, 0.0);
        assert_eq!(cost.startup_io, 0.0);
        assert_eq!(cost.startup_network, 0.0);
    }
}
