//! Predefined resource budget profiles for common use cases.
//!
//! Each profile provides sensible defaults for a particular workload
//! type. All profiles can be further customized using the builder
//! methods on [`ResourceBudget`].

use std::time::Duration;

use crate::resource_budget::{
    ConvergenceBehavior, FastPathPreferences, OverflowStrategy,
    ResourceBudget, RuleSelectionBehavior,
};

impl ResourceBudget {
    /// Interactive query profile: fast response time.
    ///
    /// Targets <100ms wall-clock, limited e-graph and iterations
    /// to avoid runaway optimization. Returns best-so-far on overflow.
    #[must_use]
    pub fn interactive() -> Self {
        Self {
            max_time: Some(Duration::from_millis(100)),
            max_cpu_time: Some(Duration::from_millis(100)),
            max_memory: Some(50 * 1024 * 1024), // 50 MB
            max_egraph_nodes: Some(10_000),
            max_iterations: Some(10),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior::default(),
            convergence: ConvergenceBehavior::Adaptive,
            fast_path: FastPathPreferences::default(),
        }
    }

    /// Standard query profile: balanced optimization.
    ///
    /// Targets ~1s wall-clock with moderate resource limits.
    /// Suitable for OLTP queries where some optimization is
    /// beneficial but latency matters.
    #[must_use]
    pub fn standard() -> Self {
        Self {
            max_time: Some(Duration::from_secs(1)),
            max_cpu_time: Some(Duration::from_secs(1)),
            max_memory: Some(500 * 1024 * 1024), // 500 MB
            max_egraph_nodes: Some(100_000),
            max_iterations: Some(30),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior::default(),
            convergence: ConvergenceBehavior::Adaptive,
            fast_path: FastPathPreferences::default(),
        }
    }

    /// Batch/analytical profile: thorough optimization.
    ///
    /// Targets ~10s wall-clock with generous resource limits.
    /// Suitable for long-running analytical queries where extra
    /// optimization time pays off in execution time savings.
    #[must_use]
    pub fn batch() -> Self {
        Self {
            max_time: Some(Duration::from_secs(10)),
            max_cpu_time: Some(Duration::from_secs(10)),
            max_memory: Some(2 * 1024 * 1024 * 1024), // 2 GB
            max_egraph_nodes: Some(1_000_000),
            max_iterations: Some(100),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior::default(),
            convergence: ConvergenceBehavior::Adaptive,
            fast_path: FastPathPreferences::default(),
        }
    }

    /// Memory-constrained profile: strict memory limit.
    ///
    /// Designed for environments with limited memory. Keeps a very
    /// tight memory budget while allowing moderate time and iterations.
    #[must_use]
    pub fn memory_constrained() -> Self {
        Self {
            max_time: Some(Duration::from_secs(5)),
            max_cpu_time: Some(Duration::from_secs(5)),
            max_memory: Some(10 * 1024 * 1024), // 10 MB
            max_egraph_nodes: Some(5_000),
            max_iterations: Some(15),
            overflow_strategy: OverflowStrategy::ReturnBestSoFar,
            rule_selection: RuleSelectionBehavior::default(),
            convergence: ConvergenceBehavior::Adaptive,
            fast_path: FastPathPreferences::default(),
        }
    }


    /// Select an appropriate profile based on a named workload.
    ///
    /// Returns `None` if the name is unrecognized. This is useful
    /// for CLI tools and configuration files that specify profiles
    /// by name.
    #[must_use]
    pub fn from_profile_name(name: &str) -> Option<Self> {
        match name {
            "interactive" => Some(Self::interactive()),
            "interactive_plus" | "interactive-plus" => {
                Some(Self::interactive_plus())
            }
            "standard" => Some(Self::standard()),
            "batch" => Some(Self::batch()),
            "memory_constrained" | "memory-constrained" => {
                Some(Self::memory_constrained())
            }
            "oltp" => Some(Self::oltp()),
            "olap" => Some(Self::olap()),
            "research" => Some(Self::research()),
            "unlimited" => Some(Self::unlimited()),
            _ => None,
        }
    }

    /// All available profile names, sorted alphabetically.
    #[must_use]
    pub fn profile_names() -> &'static [&'static str] {
        &[
            "batch",
            "interactive",
            "interactive_plus",
            "memory_constrained",
            "olap",
            "oltp",
            "research",
            "standard",
            "unlimited",
        ]
    }
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;

    // ---- Interactive profile ----

    #[test]
    fn interactive_has_time_limit() {
        let budget = ResourceBudget::interactive();
        assert_eq!(budget.max_time, Some(Duration::from_millis(100)));
    }

    #[test]
    fn interactive_has_cpu_time_limit() {
        let budget = ResourceBudget::interactive();
        assert_eq!(budget.max_cpu_time, Some(Duration::from_millis(100)));
    }

    #[test]
    fn interactive_has_memory_limit() {
        let budget = ResourceBudget::interactive();
        assert_eq!(budget.max_memory, Some(50 * 1024 * 1024));
    }

    #[test]
    fn interactive_has_egraph_node_limit() {
        let budget = ResourceBudget::interactive();
        assert_eq!(budget.max_egraph_nodes, Some(10_000));
    }

    #[test]
    fn interactive_has_iteration_limit() {
        let budget = ResourceBudget::interactive();
        assert_eq!(budget.max_iterations, Some(10));
    }

    #[test]
    fn interactive_returns_best_so_far() {
        let budget = ResourceBudget::interactive();
        assert_eq!(budget.overflow_strategy, OverflowStrategy::ReturnBestSoFar);
    }

    #[test]
    fn interactive_is_not_unlimited() {
        assert!(!ResourceBudget::interactive().is_unlimited());
    }

    // ---- Standard profile ----

    #[test]
    fn standard_has_time_limit() {
        let budget = ResourceBudget::standard();
        assert_eq!(budget.max_time, Some(Duration::from_secs(1)));
    }

    #[test]
    fn standard_has_cpu_time_limit() {
        let budget = ResourceBudget::standard();
        assert_eq!(budget.max_cpu_time, Some(Duration::from_secs(1)));
    }

    #[test]
    fn standard_has_memory_limit() {
        let budget = ResourceBudget::standard();
        assert_eq!(budget.max_memory, Some(500 * 1024 * 1024));
    }

    #[test]
    fn standard_has_egraph_node_limit() {
        let budget = ResourceBudget::standard();
        assert_eq!(budget.max_egraph_nodes, Some(100_000));
    }

    #[test]
    fn standard_has_iteration_limit() {
        let budget = ResourceBudget::standard();
        assert_eq!(budget.max_iterations, Some(30));
    }

    #[test]
    fn standard_returns_best_so_far() {
        let budget = ResourceBudget::standard();
        assert_eq!(budget.overflow_strategy, OverflowStrategy::ReturnBestSoFar);
    }

    #[test]
    fn standard_is_not_unlimited() {
        assert!(!ResourceBudget::standard().is_unlimited());
    }

    // ---- Batch profile ----

    #[test]
    fn batch_has_time_limit() {
        let budget = ResourceBudget::batch();
        assert_eq!(budget.max_time, Some(Duration::from_secs(10)));
    }

    #[test]
    fn batch_has_cpu_time_limit() {
        let budget = ResourceBudget::batch();
        assert_eq!(budget.max_cpu_time, Some(Duration::from_secs(10)));
    }

    #[test]
    fn batch_has_memory_limit() {
        let budget = ResourceBudget::batch();
        assert_eq!(budget.max_memory, Some(2 * 1024 * 1024 * 1024));
    }

    #[test]
    fn batch_has_egraph_node_limit() {
        let budget = ResourceBudget::batch();
        assert_eq!(budget.max_egraph_nodes, Some(1_000_000));
    }

    #[test]
    fn batch_has_iteration_limit() {
        let budget = ResourceBudget::batch();
        assert_eq!(budget.max_iterations, Some(100));
    }

    #[test]
    fn batch_returns_best_so_far() {
        let budget = ResourceBudget::batch();
        assert_eq!(budget.overflow_strategy, OverflowStrategy::ReturnBestSoFar);
    }

    #[test]
    fn batch_is_not_unlimited() {
        assert!(!ResourceBudget::batch().is_unlimited());
    }

    // ---- Memory-constrained profile ----

    #[test]
    fn memory_constrained_has_time_limit() {
        let budget = ResourceBudget::memory_constrained();
        assert_eq!(budget.max_time, Some(Duration::from_secs(5)));
    }

    #[test]
    fn memory_constrained_has_strict_memory_limit() {
        let budget = ResourceBudget::memory_constrained();
        assert_eq!(budget.max_memory, Some(10 * 1024 * 1024));
    }

    #[test]
    fn memory_constrained_has_egraph_node_limit() {
        let budget = ResourceBudget::memory_constrained();
        assert_eq!(budget.max_egraph_nodes, Some(5_000));
    }

    #[test]
    fn memory_constrained_has_iteration_limit() {
        let budget = ResourceBudget::memory_constrained();
        assert_eq!(budget.max_iterations, Some(15));
    }

    #[test]
    fn memory_constrained_returns_best_so_far() {
        let budget = ResourceBudget::memory_constrained();
        assert_eq!(budget.overflow_strategy, OverflowStrategy::ReturnBestSoFar);
    }

    #[test]
    fn memory_constrained_is_not_unlimited() {
        assert!(!ResourceBudget::memory_constrained().is_unlimited());
    }

    // ---- Unlimited profile ----

    #[test]
    fn unlimited_has_no_time_limit() {
        assert!(ResourceBudget::unlimited().max_time.is_none());
    }

    #[test]
    fn unlimited_has_no_memory_limit() {
        assert!(ResourceBudget::unlimited().max_memory.is_none());
    }

    #[test]
    fn unlimited_has_no_egraph_limit() {
        assert!(ResourceBudget::unlimited().max_egraph_nodes.is_none());
    }

    #[test]
    fn unlimited_has_no_iteration_limit() {
        // "unlimited" still has a safety iteration cap to prevent infinite loops
        assert_eq!(ResourceBudget::unlimited().max_iterations, Some(1000));
    }

    #[test]
    fn unlimited_is_unlimited() {
        assert!(ResourceBudget::unlimited().is_unlimited());
    }

    // ---- Profile ordering ----

    #[test]
    fn interactive_time_less_than_standard() {
        let interactive = ResourceBudget::interactive();
        let standard = ResourceBudget::standard();
        assert!(interactive.max_time.expect("has limit") < standard.max_time.expect("has limit"));
    }

    #[test]
    fn standard_time_less_than_batch() {
        let standard = ResourceBudget::standard();
        let batch = ResourceBudget::batch();
        assert!(standard.max_time.expect("has limit") < batch.max_time.expect("has limit"));
    }

    #[test]
    fn interactive_memory_less_than_standard() {
        let interactive = ResourceBudget::interactive();
        let standard = ResourceBudget::standard();
        assert!(
            interactive.max_memory.expect("has limit") < standard.max_memory.expect("has limit")
        );
    }

    #[test]
    fn standard_memory_less_than_batch() {
        let standard = ResourceBudget::standard();
        let batch = ResourceBudget::batch();
        assert!(standard.max_memory.expect("has limit") < batch.max_memory.expect("has limit"));
    }

    #[test]
    fn memory_constrained_memory_less_than_interactive() {
        let mc = ResourceBudget::memory_constrained();
        let interactive = ResourceBudget::interactive();
        assert!(mc.max_memory.expect("has limit") < interactive.max_memory.expect("has limit"));
    }

    #[test]
    fn interactive_iterations_less_than_standard() {
        let interactive = ResourceBudget::interactive();
        let standard = ResourceBudget::standard();
        assert!(
            interactive.max_iterations.expect("has limit")
                < standard.max_iterations.expect("has limit")
        );
    }

    #[test]
    fn standard_iterations_less_than_batch() {
        let standard = ResourceBudget::standard();
        let batch = ResourceBudget::batch();
        assert!(
            standard.max_iterations.expect("has limit") < batch.max_iterations.expect("has limit")
        );
    }

    #[test]
    fn interactive_egraph_nodes_less_than_standard() {
        let interactive = ResourceBudget::interactive();
        let standard = ResourceBudget::standard();
        assert!(
            interactive.max_egraph_nodes.expect("has limit")
                < standard.max_egraph_nodes.expect("has limit")
        );
    }

    #[test]
    fn standard_egraph_nodes_less_than_batch() {
        let standard = ResourceBudget::standard();
        let batch = ResourceBudget::batch();
        assert!(
            standard.max_egraph_nodes.expect("has limit")
                < batch.max_egraph_nodes.expect("has limit")
        );
    }

    // ---- Profile customization ----

    #[test]
    fn interactive_can_override_time() {
        let budget = ResourceBudget::interactive().with_time_limit(Duration::from_millis(200));
        assert_eq!(budget.max_time, Some(Duration::from_millis(200)));
        // Other fields unchanged
        assert_eq!(budget.max_memory, Some(50 * 1024 * 1024));
    }

    #[test]
    fn standard_can_override_strategy() {
        let budget = ResourceBudget::standard().with_overflow_strategy(OverflowStrategy::Fail);
        assert_eq!(budget.overflow_strategy, OverflowStrategy::Fail);
        // Other fields unchanged
        assert_eq!(budget.max_time, Some(Duration::from_secs(1)));
    }

    #[test]
    fn batch_can_override_memory() {
        let budget = ResourceBudget::batch().with_memory_limit(4 * 1024 * 1024 * 1024);
        assert_eq!(budget.max_memory, Some(4 * 1024 * 1024 * 1024));
    }

    #[test]
    fn memory_constrained_can_override_egraph_limit() {
        let budget = ResourceBudget::memory_constrained().with_egraph_node_limit(2_000);
        assert_eq!(budget.max_egraph_nodes, Some(2_000));
    }

    // ---- Profile selection ----

    #[test]
    fn from_profile_name_returns_known_profiles() {
        for name in ResourceBudget::profile_names() {
            assert!(
                ResourceBudget::from_profile_name(name).is_some(),
                "profile '{name}' should be recognized"
            );
        }
    }

    #[test]
    fn from_profile_name_returns_none_for_unknown() {
        assert!(ResourceBudget::from_profile_name("nonexistent").is_none());
        assert!(ResourceBudget::from_profile_name("").is_none());
    }

    #[test]
    fn from_profile_name_accepts_hyphenated_variants() {
        assert!(ResourceBudget::from_profile_name("interactive-plus").is_some());
        assert!(ResourceBudget::from_profile_name("memory-constrained").is_some());
    }

    #[test]
    fn profile_names_are_sorted() {
        let names = ResourceBudget::profile_names();
        for window in names.windows(2) {
            assert!(
                window[0] < window[1],
                "profile names should be sorted: '{}' >= '{}'",
                window[0],
                window[1]
            );
        }
    }

    // ---- All profiles have all fields set ----

    #[test]
    fn all_constrained_profiles_have_all_fields() {
        let profiles = [
            ResourceBudget::interactive(),
            ResourceBudget::standard(),
            ResourceBudget::batch(),
            ResourceBudget::memory_constrained(),
        ];
        for budget in &profiles {
            assert!(budget.max_time.is_some(), "profile should have time limit");
            assert!(
                budget.max_cpu_time.is_some(),
                "profile should have CPU time limit"
            );
            assert!(
                budget.max_memory.is_some(),
                "profile should have memory limit"
            );
            assert!(
                budget.max_egraph_nodes.is_some(),
                "profile should have e-graph node limit"
            );
            assert!(
                budget.max_iterations.is_some(),
                "profile should have iteration limit"
            );
        }
    }
}
