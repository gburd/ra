//! Bayesian adaptive search space pruning (RFC 0059).
//!
//! Uses Beta-Binomial conjugate inference to learn which structural
//! patterns in the plan search space are worth exploring. Each plan
//! fingerprint bucket maintains a Beta distribution parameterized by
//! `(alpha, beta)`. Observations are Bernoulli trials (improved or
//! not) with EWMA decay for non-stationarity.
//!
//! The pruner computes an adaptive threshold that rises as the
//! optimization budget shrinks, becoming increasingly selective.

use std::collections::HashMap;

use ra_core::algebra::RelExpr;

use crate::pattern_fingerprint::PlanFingerprint;

/// Improvement statistics for one fingerprint bucket.
///
/// Uses a Beta distribution: `Beta(alpha, beta)` where `alpha` counts
/// successes (improvements) and `beta` counts failures. The posterior
/// mean `alpha / (alpha + beta)` gives the estimated improvement
/// probability.
#[derive(Debug, Clone)]
pub struct BucketStats {
    /// Pseudo-count of successes (improvements observed).
    pub alpha: f64,
    /// Pseudo-count of failures (no improvement observed).
    pub beta: f64,
}

impl BucketStats {
    /// Uninformative prior: Beta(1, 1) = Uniform(0, 1).
    #[must_use]
    pub fn uninformative() -> Self {
        Self {
            alpha: 1.0,
            beta: 1.0,
        }
    }

    /// Create from explicit alpha/beta values.
    #[must_use]
    pub fn new(alpha: f64, beta: f64) -> Self {
        Self { alpha, beta }
    }

    /// Posterior mean: `E[p] = alpha / (alpha + beta)`.
    #[must_use]
    pub fn mean(&self) -> f64 {
        let total = self.alpha + self.beta;
        if total <= 0.0 {
            return 0.5;
        }
        self.alpha / total
    }

    /// Effective number of observations (subtracting the 2
    /// pseudo-counts from the uninformative prior).
    #[must_use]
    pub fn sample_count(&self) -> f64 {
        (self.alpha + self.beta) - 2.0
    }

    /// Record an observation with EWMA decay.
    ///
    /// `decay` in `(0, 1]` controls how fast old observations fade.
    /// 0.95 means each observation reduces prior weight by 5%.
    pub fn record(&mut self, improved: bool, decay: f64) {
        self.alpha = 1.0 + (self.alpha - 1.0) * decay;
        self.beta = 1.0 + (self.beta - 1.0) * decay;

        if improved {
            self.alpha += 1.0;
        } else {
            self.beta += 1.0;
        }
    }

    /// Posterior variance: `Var[p] = ab / ((a+b)^2 (a+b+1))`.
    #[must_use]
    pub fn variance(&self) -> f64 {
        let total = self.alpha + self.beta;
        if total <= 0.0 {
            return 0.25;
        }
        (self.alpha * self.beta) / (total * total * (total + 1.0))
    }
}

/// Result of a single exploration decision, for diagnostics.
#[derive(Debug, Clone)]
pub struct PruningOutcome {
    /// The fingerprint considered.
    pub fingerprint: PlanFingerprint,
    /// Whether the pruner chose to explore.
    pub explored: bool,
    /// If explored, whether the best plan improved.
    pub improved: Option<bool>,
    /// Posterior probability at decision time.
    pub posterior: f64,
    /// Budget fraction remaining at decision time.
    pub budget_remaining: f64,
}

/// Tuning knobs for the Bayesian pruner.
#[derive(Debug, Clone)]
pub struct PruningConfig {
    /// EWMA decay factor for observation aging.
    /// Range: `(0, 1]`. Default: 0.95.
    pub decay: f64,
    /// Base threshold below which exploration is skipped.
    /// Range: `(0, 1)`. Default: 0.15.
    pub base_threshold: f64,
    /// How aggressively the threshold rises as budget shrinks.
    /// Higher values are more aggressive. Default: 2.0.
    pub budget_sensitivity: f64,
    /// Minimum observations before trusting the posterior enough
    /// to prune. Below this we always explore. Default: 3.
    pub min_observations: u64,
    /// Maximum outcome entries retained in history. Default: 10000.
    pub max_history: usize,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            decay: 0.95,
            base_threshold: 0.15,
            budget_sensitivity: 2.0,
            min_observations: 3,
            max_history: 10_000,
        }
    }
}

/// Bayesian adaptive search space pruner.
///
/// Maintains per-fingerprint Beta distributions and uses them with
/// the current budget state to decide whether exploring a plan
/// subtree is worthwhile.
#[derive(Debug, Clone)]
pub struct BayesianPruner {
    stats: HashMap<PlanFingerprint, BucketStats>,
    config: PruningConfig,
    history: Vec<PruningOutcome>,
    explored_count: u64,
    skipped_count: u64,
}

impl BayesianPruner {
    /// Create a new pruner with the given configuration.
    #[must_use]
    pub fn new(config: PruningConfig) -> Self {
        Self {
            stats: HashMap::new(),
            config,
            history: Vec::new(),
            explored_count: 0,
            skipped_count: 0,
        }
    }

    /// Create a pruner with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(PruningConfig::default())
    }

    /// Build a fingerprint for the given plan expression.
    #[must_use]
    pub fn fingerprint(&self, plan: &RelExpr) -> PlanFingerprint {
        PlanFingerprint::from_plan(plan)
    }

    /// Decide whether to explore a subtree with the given
    /// fingerprint.
    ///
    /// `budget_remaining` is in `[0.0, 1.0]` where 1.0 means the
    /// full budget is available and 0.0 means exhausted.
    #[must_use]
    pub fn should_explore(&self, fingerprint: &PlanFingerprint, budget_remaining: f64) -> bool {
        let bucket = self
            .stats
            .get(fingerprint)
            .cloned()
            .unwrap_or_else(BucketStats::uninformative);

        if bucket.sample_count() < self.config.min_observations as f64 {
            return true;
        }

        let posterior = bucket.mean();
        let threshold = self.adaptive_threshold(budget_remaining);

        posterior >= threshold
    }

    /// Compute the adaptive pruning threshold given remaining budget.
    ///
    /// As `budget_remaining` drops from 1.0 toward 0.0, the
    /// threshold rises from `base_threshold` toward 1.0:
    ///
    /// ```text
    /// threshold = base + (1 - base) * (1 - remaining)^sensitivity
    /// ```
    #[must_use]
    pub fn adaptive_threshold(&self, budget_remaining: f64) -> f64 {
        let spent = (1.0 - budget_remaining).clamp(0.0, 1.0);
        let base = self.config.base_threshold;
        let sens = self.config.budget_sensitivity;
        base + (1.0 - base) * spent.powf(sens)
    }

    /// Record the outcome of an exploration.
    pub fn record_outcome(&mut self, fingerprint: &PlanFingerprint, improved: bool) {
        let bucket = self
            .stats
            .entry(fingerprint.clone())
            .or_insert_with(BucketStats::uninformative);
        bucket.record(improved, self.config.decay);
        self.explored_count += 1;
    }

    /// Record that a subtree was skipped (not explored).
    pub fn record_skip(&mut self, fingerprint: &PlanFingerprint, budget_remaining: f64) {
        let posterior = self.stats.get(fingerprint).map_or(0.5, BucketStats::mean);

        if self.history.len() < self.config.max_history {
            self.history.push(PruningOutcome {
                fingerprint: fingerprint.clone(),
                explored: false,
                improved: None,
                posterior,
                budget_remaining,
            });
        }

        self.skipped_count += 1;
    }

    /// Record an exploration decision and its outcome in the history.
    pub fn record_explored(
        &mut self,
        fingerprint: &PlanFingerprint,
        improved: bool,
        budget_remaining: f64,
    ) {
        let posterior = self.stats.get(fingerprint).map_or(0.5, BucketStats::mean);

        self.record_outcome(fingerprint, improved);

        if self.history.len() < self.config.max_history {
            self.history.push(PruningOutcome {
                fingerprint: fingerprint.clone(),
                explored: true,
                improved: Some(improved),
                posterior,
                budget_remaining,
            });
        }
    }

    /// Get the current stats for a fingerprint, or `None` if unseen.
    #[must_use]
    pub fn bucket_stats(&self, fingerprint: &PlanFingerprint) -> Option<&BucketStats> {
        self.stats.get(fingerprint)
    }

    /// Number of distinct fingerprint buckets observed so far.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.stats.len()
    }

    /// Total number of subtrees explored.
    #[must_use]
    pub fn explored_count(&self) -> u64 {
        self.explored_count
    }

    /// Total number of subtrees skipped.
    #[must_use]
    pub fn skipped_count(&self) -> u64 {
        self.skipped_count
    }

    /// Fraction of decisions that were "skip".
    #[must_use]
    pub fn skip_rate(&self) -> f64 {
        let total = self.explored_count + self.skipped_count;
        if total == 0 {
            return 0.0;
        }
        self.skipped_count as f64 / total as f64
    }

    /// Read-only access to the outcome history.
    #[must_use]
    pub fn history(&self) -> &[PruningOutcome] {
        &self.history
    }

    /// Get the pruning configuration.
    #[must_use]
    pub fn config(&self) -> &PruningConfig {
        &self.config
    }

    /// Compute summary statistics over all buckets.
    #[must_use]
    pub fn summary(&self) -> PruningSummary {
        let mut total_alpha = 0.0;
        let mut total_beta = 0.0;
        let mut highest_mean = 0.0_f64;
        let mut lowest_mean = 1.0_f64;

        for bucket in self.stats.values() {
            total_alpha += bucket.alpha - 1.0;
            total_beta += bucket.beta - 1.0;
            let m = bucket.mean();
            highest_mean = highest_mean.max(m);
            lowest_mean = lowest_mean.min(m);
        }

        let total_observations = total_alpha + total_beta;
        let overall_improvement_rate = if total_observations > 0.0 {
            total_alpha / total_observations
        } else {
            0.0
        };

        PruningSummary {
            bucket_count: self.stats.len(),
            total_explored: self.explored_count,
            total_skipped: self.skipped_count,
            overall_improvement_rate,
            highest_bucket_mean: highest_mean,
            lowest_bucket_mean: lowest_mean,
        }
    }

    /// Reset all learned state (buckets and history).
    pub fn reset(&mut self) {
        self.stats.clear();
        self.history.clear();
        self.explored_count = 0;
        self.skipped_count = 0;
    }
}

/// Aggregate statistics about the pruner's state and performance.
#[derive(Debug, Clone)]
pub struct PruningSummary {
    /// Number of distinct fingerprint buckets.
    pub bucket_count: usize,
    /// Total subtrees explored.
    pub total_explored: u64,
    /// Total subtrees skipped.
    pub total_skipped: u64,
    /// Overall improvement rate across all buckets.
    pub overall_improvement_rate: f64,
    /// Highest posterior mean across all buckets.
    pub highest_bucket_mean: f64,
    /// Lowest posterior mean across all buckets.
    pub lowest_bucket_mean: f64,
}
