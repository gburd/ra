//! Cost model traits and types for query optimization.
//!
//! The cost model assigns a numeric cost to each relational expression,
//! allowing the optimizer to compare alternative plans and choose the
//! cheapest one.

use serde::{Deserialize, Serialize};

use crate::algebra::RelExpr;
use crate::statistics::Statistics;

/// A cost estimate for a query plan or sub-plan.
///
/// Costs are broken into CPU and I/O components so that different
/// hardware profiles can weight them differently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Estimated CPU cost (arbitrary units).
    pub cpu: f64,
    /// Estimated I/O cost (arbitrary units).
    pub io: f64,
    /// Estimated network cost (arbitrary units, for distributed plans).
    pub network: f64,
    /// Estimated memory usage in bytes.
    pub memory: u64,
}

impl Cost {
    /// A zero cost.
    pub const ZERO: Self = Self {
        cpu: 0.0,
        io: 0.0,
        network: 0.0,
        memory: 0,
    };

    /// Create a new cost estimate.
    #[must_use]
    pub fn new(cpu: f64, io: f64, network: f64, memory: u64) -> Self {
        Self {
            cpu,
            io,
            network,
            memory,
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

    /// Return a weighted total cost.
    #[must_use]
    pub fn weighted_total(
        &self,
        cpu_weight: f64,
        io_weight: f64,
        network_weight: f64,
    ) -> f64 {
        self.cpu * cpu_weight
            + self.io * io_weight
            + self.network * network_weight
    }

    /// Add two costs component-wise.
    #[must_use]
    pub fn add(&self, other: &Self) -> Self {
        Self {
            cpu: self.cpu + other.cpu,
            io: self.io + other.io,
            network: self.network + other.network,
            memory: self.memory.saturating_add(other.memory),
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
    fn estimate(
        &self,
        expr: &RelExpr,
        statistics: &dyn StatisticsProvider,
    ) -> Cost;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_zero() {
        let c = Cost::ZERO;
        assert_eq!(c.cpu, 0.0);
        assert_eq!(c.io, 0.0);
        assert_eq!(c.network, 0.0);
        assert_eq!(c.memory, 0);
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
    fn default_cost_function() {
        let cf = DefaultCostFunction;
        let cheap = Cost::new(1.0, 1.0, 0.0, 0);
        let expensive = Cost::new(100.0, 100.0, 0.0, 0);
        assert!(cf.is_cheaper(&cheap, &expensive));
        assert!(!cf.is_cheaper(&expensive, &cheap));
    }

    #[test]
    fn serialize_roundtrip() {
        let cost = Cost::new(1.5, 2.5, 0.0, 4096);
        let json = serde_json::to_string(&cost)
            .expect("serialization should succeed");
        let deserialized: Cost = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(cost, deserialized);
    }
}
