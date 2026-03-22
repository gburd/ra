//! Early convergence detection for e-graph equality saturation.
//!
//! Monitors e-graph growth across iterations and terminates early when
//! saturation is detected, avoiding wasted computation on iterations that
//! produce no new equivalences.

use std::collections::VecDeque;

/// Tracks e-graph metrics across iterations to detect convergence.
#[derive(Debug, Clone)]
pub struct ConvergenceDetector {
    /// Number of iterations to consider for convergence check.
    window_size: usize,
    /// Minimum growth rate to consider progress (e.g., 0.05 = 5%).
    min_growth_rate: f64,
    /// Recent union counts (new equivalences found).
    recent_unions: VecDeque<usize>,
    /// Recent node counts.
    recent_nodes: VecDeque<usize>,
    /// Recent equivalence class counts.
    recent_classes: VecDeque<usize>,
}

/// Reason for termination decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationDecision {
    /// Continue optimization.
    Continue,
    /// Stop due to convergence (no progress).
    Converged,
}

/// Metrics from a single e-graph iteration.
#[derive(Debug, Clone, Copy)]
pub struct IterationMetrics {
    /// Iteration number.
    pub iteration: usize,
    /// Number of new equivalences (unions) found.
    pub unions: usize,
    /// Total number of nodes in e-graph.
    pub total_nodes: usize,
    /// Total number of equivalence classes.
    pub total_classes: usize,
}

impl ConvergenceDetector {
    /// Create a new convergence detector.
    ///
    /// # Arguments
    /// * `window_size` - Number of consecutive iterations to check (typically 2-3)
    /// * `min_growth_rate` - Minimum growth rate to consider progress (e.g., 0.05 = 5%)
    pub fn new(window_size: usize, min_growth_rate: f64) -> Self {
        Self {
            window_size,
            min_growth_rate,
            recent_unions: VecDeque::with_capacity(window_size),
            recent_nodes: VecDeque::with_capacity(window_size),
            recent_classes: VecDeque::with_capacity(window_size),
        }
    }

    /// Create detector with default settings.
    ///
    /// Defaults:
    /// - Window size: 3 iterations
    /// - Min growth rate: 5%
    pub fn default_settings() -> Self {
        Self::new(3, 0.05)
    }

    /// Record metrics from an iteration.
    pub fn record(&mut self, metrics: IterationMetrics) {
        // Add new metrics
        self.recent_unions.push_back(metrics.unions);
        self.recent_nodes.push_back(metrics.total_nodes);
        self.recent_classes.push_back(metrics.total_classes);

        // Keep only last window_size entries
        while self.recent_unions.len() > self.window_size {
            self.recent_unions.pop_front();
        }
        while self.recent_nodes.len() > self.window_size {
            self.recent_nodes.pop_front();
        }
        while self.recent_classes.len() > self.window_size {
            self.recent_classes.pop_front();
        }
    }

    /// Check if optimization should terminate early.
    ///
    /// Returns `Converged` if:
    /// 1. No new equivalences for `window_size` consecutive iterations, OR
    /// 2. Node growth rate < `min_growth_rate` for `window_size` consecutive iterations
    pub fn should_terminate(&self) -> TerminationDecision {
        // Need enough data for decision
        if self.recent_unions.len() < self.window_size {
            return TerminationDecision::Continue;
        }

        // Check if all recent iterations found 0 new equivalences
        let all_zero_unions = self.recent_unions.iter().all(|&u| u == 0);
        if all_zero_unions {
            return TerminationDecision::Converged;
        }

        // Check if node growth rate is below threshold
        if let Some(growth_stalled) = self.check_growth_rate() {
            if growth_stalled {
                return TerminationDecision::Converged;
            }
        }

        TerminationDecision::Continue
    }

    /// Check if e-graph growth rate is below threshold.
    ///
    /// Returns `Some(true)` if growth stalled, `None` if not enough data.
    fn check_growth_rate(&self) -> Option<bool> {
        if self.recent_nodes.len() < 2 {
            return None;
        }

        // Calculate growth rates for last window_size-1 intervals
        let mut growth_rates = Vec::new();
        for i in 1..self.recent_nodes.len() {
            let prev_nodes = self.recent_nodes[i - 1];
            let curr_nodes = self.recent_nodes[i];

            if prev_nodes == 0 {
                continue;
            }

            let growth_rate = (curr_nodes as f64 - prev_nodes as f64) / prev_nodes as f64;
            growth_rates.push(growth_rate);
        }

        // Check if all growth rates are below threshold
        if growth_rates.is_empty() {
            return None;
        }

        let all_below_threshold = growth_rates
            .iter()
            .all(|&rate| rate < self.min_growth_rate);

        Some(all_below_threshold)
    }

    /// Get current window statistics for debugging.
    pub fn stats(&self) -> ConvergenceStats {
        let avg_unions = if self.recent_unions.is_empty() {
            0.0
        } else {
            self.recent_unions.iter().sum::<usize>() as f64 / self.recent_unions.len() as f64
        };

        let avg_growth_rate = if self.recent_nodes.len() < 2 {
            0.0
        } else {
            let mut total_growth = 0.0;
            let mut count = 0;
            for i in 1..self.recent_nodes.len() {
                let prev = self.recent_nodes[i - 1];
                if prev > 0 {
                    let curr = self.recent_nodes[i];
                    total_growth += (curr as f64 - prev as f64) / prev as f64;
                    count += 1;
                }
            }
            if count > 0 {
                total_growth / count as f64
            } else {
                0.0
            }
        };

        ConvergenceStats {
            window_size: self.window_size,
            samples_collected: self.recent_unions.len(),
            avg_unions_per_iteration: avg_unions,
            avg_growth_rate,
        }
    }
}

/// Statistics about convergence detection.
#[derive(Debug, Clone)]
pub struct ConvergenceStats {
    /// Window size being used.
    pub window_size: usize,
    /// Number of samples collected so far.
    pub samples_collected: usize,
    /// Average unions per iteration in window.
    pub avg_unions_per_iteration: f64,
    /// Average growth rate in window.
    pub avg_growth_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convergence_on_zero_unions() {
        let mut detector = ConvergenceDetector::new(3, 0.05);

        // First iteration: progress
        detector.record(IterationMetrics {
            iteration: 0,
            unions: 10,
            total_nodes: 100,
            total_classes: 50,
        });
        assert_eq!(detector.should_terminate(), TerminationDecision::Continue);

        // Next 3 iterations: no progress
        for i in 1..=3 {
            detector.record(IterationMetrics {
                iteration: i,
                unions: 0,
                total_nodes: 100,
                total_classes: 50,
            });
        }

        // Should detect convergence after 3 iterations of 0 unions
        assert_eq!(detector.should_terminate(), TerminationDecision::Converged);
    }

    #[test]
    fn test_convergence_on_low_growth() {
        let mut detector = ConvergenceDetector::new(3, 0.05);

        // Iterations with diminishing growth (< 5%)
        detector.record(IterationMetrics {
            iteration: 0,
            unions: 5,
            total_nodes: 1000,
            total_classes: 100,
        });
        detector.record(IterationMetrics {
            iteration: 1,
            unions: 3,
            total_nodes: 1010, // 1% growth
            total_classes: 102,
        });
        detector.record(IterationMetrics {
            iteration: 2,
            unions: 2,
            total_nodes: 1020, // ~1% growth
            total_classes: 104,
        });
        detector.record(IterationMetrics {
            iteration: 3,
            unions: 1,
            total_nodes: 1025, // ~0.5% growth
            total_classes: 105,
        });

        // Should detect convergence (growth < 5% for 3 iterations)
        assert_eq!(detector.should_terminate(), TerminationDecision::Converged);
    }

    #[test]
    fn test_no_convergence_with_progress() {
        let mut detector = ConvergenceDetector::new(3, 0.05);

        // Iterations with steady progress
        for i in 0..5 {
            detector.record(IterationMetrics {
                iteration: i,
                unions: 10,
                total_nodes: 100 + i * 20, // 20% growth each iteration
                total_classes: 50 + i * 5,
            });
        }

        // Should not converge
        assert_eq!(detector.should_terminate(), TerminationDecision::Continue);
    }

    #[test]
    fn test_stats() {
        let mut detector = ConvergenceDetector::new(3, 0.05);

        detector.record(IterationMetrics {
            iteration: 0,
            unions: 10,
            total_nodes: 100,
            total_classes: 50,
        });
        detector.record(IterationMetrics {
            iteration: 1,
            unions: 5,
            total_nodes: 150,
            total_classes: 60,
        });

        let stats = detector.stats();
        assert_eq!(stats.window_size, 3);
        assert_eq!(stats.samples_collected, 2);
        assert!((stats.avg_unions_per_iteration - 7.5).abs() < 0.01);
    }
}
