//! Neural-guided saturation control.
//!
//! Provides convergence detection and adaptive rule stalling that the
//! optimizer's saturation loop uses to decide:
//! 1. When to stop iterating (neural convergence)
//! 2. Which rule groups to disable per-iteration (stalling)
//!
//! # Performance Budget
//!
//! - Convergence check: ~100ns (compare cached scores)
//! - Rule stalling update: ~50ns (counter increment + threshold check)
//! - Per-iteration scoring: ~4μs for K=50 (deferred to full implementation)

/// Tracks per-rule-group improvement over iterations for adaptive stalling.
///
/// Rule groups that don't improve the best neural cost for `stall_threshold`
/// consecutive iterations are demoted. Demoted groups fire every Nth iteration
/// instead of every iteration.
pub struct RuleStallingTracker {
    /// Consecutive iterations without improvement, per rule group.
    stall_counts: Vec<u32>,
    /// Number of rule groups tracked.
    num_groups: usize,
    /// Iterations without improvement before demotion (default: 2).
    stall_threshold: u32,
    /// Demoted rules fire every Nth iteration (default: 3).
    demoted_cadence: u32,
    /// Current iteration number.
    current_iteration: u32,
}

impl RuleStallingTracker {
    /// Create a tracker for `num_groups` rule groups.
    #[must_use]
    pub fn new(num_groups: usize) -> Self {
        Self {
            stall_counts: vec![0; num_groups],
            num_groups,
            stall_threshold: 2,
            demoted_cadence: 3,
            current_iteration: 0,
        }
    }

    /// Create with custom thresholds.
    #[must_use]
    pub fn with_config(
        num_groups: usize,
        stall_threshold: u32,
        demoted_cadence: u32,
    ) -> Self {
        Self {
            stall_counts: vec![0; num_groups],
            num_groups,
            stall_threshold,
            demoted_cadence,
            current_iteration: 0,
        }
    }

    /// Record whether each rule group improved the best cost this iteration.
    ///
    /// `improved[i]` = true if group i contributed to a cost improvement.
    pub fn record_iteration(&mut self, improved: &[bool]) {
        self.current_iteration += 1;

        for (i, &did_improve) in improved.iter().enumerate().take(self.num_groups) {
            if did_improve {
                self.stall_counts[i] = 0;
            } else {
                self.stall_counts[i] = self.stall_counts[i].saturating_add(1);
            }
        }
    }

    /// Determine which rule groups should be active for the current iteration.
    ///
    /// Returns a boolean mask: `active[i]` = true means group i should fire.
    /// Stalled groups still fire periodically (every `demoted_cadence` iterations)
    /// to avoid permanently disabling useful rules.
    #[must_use]
    pub fn active_groups(&self) -> Vec<bool> {
        (0..self.num_groups)
            .map(|i| {
                if self.stall_counts[i] < self.stall_threshold {
                    // Not stalled: always active
                    true
                } else {
                    // Stalled: fire every Nth iteration
                    self.current_iteration.is_multiple_of(self.demoted_cadence)
                }
            })
            .collect()
    }

    /// Get indices of currently active rule groups.
    #[must_use]
    pub fn active_indices(&self) -> Vec<usize> {
        self.active_groups()
            .iter()
            .enumerate()
            .filter(|(_, &active)| active)
            .map(|(i, _)| i)
            .collect()
    }

    /// Number of currently stalled (demoted) groups.
    #[must_use]
    pub fn stalled_count(&self) -> usize {
        self.stall_counts
            .iter()
            .filter(|&&c| c >= self.stall_threshold)
            .count()
    }

    /// Reset all stall counts (e.g., after a significant cost improvement).
    pub fn reset(&mut self) {
        self.stall_counts.fill(0);
    }

    /// Current iteration number.
    #[must_use]
    pub fn iteration(&self) -> u32 {
        self.current_iteration
    }
}

/// Neural convergence detector for the saturation loop.
///
/// Tracks the neural model's predicted best cost across iterations and
/// declares convergence when improvement drops below a threshold.
pub struct NeuralConvergenceDetector {
    /// Best neural cost seen so far.
    best_cost: f64,
    /// Previous iteration's best cost (for improvement calculation).
    prev_cost: f64,
    /// Minimum relative improvement to continue (default: 0.02 = 2%).
    epsilon: f64,
    /// Consecutive iterations below epsilon.
    below_epsilon_count: u32,
    /// Require this many consecutive sub-epsilon iterations before declaring
    /// convergence (default: 2, avoids premature termination on plateaus).
    patience: u32,
    /// Whether convergence has been declared.
    converged: bool,
}

impl NeuralConvergenceDetector {
    /// Create a new detector with default parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            best_cost: f64::MAX,
            prev_cost: f64::MAX,
            epsilon: 0.02,
            patience: 2,
            below_epsilon_count: 0,
            converged: false,
        }
    }

    /// Create with custom convergence threshold.
    #[must_use]
    pub fn with_epsilon(epsilon: f64) -> Self {
        Self {
            epsilon: epsilon.max(0.001),
            ..Self::new()
        }
    }

    /// Create with custom epsilon and patience.
    #[must_use]
    pub fn with_config(epsilon: f64, patience: u32) -> Self {
        Self {
            epsilon: epsilon.max(0.001),
            patience: patience.max(1),
            ..Self::new()
        }
    }

    /// Record the best neural cost for this iteration.
    ///
    /// Returns `true` if convergence is declared (safe to stop saturation).
    pub fn record(&mut self, best_cost: f64) -> bool {
        if self.converged {
            return true;
        }

        self.prev_cost = self.best_cost;
        if best_cost < self.best_cost {
            self.best_cost = best_cost;
        }

        // Can't compute improvement on first observation
        if (self.prev_cost - f64::MAX).abs() < f64::EPSILON {
            return false;
        }

        let improvement = if self.prev_cost > 0.0 {
            (self.prev_cost - self.best_cost) / self.prev_cost
        } else {
            0.0
        };

        if improvement < self.epsilon {
            self.below_epsilon_count += 1;
        } else {
            self.below_epsilon_count = 0;
        }

        if self.below_epsilon_count >= self.patience {
            self.converged = true;
        }

        self.converged
    }

    /// Whether convergence has been declared.
    #[must_use]
    pub fn is_converged(&self) -> bool {
        self.converged
    }

    /// Best cost seen so far.
    #[must_use]
    pub fn best_cost(&self) -> f64 {
        self.best_cost
    }

    /// Reset the detector for a new query.
    pub fn reset(&mut self) {
        self.best_cost = f64::MAX;
        self.prev_cost = f64::MAX;
        self.below_epsilon_count = 0;
        self.converged = false;
    }
}

impl Default for NeuralConvergenceDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_stalling_new_groups_are_active() {
        let tracker = RuleStallingTracker::new(5);
        let active = tracker.active_groups();
        assert_eq!(active, vec![true; 5]);
    }

    #[test]
    fn rule_stalling_demotes_after_threshold() {
        let mut tracker = RuleStallingTracker::new(3);

        // Group 0 never improves, groups 1-2 do
        tracker.record_iteration(&[false, true, true]);
        tracker.record_iteration(&[false, true, true]);

        // Group 0 should be stalled (2 iterations without improvement)
        assert_eq!(tracker.stalled_count(), 1);

        // On non-cadence iterations, group 0 should be inactive
        let active = tracker.active_groups();
        // iteration is 2, cadence is 3, so 2 % 3 != 0 → stalled group inactive
        assert!(!active[0]);
        assert!(active[1]);
        assert!(active[2]);
    }

    #[test]
    fn rule_stalling_demoted_fires_on_cadence() {
        let mut tracker = RuleStallingTracker::with_config(2, 1, 3);

        // Stall group 0 immediately
        tracker.record_iteration(&[false, true]);
        assert_eq!(tracker.stalled_count(), 1);

        // Advance to iteration 3 (cadence hit)
        tracker.record_iteration(&[false, true]);
        tracker.record_iteration(&[false, true]); // iteration = 3

        let active = tracker.active_groups();
        // iteration 3 % 3 == 0 → stalled group fires
        assert!(active[0]);
    }

    #[test]
    fn rule_stalling_improvement_resets_count() {
        let mut tracker = RuleStallingTracker::new(2);

        tracker.record_iteration(&[false, true]);
        tracker.record_iteration(&[false, true]);
        assert_eq!(tracker.stalled_count(), 1);

        // Group 0 improves → reset its stall count
        tracker.record_iteration(&[true, true]);
        assert_eq!(tracker.stalled_count(), 0);
    }

    #[test]
    fn convergence_not_declared_on_first_record() {
        let mut detector = NeuralConvergenceDetector::new();
        assert!(!detector.record(100.0));
    }

    #[test]
    fn convergence_declared_on_plateau() {
        let mut detector = NeuralConvergenceDetector::with_config(0.02, 2);

        detector.record(100.0);
        // No improvement
        detector.record(100.0);
        assert!(!detector.is_converged()); // patience = 2, only 1 below epsilon
        detector.record(100.0);
        assert!(detector.is_converged()); // 2 consecutive below epsilon
    }

    #[test]
    fn convergence_not_declared_with_improvement() {
        let mut detector = NeuralConvergenceDetector::with_config(0.02, 2);

        detector.record(100.0);
        detector.record(95.0); // 5% improvement > 2% epsilon
        assert!(!detector.is_converged());
        detector.record(90.0); // Still improving
        assert!(!detector.is_converged());
    }

    #[test]
    fn convergence_resets() {
        let mut detector = NeuralConvergenceDetector::new();
        detector.record(100.0);
        detector.record(100.0);
        detector.record(100.0);
        assert!(detector.is_converged());

        detector.reset();
        assert!(!detector.is_converged());
        assert_eq!(detector.best_cost(), f64::MAX);
    }

    #[test]
    fn convergence_small_improvements_below_epsilon() {
        let mut detector = NeuralConvergenceDetector::with_config(0.02, 2);

        detector.record(100.0);
        detector.record(99.5); // 0.5% < 2% epsilon
        detector.record(99.3); // 0.2% < 2% epsilon
        assert!(detector.is_converged());
    }
}
