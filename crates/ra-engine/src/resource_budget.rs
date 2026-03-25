//! Resource budget system for constraining optimizer execution.
//!
//! Provides [`ResourceBudget`] for specifying constraints on time,
//! memory, e-graph size, and iteration count. The [`ResourceTracker`]
//! monitors usage during optimization, and [`OverflowStrategy`]
//! controls behavior when limits are exceeded.

use std::time::{Duration, Instant};

/// Constraints on optimizer resource consumption.
///
/// All fields are optional. An unset field means no constraint
/// for that resource. Use the predefined constructors for common
/// configurations.
#[derive(Debug, Clone)]
pub struct ResourceBudget {
    /// Wall-clock time limit.
    pub max_time: Option<Duration>,
    /// CPU time limit (approximated by wall-clock in this impl).
    pub max_cpu_time: Option<Duration>,
    /// Memory usage limit in bytes.
    pub max_memory: Option<u64>,
    /// Maximum number of e-graph nodes.
    pub max_egraph_nodes: Option<usize>,
    /// Maximum number of optimization iterations.
    pub max_iterations: Option<usize>,
    /// What to do when a limit is exceeded.
    pub overflow_strategy: OverflowStrategy,
}

impl ResourceBudget {
    /// Create a budget with no constraints.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_time: None,
            max_cpu_time: None,
            max_memory: None,
            max_egraph_nodes: None,
            max_iterations: None,
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
        }
    }

    /// Whether all fields are unconstrained.
    #[must_use]
    pub fn is_unlimited(&self) -> bool {
        self.max_time.is_none()
            && self.max_cpu_time.is_none()
            && self.max_memory.is_none()
            && self.max_egraph_nodes.is_none()
            && self.max_iterations.is_none()
    }

    /// Set the wall-clock time limit.
    #[must_use]
    pub fn with_time_limit(mut self, limit: Duration) -> Self {
        self.max_time = Some(limit);
        self
    }

    /// Set the CPU time limit.
    #[must_use]
    pub fn with_cpu_time_limit(mut self, limit: Duration) -> Self {
        self.max_cpu_time = Some(limit);
        self
    }

    /// Set the memory limit in bytes.
    #[must_use]
    pub fn with_memory_limit(mut self, bytes: u64) -> Self {
        self.max_memory = Some(bytes);
        self
    }

    /// Set the e-graph node limit.
    #[must_use]
    pub fn with_egraph_node_limit(mut self, limit: usize) -> Self {
        self.max_egraph_nodes = Some(limit);
        self
    }

    /// Set the iteration limit.
    #[must_use]
    pub fn with_iteration_limit(mut self, limit: usize) -> Self {
        self.max_iterations = Some(limit);
        self
    }

    /// Set the overflow strategy.
    #[must_use]
    pub fn with_overflow_strategy(
        mut self,
        strategy: OverflowStrategy,
    ) -> Self {
        self.overflow_strategy = strategy;
        self
    }
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// Strategy for handling resource limit overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowStrategy {
    /// Return the best plan found so far.
    ReturnBestSoFar,
    /// Return the original, unoptimized plan.
    ReturnOriginal,
    /// Fail with an error.
    Fail,
}

/// Tracks resource usage during optimization.
#[derive(Debug)]
pub struct ResourceTracker {
    budget: ResourceBudget,
    start_time: Instant,
    iterations_used: usize,
    peak_egraph_nodes: usize,
    peak_memory_estimate: u64,
}

impl ResourceTracker {
    /// Start tracking against the given budget.
    #[must_use]
    pub fn start(budget: ResourceBudget) -> Self {
        Self {
            budget,
            start_time: Instant::now(),
            iterations_used: 0,
            peak_egraph_nodes: 0,
            peak_memory_estimate: 0,
        }
    }

    /// Record one completed iteration.
    pub fn record_iteration(&mut self) {
        self.iterations_used += 1;
    }

    /// Record the current e-graph node count.
    pub fn record_egraph_nodes(&mut self, count: usize) {
        if count > self.peak_egraph_nodes {
            self.peak_egraph_nodes = count;
        }
    }

    /// Record an estimated memory usage sample.
    pub fn record_memory_estimate(&mut self, bytes: u64) {
        if bytes > self.peak_memory_estimate {
            self.peak_memory_estimate = bytes;
        }
    }

    /// Check whether any budget limit has been exceeded.
    #[must_use]
    pub fn check(&self) -> ResourceCheckResult {
        let elapsed = self.start_time.elapsed();

        if let Some(limit) = self.budget.max_time {
            if elapsed >= limit {
                return ResourceCheckResult::Exceeded(
                    ExceededResource::Time,
                );
            }
        }

        if let Some(limit) = self.budget.max_cpu_time {
            if elapsed >= limit {
                return ResourceCheckResult::Exceeded(
                    ExceededResource::CpuTime,
                );
            }
        }

        if let Some(limit) = self.budget.max_memory {
            if self.peak_memory_estimate >= limit {
                return ResourceCheckResult::Exceeded(
                    ExceededResource::Memory,
                );
            }
        }

        if let Some(limit) = self.budget.max_egraph_nodes {
            if self.peak_egraph_nodes >= limit {
                return ResourceCheckResult::Exceeded(
                    ExceededResource::EGraphNodes,
                );
            }
        }

        if let Some(limit) = self.budget.max_iterations {
            if self.iterations_used >= limit {
                return ResourceCheckResult::Exceeded(
                    ExceededResource::Iterations,
                );
            }
        }

        ResourceCheckResult::WithinBudget
    }

    /// Elapsed wall-clock time since tracking started.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Number of iterations completed.
    #[must_use]
    pub fn iterations_used(&self) -> usize {
        self.iterations_used
    }

    /// Peak observed e-graph node count.
    #[must_use]
    pub fn peak_egraph_nodes(&self) -> usize {
        self.peak_egraph_nodes
    }

    /// Peak estimated memory usage in bytes.
    #[must_use]
    pub fn peak_memory_estimate(&self) -> u64 {
        self.peak_memory_estimate
    }

    /// The overflow strategy from the budget.
    #[must_use]
    pub fn overflow_strategy(&self) -> OverflowStrategy {
        self.budget.overflow_strategy
    }

    /// Produce a usage report from the current state.
    #[must_use]
    pub fn report(&self) -> ResourceUsageReport {
        ResourceUsageReport {
            elapsed_time: self.start_time.elapsed(),
            iterations_used: self.iterations_used,
            peak_egraph_nodes: self.peak_egraph_nodes,
            peak_memory_estimate: self.peak_memory_estimate,
            budget_exceeded: match self.check() {
                ResourceCheckResult::WithinBudget => None,
                ResourceCheckResult::Exceeded(r) => Some(r),
            },
        }
    }
}

/// Result of checking resource usage against the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCheckResult {
    /// All resource usage is within budget.
    WithinBudget,
    /// A specific resource limit was exceeded.
    Exceeded(ExceededResource),
}

impl ResourceCheckResult {
    /// Whether usage is within budget.
    #[must_use]
    pub fn is_within_budget(&self) -> bool {
        matches!(self, Self::WithinBudget)
    }
}

/// Which resource limit was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceededResource {
    /// Wall-clock time limit.
    Time,
    /// CPU time limit.
    CpuTime,
    /// Memory usage limit.
    Memory,
    /// E-graph node count limit.
    EGraphNodes,
    /// Iteration count limit.
    Iterations,
}

impl std::fmt::Display for ExceededResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Time => write!(f, "wall-clock time"),
            Self::CpuTime => write!(f, "CPU time"),
            Self::Memory => write!(f, "memory"),
            Self::EGraphNodes => write!(f, "e-graph nodes"),
            Self::Iterations => write!(f, "iterations"),
        }
    }
}

/// Summary of resource usage after optimization.
#[derive(Debug, Clone)]
pub struct ResourceUsageReport {
    /// Total wall-clock time spent.
    pub elapsed_time: Duration,
    /// Number of optimization iterations completed.
    pub iterations_used: usize,
    /// Peak e-graph node count observed.
    pub peak_egraph_nodes: usize,
    /// Peak estimated memory usage in bytes.
    pub peak_memory_estimate: u64,
    /// Which resource was exceeded, if any.
    pub budget_exceeded: Option<ExceededResource>,
}

impl ResourceUsageReport {
    /// Whether the optimization completed within budget.
    #[must_use]
    pub fn completed_within_budget(&self) -> bool {
        self.budget_exceeded.is_none()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    // ---- ResourceBudget construction ----

    #[test]
    fn unlimited_budget_has_no_constraints() {
        let budget = ResourceBudget::unlimited();
        assert!(budget.is_unlimited());
        assert!(budget.max_time.is_none());
        assert!(budget.max_cpu_time.is_none());
        assert!(budget.max_memory.is_none());
        assert!(budget.max_egraph_nodes.is_none());
        assert!(budget.max_iterations.is_none());
    }

    #[test]
    fn default_budget_is_unlimited() {
        let budget = ResourceBudget::default();
        assert!(budget.is_unlimited());
    }

    #[test]
    fn with_time_limit_sets_constraint() {
        let budget = ResourceBudget::unlimited()
            .with_time_limit(Duration::from_millis(100));
        assert!(!budget.is_unlimited());
        assert_eq!(budget.max_time, Some(Duration::from_millis(100)));
    }

    #[test]
    fn with_cpu_time_limit_sets_constraint() {
        let budget = ResourceBudget::unlimited()
            .with_cpu_time_limit(Duration::from_secs(1));
        assert!(!budget.is_unlimited());
        assert_eq!(budget.max_cpu_time, Some(Duration::from_secs(1)));
    }

    #[test]
    fn with_memory_limit_sets_constraint() {
        let budget = ResourceBudget::unlimited()
            .with_memory_limit(50 * 1024 * 1024);
        assert!(!budget.is_unlimited());
        assert_eq!(budget.max_memory, Some(50 * 1024 * 1024));
    }

    #[test]
    fn with_egraph_node_limit_sets_constraint() {
        let budget = ResourceBudget::unlimited()
            .with_egraph_node_limit(10_000);
        assert!(!budget.is_unlimited());
        assert_eq!(budget.max_egraph_nodes, Some(10_000));
    }

    #[test]
    fn with_iteration_limit_sets_constraint() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(5);
        assert!(!budget.is_unlimited());
        assert_eq!(budget.max_iterations, Some(5));
    }

    #[test]
    fn with_overflow_strategy_sets_strategy() {
        let budget = ResourceBudget::unlimited()
            .with_overflow_strategy(OverflowStrategy::Fail);
        assert_eq!(budget.overflow_strategy, OverflowStrategy::Fail);
    }

    #[test]
    fn builder_chains_multiple_constraints() {
        let budget = ResourceBudget::unlimited()
            .with_time_limit(Duration::from_secs(1))
            .with_memory_limit(100 * 1024 * 1024)
            .with_iteration_limit(10)
            .with_overflow_strategy(OverflowStrategy::ReturnOriginal);
        assert!(!budget.is_unlimited());
        assert_eq!(budget.max_time, Some(Duration::from_secs(1)));
        assert_eq!(budget.max_memory, Some(100 * 1024 * 1024));
        assert_eq!(budget.max_iterations, Some(10));
        assert_eq!(
            budget.overflow_strategy,
            OverflowStrategy::ReturnOriginal
        );
    }

    #[test]
    fn default_overflow_strategy_is_return_best_so_far() {
        let budget = ResourceBudget::unlimited();
        assert_eq!(
            budget.overflow_strategy,
            OverflowStrategy::ReturnBestSoFar
        );
    }

    // ---- OverflowStrategy equality ----

    #[test]
    fn overflow_strategy_variants_are_distinct() {
        assert_ne!(
            OverflowStrategy::ReturnBestSoFar,
            OverflowStrategy::ReturnOriginal
        );
        assert_ne!(
            OverflowStrategy::ReturnBestSoFar,
            OverflowStrategy::Fail
        );
        assert_ne!(
            OverflowStrategy::ReturnOriginal,
            OverflowStrategy::Fail
        );
    }

    // ---- ResourceTracker creation and basic tracking ----

    #[test]
    fn tracker_starts_at_zero() {
        let tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        assert_eq!(tracker.iterations_used(), 0);
        assert_eq!(tracker.peak_egraph_nodes(), 0);
        assert_eq!(tracker.peak_memory_estimate(), 0);
    }

    #[test]
    fn tracker_records_iterations() {
        let mut tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        tracker.record_iteration();
        tracker.record_iteration();
        tracker.record_iteration();
        assert_eq!(tracker.iterations_used(), 3);
    }

    #[test]
    fn tracker_records_peak_egraph_nodes() {
        let mut tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        tracker.record_egraph_nodes(100);
        tracker.record_egraph_nodes(500);
        tracker.record_egraph_nodes(200);
        assert_eq!(tracker.peak_egraph_nodes(), 500);
    }

    #[test]
    fn tracker_records_peak_memory() {
        let mut tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        tracker.record_memory_estimate(1000);
        tracker.record_memory_estimate(5000);
        tracker.record_memory_estimate(2000);
        assert_eq!(tracker.peak_memory_estimate(), 5000);
    }

    #[test]
    fn tracker_elapsed_returns_valid_duration() {
        let tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        // Duration is always non-negative by construction;
        // verify we get a finite duration back.
        let _elapsed = tracker.elapsed();
    }

    // ---- ResourceCheckResult ----

    #[test]
    fn unlimited_budget_always_within_budget() {
        let tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        assert_eq!(tracker.check(), ResourceCheckResult::WithinBudget);
        assert!(tracker.check().is_within_budget());
    }

    #[test]
    fn iteration_limit_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(2);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_iteration();
        assert!(tracker.check().is_within_budget());
        tracker.record_iteration();
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::Iterations)
        );
    }

    #[test]
    fn egraph_node_limit_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_egraph_node_limit(1000);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_egraph_nodes(500);
        assert!(tracker.check().is_within_budget());
        tracker.record_egraph_nodes(1000);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(
                ExceededResource::EGraphNodes
            )
        );
    }

    #[test]
    fn memory_limit_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_memory_limit(1024);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_memory_estimate(512);
        assert!(tracker.check().is_within_budget());
        tracker.record_memory_estimate(1024);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::Memory)
        );
    }

    #[test]
    fn time_limit_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_time_limit(Duration::from_millis(0));
        let tracker = ResourceTracker::start(budget);
        // Even a zero-duration limit should be exceeded immediately
        // (or nearly so). We spin briefly to ensure time passes.
        std::thread::sleep(Duration::from_millis(1));
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::Time)
        );
    }

    #[test]
    fn cpu_time_limit_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_cpu_time_limit(Duration::from_millis(0));
        let tracker = ResourceTracker::start(budget);
        std::thread::sleep(Duration::from_millis(1));
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::CpuTime)
        );
    }

    #[test]
    fn check_result_is_within_budget_true() {
        assert!(ResourceCheckResult::WithinBudget.is_within_budget());
    }

    #[test]
    fn check_result_is_within_budget_false() {
        let exceeded = ResourceCheckResult::Exceeded(
            ExceededResource::Time,
        );
        assert!(!exceeded.is_within_budget());
    }

    // ---- ExceededResource Display ----

    #[test]
    fn exceeded_resource_display_time() {
        assert_eq!(ExceededResource::Time.to_string(), "wall-clock time");
    }

    #[test]
    fn exceeded_resource_display_cpu_time() {
        assert_eq!(ExceededResource::CpuTime.to_string(), "CPU time");
    }

    #[test]
    fn exceeded_resource_display_memory() {
        assert_eq!(ExceededResource::Memory.to_string(), "memory");
    }

    #[test]
    fn exceeded_resource_display_egraph_nodes() {
        assert_eq!(
            ExceededResource::EGraphNodes.to_string(),
            "e-graph nodes"
        );
    }

    #[test]
    fn exceeded_resource_display_iterations() {
        assert_eq!(
            ExceededResource::Iterations.to_string(),
            "iterations"
        );
    }

    // ---- ResourceTracker overflow_strategy ----

    #[test]
    fn tracker_returns_budget_overflow_strategy() {
        let budget = ResourceBudget::unlimited()
            .with_overflow_strategy(OverflowStrategy::Fail);
        let tracker = ResourceTracker::start(budget);
        assert_eq!(
            tracker.overflow_strategy(),
            OverflowStrategy::Fail
        );
    }

    // ---- ResourceUsageReport ----

    #[test]
    fn report_within_budget() {
        let tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        let report = tracker.report();
        assert!(report.completed_within_budget());
        assert!(report.budget_exceeded.is_none());
        assert_eq!(report.iterations_used, 0);
        assert_eq!(report.peak_egraph_nodes, 0);
        assert_eq!(report.peak_memory_estimate, 0);
    }

    #[test]
    fn report_exceeded_budget() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(1);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_iteration();
        let report = tracker.report();
        assert!(!report.completed_within_budget());
        assert_eq!(
            report.budget_exceeded,
            Some(ExceededResource::Iterations)
        );
    }

    #[test]
    fn report_captures_all_metrics() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(100);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_iteration();
        tracker.record_iteration();
        tracker.record_egraph_nodes(500);
        tracker.record_memory_estimate(2048);
        let report = tracker.report();
        assert_eq!(report.iterations_used, 2);
        assert_eq!(report.peak_egraph_nodes, 500);
        assert_eq!(report.peak_memory_estimate, 2048);
        // Elapsed time should be set (Duration is always non-negative)
        let _ = report.elapsed_time;
    }

    // ---- Edge cases ----

    #[test]
    fn zero_iteration_limit_immediately_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(0);
        let tracker = ResourceTracker::start(budget);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(
                ExceededResource::Iterations
            )
        );
    }

    #[test]
    fn zero_egraph_node_limit_immediately_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_egraph_node_limit(0);
        let tracker = ResourceTracker::start(budget);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(
                ExceededResource::EGraphNodes
            )
        );
    }

    #[test]
    fn zero_memory_limit_immediately_exceeded() {
        let budget = ResourceBudget::unlimited()
            .with_memory_limit(0);
        let tracker = ResourceTracker::start(budget);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::Memory)
        );
    }

    #[test]
    fn multiple_limits_first_exceeded_wins() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(2)
            .with_egraph_node_limit(1000);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_egraph_nodes(1000);
        tracker.record_iteration();
        // E-graph check comes before iteration check in order
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(
                ExceededResource::EGraphNodes
            )
        );
    }

    #[test]
    fn peak_tracking_never_decreases() {
        let mut tracker = ResourceTracker::start(
            ResourceBudget::unlimited(),
        );
        tracker.record_egraph_nodes(1000);
        tracker.record_egraph_nodes(500);
        assert_eq!(tracker.peak_egraph_nodes(), 1000);

        tracker.record_memory_estimate(5000);
        tracker.record_memory_estimate(3000);
        assert_eq!(tracker.peak_memory_estimate(), 5000);
    }

    #[test]
    fn large_budget_values_work() {
        let budget = ResourceBudget::unlimited()
            .with_memory_limit(u64::MAX)
            .with_egraph_node_limit(usize::MAX)
            .with_iteration_limit(usize::MAX);
        let tracker = ResourceTracker::start(budget);
        assert!(tracker.check().is_within_budget());
    }
}
