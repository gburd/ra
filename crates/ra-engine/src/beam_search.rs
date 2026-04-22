//! Beam search for managing search space size.
//!
//! Keeps only the top-K candidate plans at each iteration, providing:
//! - Predictable memory usage (bounded by beam width)
//! - Faster optimization (fewer plans to evaluate)
//! - Prevention of exponential explosion in complex queries
//!
//! Inspired by ORCA's approach to managing large search spaces.

use egg::Id;
use std::collections::HashMap;

/// Configuration for beam search optimization.
#[derive(Debug, Clone)]
pub struct BeamSearchConfig {
    /// Maximum number of plans to keep per iteration (beam width).
    /// Higher values = better plan quality, slower optimization.
    /// Typical values: 50-200 for complex queries.
    pub beam_width: usize,

    /// Minimum number of iterations before beam search activates.
    /// Early iterations explore broadly before pruning.
    pub warmup_iterations: usize,

    /// Enable beam search (can be disabled for testing).
    pub enabled: bool,
}

impl BeamSearchConfig {
    /// Create a new beam search configuration.
    pub fn new(beam_width: usize, warmup_iterations: usize) -> Self {
        Self {
            beam_width,
            warmup_iterations,
            enabled: true,
        }
    }

    /// Default configuration for complex queries (8+ tables).
    pub fn complex() -> Self {
        Self::new(100, 3)
    }

    /// Aggressive configuration for very complex queries (12+ tables).
    pub fn aggressive() -> Self {
        Self::new(50, 2)
    }

    /// Conservative configuration (more exploration).
    pub fn conservative() -> Self {
        Self::new(200, 5)
    }

    /// Disabled configuration (no beam search).
    pub fn disabled() -> Self {
        Self {
            beam_width: usize::MAX,
            warmup_iterations: 0,
            enabled: false,
        }
    }
}

impl Default for BeamSearchConfig {
    fn default() -> Self {
        Self::complex()
    }
}

/// Tracks top-K plans during beam search.
#[derive(Debug)]
pub struct BeamSearchTracker {
    /// Configuration.
    config: BeamSearchConfig,

    /// Current iteration number.
    iteration: usize,

    /// Plan costs tracked so far.
    /// Key: equivalence class ID, Value: (cost, keep_flag)
    plan_costs: HashMap<Id, (f64, bool)>,

    /// Number of plans pruned.
    pruned_count: usize,

    /// Number of plans kept.
    kept_count: usize,
}

impl BeamSearchTracker {
    /// Create a new beam search tracker.
    pub fn new(config: BeamSearchConfig) -> Self {
        Self {
            config,
            iteration: 0,
            plan_costs: HashMap::new(),
            pruned_count: 0,
            kept_count: 0,
        }
    }

    /// Record the start of a new iteration.
    pub fn start_iteration(&mut self, iteration: usize) {
        self.iteration = iteration;
    }

    /// Record the cost of a plan.
    pub fn record_plan(&mut self, eclass: Id, cost: f64) {
        self.plan_costs.insert(eclass, (cost, false));
    }

    /// Prune plans, keeping only top-K by cost.
    ///
    /// Returns the number of plans pruned.
    pub fn prune(&mut self) -> usize {
        // Skip pruning if disabled or in warmup
        if !self.config.enabled || self.iteration < self.config.warmup_iterations {
            return 0;
        }

        if self.plan_costs.is_empty() {
            return 0;
        }

        // Sort plans by cost
        let mut plans: Vec<_> = self.plan_costs.iter().collect();
        plans.sort_by(|a, b| {
            a.1 .0
                .partial_cmp(&b.1 .0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Keep top-K plans
        let keep_count = self.config.beam_width.min(plans.len());
        let prune_count = plans.len().saturating_sub(keep_count);

        // Collect eclasses to keep
        let to_keep: Vec<Id> = plans
            .iter()
            .take(keep_count)
            .map(|(eclass, _)| **eclass)
            .collect();

        // Mark kept eclasses
        for eclass in to_keep {
            if let Some((_, keep)) = self.plan_costs.get_mut(&eclass) {
                *keep = true;
            }
        }

        self.kept_count = keep_count;
        self.pruned_count += prune_count;

        prune_count
    }

    /// Check if a plan should be kept (not pruned).
    pub fn should_keep(&self, eclass: Id) -> bool {
        // If beam search disabled, keep everything
        if !self.config.enabled {
            return true;
        }

        // If in warmup, keep everything
        if self.iteration < self.config.warmup_iterations {
            return true;
        }

        // Check if marked as kept
        self.plan_costs
            .get(&eclass)
            .map(|(_, keep)| *keep)
            .unwrap_or(true) // Keep unknown plans (conservative)
    }

    /// Get statistics about beam search pruning.
    pub fn stats(&self) -> BeamSearchStats {
        BeamSearchStats {
            beam_width: self.config.beam_width,
            warmup_iterations: self.config.warmup_iterations,
            current_iteration: self.iteration,
            total_plans: self.plan_costs.len(),
            plans_kept: self.kept_count,
            plans_pruned: self.pruned_count,
            enabled: self.config.enabled,
        }
    }

    /// Reset the tracker for a new optimization.
    pub fn reset(&mut self) {
        self.iteration = 0;
        self.plan_costs.clear();
        self.pruned_count = 0;
        self.kept_count = 0;
    }
}

/// Statistics about beam search pruning.
#[derive(Debug, Clone, Copy)]
pub struct BeamSearchStats {
    /// Configured beam width.
    pub beam_width: usize,
    /// Warmup iterations before pruning starts.
    pub warmup_iterations: usize,
    /// Current iteration number.
    pub current_iteration: usize,
    /// Total number of plans tracked.
    pub total_plans: usize,
    /// Number of plans kept.
    pub plans_kept: usize,
    /// Number of plans pruned.
    pub plans_pruned: usize,
    /// Whether beam search is enabled.
    pub enabled: bool,
}

impl BeamSearchStats {
    /// Calculate pruning rate as a percentage.
    pub fn pruning_rate(&self) -> f64 {
        if self.total_plans == 0 {
            0.0
        } else {
            (self.plans_pruned as f64 / self.total_plans as f64) * 100.0
        }
    }

    /// Check if beam search is active (past warmup).
    pub fn is_active(&self) -> bool {
        self.enabled && self.current_iteration >= self.warmup_iterations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg::Id;

    fn make_id(n: usize) -> Id {
        Id::from(n)
    }

    #[test]
    fn test_beam_search_basic() {
        let config = BeamSearchConfig::new(3, 1); // Keep 3 plans, warmup 1 iter
        let mut tracker = BeamSearchTracker::new(config);

        // Iteration 0 (warmup) - no pruning
        tracker.start_iteration(0);
        tracker.record_plan(make_id(1), 100.0);
        tracker.record_plan(make_id(2), 200.0);
        tracker.record_plan(make_id(3), 300.0);
        tracker.record_plan(make_id(4), 400.0);
        tracker.record_plan(make_id(5), 500.0);

        let pruned = tracker.prune();
        assert_eq!(pruned, 0); // Warmup - no pruning
        assert!(tracker.should_keep(make_id(1)));
        assert!(tracker.should_keep(make_id(5)));

        // Iteration 1 (active) - prune worst 2 plans
        tracker.start_iteration(1);
        let pruned = tracker.prune();
        assert_eq!(pruned, 2); // Keep 3 out of 5

        // Best 3 should be kept
        assert!(tracker.should_keep(make_id(1))); // Cost 100
        assert!(tracker.should_keep(make_id(2))); // Cost 200
        assert!(tracker.should_keep(make_id(3))); // Cost 300
    }

    #[test]
    fn test_beam_search_disabled() {
        let config = BeamSearchConfig::disabled();
        let mut tracker = BeamSearchTracker::new(config);

        tracker.start_iteration(0);
        tracker.record_plan(make_id(1), 100.0);
        tracker.record_plan(make_id(2), 200.0);
        tracker.record_plan(make_id(3), 300.0);

        let pruned = tracker.prune();
        assert_eq!(pruned, 0); // Disabled - no pruning

        // All plans should be kept
        assert!(tracker.should_keep(make_id(1)));
        assert!(tracker.should_keep(make_id(2)));
        assert!(tracker.should_keep(make_id(3)));
    }

    #[test]
    fn test_beam_search_stats() {
        let config = BeamSearchConfig::new(2, 0); // Keep 2, no warmup
        let mut tracker = BeamSearchTracker::new(config);

        tracker.start_iteration(0);
        tracker.record_plan(make_id(1), 100.0);
        tracker.record_plan(make_id(2), 200.0);
        tracker.record_plan(make_id(3), 300.0);
        tracker.record_plan(make_id(4), 400.0);

        tracker.prune();

        let stats = tracker.stats();
        assert_eq!(stats.beam_width, 2);
        assert_eq!(stats.total_plans, 4);
        assert_eq!(stats.plans_kept, 2);
        assert_eq!(stats.plans_pruned, 2);
        assert_eq!(stats.pruning_rate(), 50.0);
    }

    #[test]
    fn test_beam_configurations() {
        let complex = BeamSearchConfig::complex();
        assert_eq!(complex.beam_width, 100);
        assert_eq!(complex.warmup_iterations, 3);
        assert!(complex.enabled);

        let aggressive = BeamSearchConfig::aggressive();
        assert_eq!(aggressive.beam_width, 50);
        assert_eq!(aggressive.warmup_iterations, 2);

        let conservative = BeamSearchConfig::conservative();
        assert_eq!(conservative.beam_width, 200);
        assert_eq!(conservative.warmup_iterations, 5);

        let disabled = BeamSearchConfig::disabled();
        assert_eq!(disabled.beam_width, usize::MAX);
        assert!(!disabled.enabled);
    }

    #[test]
    fn test_warmup_period() {
        let config = BeamSearchConfig::new(2, 3); // 3 warmup iterations
        let mut tracker = BeamSearchTracker::new(config);

        // Iterations 0-2: warmup (no pruning)
        for iter in 0..3 {
            tracker.start_iteration(iter);
            tracker.record_plan(make_id(1), 100.0);
            tracker.record_plan(make_id(2), 200.0);
            tracker.record_plan(make_id(3), 300.0);

            let pruned = tracker.prune();
            assert_eq!(pruned, 0, "No pruning during warmup iteration {}", iter);
        }

        // Iteration 3: active pruning
        tracker.start_iteration(3);
        let pruned = tracker.prune();
        assert_eq!(pruned, 1); // Keep 2 out of 3
    }

    #[test]
    fn test_empty_plans() {
        let config = BeamSearchConfig::complex();
        let mut tracker = BeamSearchTracker::new(config);

        tracker.start_iteration(5); // Past warmup
        let pruned = tracker.prune();
        assert_eq!(pruned, 0); // No plans to prune
    }

    #[test]
    fn test_beam_width_larger_than_plans() {
        let config = BeamSearchConfig::new(100, 0);
        let mut tracker = BeamSearchTracker::new(config);

        tracker.start_iteration(0);
        tracker.record_plan(make_id(1), 100.0);
        tracker.record_plan(make_id(2), 200.0);

        let pruned = tracker.prune();
        assert_eq!(pruned, 0); // Beam width > plan count, no pruning

        let stats = tracker.stats();
        assert_eq!(stats.plans_kept, 2);
        assert_eq!(stats.plans_pruned, 0);
    }
}
