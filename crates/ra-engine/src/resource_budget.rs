//! Resource budget system for constraining optimizer execution.
//!
//! Provides [`ResourceBudget`] for specifying constraints on time,
//! memory, e-graph size, and iteration count. The [`ResourceTracker`]
//! monitors usage during optimization, and [`OverflowStrategy`]
//! controls behavior when limits are exceeded.
//!
//! Workload-specific controls ([`RuleSelectionBehavior`],
//! [`ConvergenceBehavior`], [`FastPathPreferences`]) allow budgets
//! to influence rule filtering, convergence detection, and fast-path
//! routing decisions.

use std::time::{Duration, Instant};

pub use crate::shortcuts::fast_path::FastPathPreferences;

// ── Workload-specific optimization controls ─────────────────────

/// Controls how the rule advisor filters and selects rules.
///
/// Different workloads benefit from different rule selection
/// strategies. OLTP queries need fast, focused rule sets while
/// analytical queries benefit from broader exploration.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleSelectionBehavior {
    /// Filter rules based on facts detected in the query plan.
    /// When true, the advisor skips rules whose preconditions
    /// cannot be satisfied by the current fact context.
    pub fact_based_filtering: bool,
    /// Use historical success rates to rank and prune rules.
    /// Requires the rule knowledge store to be populated.
    pub adaptive_learning: bool,
    /// Minimum historical success rate (0.0..=1.0) a rule must
    /// have before it is considered for application. Rules with
    /// fewer than `min_observations` data points are exempt.
    pub success_rate_threshold: f64,
    /// Minimum number of observations before a rule's success
    /// rate is trusted for filtering decisions.
    pub min_observations: u32,
    /// Maximum number of rules to apply per iteration.
    /// `None` means no limit beyond what filtering removes.
    pub max_rules_per_iteration: Option<usize>,
}

impl Default for RuleSelectionBehavior {
    fn default() -> Self {
        Self {
            fact_based_filtering: true,
            adaptive_learning: false,
            success_rate_threshold: 0.0,
            min_observations: 10,
            max_rules_per_iteration: None,
        }
    }
}

/// Controls when the optimizer considers the plan "good enough"
/// and terminates iteration.
///
/// Ordered from most aggressive (fewest iterations) to most
/// thorough (exhaustive exploration).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvergenceBehavior {
    /// Stop at the first plan improvement found. Minimizes
    /// optimization latency at the cost of plan quality.
    Immediate,
    /// Monitor e-graph growth and stop when progress stalls.
    /// Uses a sliding window to detect diminishing returns.
    /// This is the default for most workloads.
    Adaptive,
    /// Continue until growth rate drops below a strict threshold
    /// or the iteration limit is reached. Finds better plans
    /// but uses more resources.
    Thorough,
    /// Run until the iteration limit or equality saturation,
    /// whichever comes first. Only for research/benchmarking
    /// where plan quality must be maximized.
    Complete,
}

impl ConvergenceBehavior {
    /// Convergence window size: how many consecutive iterations
    /// must show low growth before declaring convergence.
    #[must_use]
    pub fn window_size(self) -> usize {
        match self {
            Self::Immediate => 1,
            Self::Adaptive => 3,
            Self::Thorough => 5,
            Self::Complete => usize::MAX,
        }
    }

    /// Minimum e-graph growth rate to consider an iteration
    /// productive. Below this threshold the window counter
    /// advances toward convergence.
    #[must_use]
    pub fn min_growth_rate(self) -> f64 {
        match self {
            Self::Immediate => 1.0,
            Self::Adaptive => 0.05,
            Self::Thorough => 0.01,
            Self::Complete => 0.0,
        }
    }
}

// ── Core budget struct ──────────────────────────────────────────

/// Constraints on optimizer resource consumption.
///
/// All resource fields are optional. An unset field means no
/// constraint for that resource. Use the predefined constructors
/// for common configurations.
///
/// The workload-specific controls ([`rule_selection`],
/// [`convergence`], [`fast_path`]) influence optimizer behavior
/// beyond simple resource caps. They default to sensible values
/// and can be overridden with the builder methods.
///
/// [`rule_selection`]: ResourceBudget::rule_selection
/// [`convergence`]: ResourceBudget::convergence
/// [`fast_path`]: ResourceBudget::fast_path
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
    /// How rules are filtered and selected per iteration.
    pub rule_selection: RuleSelectionBehavior,
    /// When the optimizer should declare convergence.
    pub convergence: ConvergenceBehavior,
    /// Which fast-path shortcuts are enabled.
    pub fast_path: FastPathPreferences,
}

impl ResourceBudget {
    /// Create a budget with no constraints except a safety limit on iterations.
    ///
    /// Even "unlimited" budgets need an iteration cap to prevent infinite loops
    /// in pathological cases (e.g., complex join orderings, recursive rules).
    /// Set to 1000 iterations - enough for complex queries, but prevents hangs.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_time: None,
            max_cpu_time: None,
            max_memory: None,
            max_egraph_nodes: None,
            max_iterations: Some(1000),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior::default(),
            convergence: ConvergenceBehavior::Adaptive,
            fast_path: FastPathPreferences::default(),
        }
    }

    // ── Workload-specific constructors ──────────────────────

    /// OLTP workload: low-latency, focused rule selection.
    ///
    /// Tight time and memory budgets. Fact-based filtering
    /// aggressively prunes irrelevant rules. Adaptive learning
    /// is enabled with a moderate success threshold to avoid
    /// wasting cycles on historically poor rules. All fast-path
    /// shortcuts are enabled for common OLTP patterns.
    #[must_use]
    pub fn oltp() -> Self {
        Self {
            max_time: Some(Duration::from_millis(200)),
            max_cpu_time: Some(Duration::from_millis(200)),
            max_memory: Some(100 * 1024 * 1024), // 100 MB
            max_egraph_nodes: Some(20_000),
            max_iterations: Some(15),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior {
                fact_based_filtering: true,
                adaptive_learning: true,
                success_rate_threshold: 0.05,
                min_observations: 10,
                max_rules_per_iteration: Some(50),
            },
            convergence: ConvergenceBehavior::Adaptive,
            fast_path: FastPathPreferences::oltp(),
        }
    }

    /// OLAP workload: thorough optimization for analytical queries.
    ///
    /// Generous time and memory budgets to allow deep exploration
    /// of join orderings and aggregation strategies. Learning is
    /// enabled with a low threshold to retain most rules. Fast
    /// paths use conservative OLAP settings since analytical
    /// queries generally need full optimization.
    #[must_use]
    pub fn olap() -> Self {
        Self {
            max_time: Some(Duration::from_secs(15)),
            max_cpu_time: Some(Duration::from_secs(15)),
            max_memory: Some(4 * 1024 * 1024 * 1024), // 4 GB
            max_egraph_nodes: Some(2_000_000),
            max_iterations: Some(150),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior {
                fact_based_filtering: true,
                adaptive_learning: true,
                success_rate_threshold: 0.01,
                min_observations: 5,
                max_rules_per_iteration: None,
            },
            convergence: ConvergenceBehavior::Thorough,
            fast_path: FastPathPreferences::olap(),
        }
    }

    /// Research workload: exhaustive optimization for benchmarking.
    ///
    /// Very generous limits. Runs until equality saturation or
    /// the iteration cap. All rules are considered regardless
    /// of historical success. No fast paths -- every query goes
    /// through full e-graph exploration.
    #[must_use]
    pub fn research() -> Self {
        Self {
            max_time: Some(Duration::from_secs(60)),
            max_cpu_time: Some(Duration::from_secs(60)),
            max_memory: Some(8 * 1024 * 1024 * 1024), // 8 GB
            max_egraph_nodes: Some(5_000_000),
            max_iterations: Some(500),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior {
                fact_based_filtering: false,
                adaptive_learning: false,
                success_rate_threshold: 0.0,
                min_observations: 0,
                max_rules_per_iteration: None,
            },
            convergence: ConvergenceBehavior::Complete,
            fast_path: FastPathPreferences::disabled(),
        }
    }

    /// Enhanced interactive workload: stricter than standard
    /// `interactive()` but with smarter optimization.
    ///
    /// Slightly more time than `interactive()`, with fact-based
    /// filtering and adaptive learning to make better use of
    /// the budget. Convergence is immediate -- we take the first
    /// improvement and return it.
    #[must_use]
    pub fn interactive_plus() -> Self {
        Self {
            max_time: Some(Duration::from_millis(150)),
            max_cpu_time: Some(Duration::from_millis(150)),
            max_memory: Some(75 * 1024 * 1024), // 75 MB
            max_egraph_nodes: Some(15_000),
            max_iterations: Some(12),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior {
                fact_based_filtering: true,
                adaptive_learning: true,
                success_rate_threshold: 0.1,
                min_observations: 5,
                max_rules_per_iteration: Some(30),
            },
            convergence: ConvergenceBehavior::Immediate,
            fast_path: FastPathPreferences::default(),
        }
    }

    // ── Predicates ──────────────────────────────────────────

    /// Whether practical resource fields are unconstrained.
    ///
    /// Returns true if time, memory, and e-graph size are unlimited.
    /// The iteration limit is a safety mechanism and doesn't count as a
    /// practical constraint for normal queries.
    #[must_use]
    pub fn is_unlimited(&self) -> bool {
        self.max_time.is_none()
            && self.max_cpu_time.is_none()
            && self.max_memory.is_none()
            && self.max_egraph_nodes.is_none()
    }

    // ── Builder methods (resource limits) ───────────────────

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

    // ── Builder methods (workload controls) ─────────────────

    /// Set the rule selection behavior.
    #[must_use]
    pub fn with_rule_selection(
        mut self,
        behavior: RuleSelectionBehavior,
    ) -> Self {
        self.rule_selection = behavior;
        self
    }

    /// Set the convergence behavior.
    #[must_use]
    pub fn with_convergence(
        mut self,
        behavior: ConvergenceBehavior,
    ) -> Self {
        self.convergence = behavior;
        self
    }

    /// Set the fast-path preferences.
    #[must_use]
    pub fn with_fast_path(
        mut self,
        prefs: FastPathPreferences,
    ) -> Self {
        self.fast_path = prefs;
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
                return ResourceCheckResult::Exceeded(ExceededResource::Time);
            }
        }

        if let Some(limit) = self.budget.max_cpu_time {
            if elapsed >= limit {
                return ResourceCheckResult::Exceeded(ExceededResource::CpuTime);
            }
        }

        if let Some(limit) = self.budget.max_memory {
            if self.peak_memory_estimate >= limit {
                return ResourceCheckResult::Exceeded(ExceededResource::Memory);
            }
        }

        if let Some(limit) = self.budget.max_egraph_nodes {
            if self.peak_egraph_nodes >= limit {
                return ResourceCheckResult::Exceeded(ExceededResource::EGraphNodes);
            }
        }

        if let Some(limit) = self.budget.max_iterations {
            if self.iterations_used >= limit {
                return ResourceCheckResult::Exceeded(ExceededResource::Iterations);
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

    /// The rule selection behavior from the budget.
    #[must_use]
    pub fn rule_selection(&self) -> &RuleSelectionBehavior {
        &self.budget.rule_selection
    }

    /// The convergence behavior from the budget.
    #[must_use]
    pub fn convergence(&self) -> ConvergenceBehavior {
        self.budget.convergence
    }

    /// The fast-path preferences from the budget.
    #[must_use]
    pub fn fast_path(&self) -> &FastPathPreferences {
        &self.budget.fast_path
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
mod tests {
    use super::*;

    // ---- ResourceBudget construction ----

    #[test]
    fn unlimited_budget_has_safety_iteration_limit() {
        let budget = ResourceBudget::unlimited();
        // "Unlimited" has a safety cap on iterations to prevent infinite loops
        assert!(budget.max_time.is_none());
        assert!(budget.max_cpu_time.is_none());
        assert!(budget.max_memory.is_none());
        assert!(budget.max_egraph_nodes.is_none());
        assert_eq!(budget.max_iterations, Some(1000));
    }

    #[test]
    fn default_budget_is_unlimited() {
        let budget = ResourceBudget::default();
        assert!(budget.is_unlimited());
    }

    #[test]
    fn unlimited_has_default_workload_controls() {
        let budget = ResourceBudget::unlimited();
        assert_eq!(budget.rule_selection, RuleSelectionBehavior::default());
        assert_eq!(budget.convergence, ConvergenceBehavior::Adaptive);
        assert!(budget.fast_path.any_enabled());
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
        let budget = ResourceBudget::unlimited().with_egraph_node_limit(10_000);
        assert!(!budget.is_unlimited());
        assert_eq!(budget.max_egraph_nodes, Some(10_000));
    }

    #[test]
    fn with_iteration_limit_sets_constraint() {
        let budget = ResourceBudget::unlimited().with_iteration_limit(5);
        // is_unlimited() ignores max_iterations (safety mechanism only)
        assert!(budget.is_unlimited());
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
        assert_eq!(budget.overflow_strategy, OverflowStrategy::ReturnOriginal);
    }

    #[test]
    fn default_overflow_strategy_is_return_best_so_far() {
        let budget = ResourceBudget::unlimited();
        assert_eq!(budget.overflow_strategy, OverflowStrategy::ReturnBestSoFar);
    }

    // ---- Builder methods for workload controls ----

    #[test]
    fn with_rule_selection_overrides_default() {
        let custom = RuleSelectionBehavior {
            fact_based_filtering: false,
            adaptive_learning: true,
            success_rate_threshold: 0.5,
            min_observations: 20,
            max_rules_per_iteration: Some(10),
        };
        let budget = ResourceBudget::unlimited()
            .with_rule_selection(custom.clone());
        assert_eq!(budget.rule_selection, custom);
    }

    #[test]
    fn with_convergence_overrides_default() {
        let budget = ResourceBudget::unlimited()
            .with_convergence(ConvergenceBehavior::Complete);
        assert_eq!(budget.convergence, ConvergenceBehavior::Complete);
    }

    #[test]
    fn with_fast_path_overrides_default() {
        let budget = ResourceBudget::unlimited()
            .with_fast_path(FastPathPreferences::disabled());
        assert!(!budget.fast_path.any_enabled());
    }

    #[test]
    fn builder_chains_workload_controls() {
        let budget = ResourceBudget::unlimited()
            .with_convergence(ConvergenceBehavior::Thorough)
            .with_fast_path(FastPathPreferences::disabled())
            .with_time_limit(Duration::from_secs(5));
        assert_eq!(budget.convergence, ConvergenceBehavior::Thorough);
        assert!(!budget.fast_path.any_enabled());
        assert_eq!(budget.max_time, Some(Duration::from_secs(5)));
    }

    // ---- OverflowStrategy equality ----

    #[test]
    fn overflow_strategy_variants_are_distinct() {
        assert_ne!(
            OverflowStrategy::ReturnBestSoFar,
            OverflowStrategy::ReturnOriginal
        );
        assert_ne!(OverflowStrategy::ReturnBestSoFar, OverflowStrategy::Fail);
        assert_ne!(OverflowStrategy::ReturnOriginal, OverflowStrategy::Fail);
    }

    // ---- RuleSelectionBehavior ----

    #[test]
    fn rule_selection_default_enables_fact_filtering() {
        let rs = RuleSelectionBehavior::default();
        assert!(rs.fact_based_filtering);
        assert!(!rs.adaptive_learning);
        assert!((rs.success_rate_threshold - 0.0).abs() < f64::EPSILON);
        assert_eq!(rs.min_observations, 10);
        assert!(rs.max_rules_per_iteration.is_none());
    }

    // ---- ConvergenceBehavior ----

    #[test]
    fn convergence_behavior_window_sizes_ordered() {
        assert!(
            ConvergenceBehavior::Immediate.window_size()
                < ConvergenceBehavior::Adaptive.window_size()
        );
        assert!(
            ConvergenceBehavior::Adaptive.window_size()
                < ConvergenceBehavior::Thorough.window_size()
        );
        assert!(
            ConvergenceBehavior::Thorough.window_size()
                < ConvergenceBehavior::Complete.window_size()
        );
    }

    #[test]
    fn convergence_behavior_growth_rates_ordered() {
        assert!(
            ConvergenceBehavior::Complete.min_growth_rate()
                < ConvergenceBehavior::Thorough.min_growth_rate()
        );
        assert!(
            ConvergenceBehavior::Thorough.min_growth_rate()
                < ConvergenceBehavior::Adaptive.min_growth_rate()
        );
        assert!(
            ConvergenceBehavior::Adaptive.min_growth_rate()
                < ConvergenceBehavior::Immediate.min_growth_rate()
        );
    }

    #[test]
    fn convergence_immediate_window_is_one() {
        assert_eq!(ConvergenceBehavior::Immediate.window_size(), 1);
    }

    #[test]
    fn convergence_complete_window_is_max() {
        assert_eq!(ConvergenceBehavior::Complete.window_size(), usize::MAX);
    }

    // ---- FastPathPreferences ----

    #[test]
    fn fast_path_default_enables_all() {
        let fp = FastPathPreferences::default();
        assert!(fp.enable_left_deep);
        assert!(fp.enable_index_only);
        assert!(fp.enable_simple_aggregation);
        assert!(fp.enable_mv_matching);
        assert!(fp.any_enabled());
    }

    #[test]
    fn fast_path_disabled_disables_all() {
        let fp = FastPathPreferences::disabled();
        assert!(!fp.enable_left_deep);
        assert!(!fp.enable_index_only);
        assert!(!fp.enable_simple_aggregation);
        assert!(!fp.enable_mv_matching);
        assert!(!fp.any_enabled());
    }

    // ---- Workload constructors ----

    #[test]
    fn oltp_has_tight_time_budget() {
        let budget = ResourceBudget::oltp();
        assert_eq!(budget.max_time, Some(Duration::from_millis(200)));
    }

    #[test]
    fn oltp_enables_adaptive_learning() {
        let budget = ResourceBudget::oltp();
        assert!(budget.rule_selection.adaptive_learning);
        assert!(budget.rule_selection.fact_based_filtering);
    }

    #[test]
    fn oltp_limits_rules_per_iteration() {
        let budget = ResourceBudget::oltp();
        assert_eq!(budget.rule_selection.max_rules_per_iteration, Some(50));
    }

    #[test]
    fn oltp_uses_adaptive_convergence() {
        let budget = ResourceBudget::oltp();
        assert_eq!(budget.convergence, ConvergenceBehavior::Adaptive);
    }

    #[test]
    fn oltp_enables_all_fast_paths() {
        let budget = ResourceBudget::oltp();
        assert!(budget.fast_path.any_enabled());
        assert!(budget.fast_path.enable_left_deep);
        assert!(budget.fast_path.enable_index_only);
    }

    #[test]
    fn olap_has_generous_time_budget() {
        let budget = ResourceBudget::olap();
        assert_eq!(budget.max_time, Some(Duration::from_secs(15)));
    }

    #[test]
    fn olap_uses_thorough_convergence() {
        let budget = ResourceBudget::olap();
        assert_eq!(budget.convergence, ConvergenceBehavior::Thorough);
    }

    #[test]
    fn olap_uses_conservative_fast_paths() {
        let budget = ResourceBudget::olap();
        // OLAP uses conservative thresholds (high min_confidence)
        assert!(budget.fast_path.any_enabled());
        assert!(budget.fast_path.min_confidence > 0.8);
        assert!(budget.fast_path.left_deep_max_tables <= 4);
    }

    #[test]
    fn olap_has_large_egraph_limit() {
        let budget = ResourceBudget::olap();
        assert_eq!(budget.max_egraph_nodes, Some(2_000_000));
    }

    #[test]
    fn research_uses_complete_convergence() {
        let budget = ResourceBudget::research();
        assert_eq!(budget.convergence, ConvergenceBehavior::Complete);
    }

    #[test]
    fn research_disables_all_fast_paths() {
        let budget = ResourceBudget::research();
        assert!(!budget.fast_path.any_enabled());
    }

    #[test]
    fn research_disables_fact_filtering() {
        let budget = ResourceBudget::research();
        assert!(!budget.rule_selection.fact_based_filtering);
        assert!(!budget.rule_selection.adaptive_learning);
    }

    #[test]
    fn research_has_high_iteration_limit() {
        let budget = ResourceBudget::research();
        assert_eq!(budget.max_iterations, Some(500));
    }

    #[test]
    fn interactive_plus_is_stricter_than_standard() {
        let iplus = ResourceBudget::interactive_plus();
        let standard = ResourceBudget::standard();
        assert!(iplus.max_time.unwrap() < standard.max_time.unwrap());
    }

    #[test]
    fn interactive_plus_uses_immediate_convergence() {
        let budget = ResourceBudget::interactive_plus();
        assert_eq!(budget.convergence, ConvergenceBehavior::Immediate);
    }

    #[test]
    fn interactive_plus_enables_learning() {
        let budget = ResourceBudget::interactive_plus();
        assert!(budget.rule_selection.adaptive_learning);
        assert!(
            budget.rule_selection.success_rate_threshold > 0.0
        );
    }

    // ---- Workload ordering ----

    #[test]
    fn oltp_time_less_than_olap() {
        let oltp = ResourceBudget::oltp();
        let olap = ResourceBudget::olap();
        assert!(oltp.max_time.unwrap() < olap.max_time.unwrap());
    }

    #[test]
    fn olap_time_less_than_research() {
        let olap = ResourceBudget::olap();
        let research = ResourceBudget::research();
        assert!(olap.max_time.unwrap() < research.max_time.unwrap());
    }

    #[test]
    fn oltp_iterations_less_than_olap() {
        let oltp = ResourceBudget::oltp();
        let olap = ResourceBudget::olap();
        assert!(
            oltp.max_iterations.unwrap()
                < olap.max_iterations.unwrap()
        );
    }

    #[test]
    fn olap_iterations_less_than_research() {
        let olap = ResourceBudget::olap();
        let research = ResourceBudget::research();
        assert!(
            olap.max_iterations.unwrap()
                < research.max_iterations.unwrap()
        );
    }

    // ---- Workload budgets can be customized ----

    #[test]
    fn oltp_can_override_convergence() {
        let budget = ResourceBudget::oltp()
            .with_convergence(ConvergenceBehavior::Thorough);
        assert_eq!(budget.convergence, ConvergenceBehavior::Thorough);
        // Other fields unchanged
        assert_eq!(budget.max_time, Some(Duration::from_millis(200)));
    }

    #[test]
    fn olap_can_override_fast_path() {
        let budget = ResourceBudget::olap()
            .with_fast_path(FastPathPreferences::oltp());
        // Overridden to OLTP fast paths (lower confidence)
        assert!(budget.fast_path.any_enabled());
        assert!(budget.fast_path.min_confidence < 0.6);
        // Other fields unchanged
        assert_eq!(budget.convergence, ConvergenceBehavior::Thorough);
    }

    #[test]
    fn research_can_add_time_limit() {
        let budget = ResourceBudget::research()
            .with_time_limit(Duration::from_secs(30));
        assert_eq!(budget.max_time, Some(Duration::from_secs(30)));
        // Convergence unchanged
        assert_eq!(budget.convergence, ConvergenceBehavior::Complete);
    }

    // ---- All workload budgets have all resource fields set ----

    #[test]
    fn all_workload_profiles_have_all_fields() {
        let profiles = [
            ResourceBudget::oltp(),
            ResourceBudget::olap(),
            ResourceBudget::research(),
            ResourceBudget::interactive_plus(),
        ];
        for budget in &profiles {
            assert!(budget.max_time.is_some());
            assert!(budget.max_cpu_time.is_some());
            assert!(budget.max_memory.is_some());
            assert!(budget.max_egraph_nodes.is_some());
            assert!(budget.max_iterations.is_some());
        }
    }

    // ---- ResourceTracker creation and basic tracking ----

    #[test]
    fn tracker_starts_at_zero() {
        let tracker = ResourceTracker::start(ResourceBudget::unlimited());
        assert_eq!(tracker.iterations_used(), 0);
        assert_eq!(tracker.peak_egraph_nodes(), 0);
        assert_eq!(tracker.peak_memory_estimate(), 0);
    }

    #[test]
    fn tracker_records_iterations() {
        let mut tracker = ResourceTracker::start(ResourceBudget::unlimited());
        tracker.record_iteration();
        tracker.record_iteration();
        tracker.record_iteration();
        assert_eq!(tracker.iterations_used(), 3);
    }

    #[test]
    fn tracker_records_peak_egraph_nodes() {
        let mut tracker = ResourceTracker::start(ResourceBudget::unlimited());
        tracker.record_egraph_nodes(100);
        tracker.record_egraph_nodes(500);
        tracker.record_egraph_nodes(200);
        assert_eq!(tracker.peak_egraph_nodes(), 500);
    }

    #[test]
    fn tracker_records_peak_memory() {
        let mut tracker = ResourceTracker::start(ResourceBudget::unlimited());
        tracker.record_memory_estimate(1000);
        tracker.record_memory_estimate(5000);
        tracker.record_memory_estimate(2000);
        assert_eq!(tracker.peak_memory_estimate(), 5000);
    }

    #[test]
    fn tracker_elapsed_returns_valid_duration() {
        let tracker = ResourceTracker::start(ResourceBudget::unlimited());
        // Duration is always non-negative by construction;
        // verify we get a finite duration back.
        let _elapsed = tracker.elapsed();
    }

    // ---- ResourceTracker workload accessors ----

    #[test]
    fn tracker_exposes_rule_selection() {
        let budget = ResourceBudget::oltp();
        let tracker = ResourceTracker::start(budget);
        assert!(tracker.rule_selection().adaptive_learning);
    }

    #[test]
    fn tracker_exposes_convergence() {
        let budget = ResourceBudget::olap();
        let tracker = ResourceTracker::start(budget);
        assert_eq!(tracker.convergence(), ConvergenceBehavior::Thorough);
    }

    #[test]
    fn tracker_exposes_fast_path() {
        let budget = ResourceBudget::research();
        let tracker = ResourceTracker::start(budget);
        assert!(!tracker.fast_path().any_enabled());
    }

    // ---- ResourceCheckResult ----

    #[test]
    fn unlimited_budget_always_within_budget() {
        let tracker = ResourceTracker::start(ResourceBudget::unlimited());
        assert_eq!(tracker.check(), ResourceCheckResult::WithinBudget);
        assert!(tracker.check().is_within_budget());
    }

    #[test]
    fn iteration_limit_exceeded() {
        let budget = ResourceBudget::unlimited().with_iteration_limit(2);
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
        let budget = ResourceBudget::unlimited().with_egraph_node_limit(1000);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_egraph_nodes(500);
        assert!(tracker.check().is_within_budget());
        tracker.record_egraph_nodes(1000);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::EGraphNodes)
        );
    }

    #[test]
    fn memory_limit_exceeded() {
        let budget = ResourceBudget::unlimited().with_memory_limit(1024);
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
        let exceeded = ResourceCheckResult::Exceeded(ExceededResource::Time);
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
        assert_eq!(ExceededResource::EGraphNodes.to_string(), "e-graph nodes");
    }

    #[test]
    fn exceeded_resource_display_iterations() {
        assert_eq!(ExceededResource::Iterations.to_string(), "iterations");
    }

    // ---- ResourceTracker overflow_strategy ----

    #[test]
    fn tracker_returns_budget_overflow_strategy() {
        let budget = ResourceBudget::unlimited()
            .with_overflow_strategy(OverflowStrategy::Fail);
        let tracker = ResourceTracker::start(budget);
        assert_eq!(tracker.overflow_strategy(), OverflowStrategy::Fail);
    }

    // ---- ResourceUsageReport ----

    #[test]
    fn report_within_budget() {
        let tracker = ResourceTracker::start(ResourceBudget::unlimited());
        let report = tracker.report();
        assert!(report.completed_within_budget());
        assert!(report.budget_exceeded.is_none());
        assert_eq!(report.iterations_used, 0);
        assert_eq!(report.peak_egraph_nodes, 0);
        assert_eq!(report.peak_memory_estimate, 0);
    }

    #[test]
    fn report_exceeded_budget() {
        let budget = ResourceBudget::unlimited().with_iteration_limit(1);
        let mut tracker = ResourceTracker::start(budget);
        tracker.record_iteration();
        let report = tracker.report();
        assert!(!report.completed_within_budget());
        assert_eq!(report.budget_exceeded, Some(ExceededResource::Iterations));
    }

    #[test]
    fn report_captures_all_metrics() {
        let budget = ResourceBudget::unlimited().with_iteration_limit(100);
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
        let budget = ResourceBudget::unlimited().with_iteration_limit(0);
        let tracker = ResourceTracker::start(budget);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::Iterations)
        );
    }

    #[test]
    fn zero_egraph_node_limit_immediately_exceeded() {
        let budget = ResourceBudget::unlimited().with_egraph_node_limit(0);
        let tracker = ResourceTracker::start(budget);
        assert_eq!(
            tracker.check(),
            ResourceCheckResult::Exceeded(ExceededResource::EGraphNodes)
        );
    }

    #[test]
    fn zero_memory_limit_immediately_exceeded() {
        let budget = ResourceBudget::unlimited().with_memory_limit(0);
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
            ResourceCheckResult::Exceeded(ExceededResource::EGraphNodes)
        );
    }

    #[test]
    fn peak_tracking_never_decreases() {
        let mut tracker = ResourceTracker::start(ResourceBudget::unlimited());
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
