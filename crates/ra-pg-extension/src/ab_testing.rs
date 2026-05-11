//! A/B testing infrastructure for neural model versions.
//!
//! Compares the current model against the previous model by splitting traffic
//! deterministically: queries are assigned to control (current model) or
//! experiment (previous model) based on `query_fingerprint % bucket_divisor`.
//!
//! Once sufficient samples accumulate in both arms, statistical analysis
//! (two-sample t-test + Cohen's d) determines whether to promote or rollback.
//!
//! # Integration
//!
//! The planner hook checks [`should_use_experiment`] before cost estimation.
//! After execution, [`record_result`] captures the prediction/actual ratio.
//! The SQL function `ra.ab_test_status()` exposes current state.

use std::sync::Mutex;

use once_cell::sync::Lazy;
use pgrx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};
use pgrx::prelude::*;

/// GUC: enable A/B testing between model versions.
pub static AB_TESTING_ENABLED: GucSetting<bool> = GucSetting::<bool>::new(true);

/// GUC: fraction of queries routed to the experiment arm (0.0 to 1.0).
pub static AB_EXPERIMENT_FRACTION: GucSetting<f64> = GucSetting::<f64>::new(0.1);

/// Register A/B testing GUC variables.
pub fn register_gucs() {
    GucRegistry::define_bool_guc(
        c"ra.ab_testing_enabled",
        c"Enable A/B testing between neural model versions.",
        c"When enabled, a fraction of queries use the previous model \
          for comparison.",
        &AB_TESTING_ENABLED,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_float_guc(
        c"ra.ab_experiment_fraction",
        c"Fraction of queries routed to experiment arm.",
        c"Values from 0.0 to 1.0. Default 0.1 means 10% of queries.",
        &AB_EXPERIMENT_FRACTION,
        0.0,
        1.0,
        GucContext::Userset,
        GucFlags::default(),
    );
}

/// Configuration for the A/B test.
#[derive(Debug, Clone)]
pub struct ABTestConfig {
    /// Fraction of queries assigned to experiment (default 0.1).
    pub experiment_fraction: f64,
    /// Minimum samples per arm before computing significance.
    pub min_samples_per_arm: usize,
    /// Cohen's d threshold for meaningful improvement (default 0.3).
    pub auto_promote_threshold: f64,
    /// Significance level (default 0.05).
    pub alpha: f64,
}

impl Default for ABTestConfig {
    fn default() -> Self {
        Self {
            experiment_fraction: 0.1,
            min_samples_per_arm: 50,
            auto_promote_threshold: 0.3,
            alpha: 0.05,
        }
    }
}

/// Which arm a query is assigned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// Current (new) model.
    Control,
    /// Previous model.
    Experiment,
}

/// Recommendation from statistical analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recommendation {
    /// Not enough data yet.
    InsufficientData,
    /// No significant difference between models.
    NoSignificantDifference,
    /// New model is significantly better: promote.
    Promote,
    /// New model is significantly worse: rollback.
    Rollback,
}

/// Results of the statistical comparison.
#[derive(Debug, Clone)]
pub struct ABTestAnalysis {
    pub control_count: usize,
    pub experiment_count: usize,
    pub control_mean_ratio: f64,
    pub experiment_mean_ratio: f64,
    pub p_value: f64,
    pub cohens_d: f64,
    pub recommendation: Recommendation,
}

/// Global A/B testing state.
static AB_STATE: Lazy<Mutex<ABTestState>> = Lazy::new(|| Mutex::new(ABTestState::new()));

struct ABTestState {
    config: ABTestConfig,
    /// Prediction/actual ratios for the control arm (current model).
    /// Ratio < 1.0 means model underestimated, > 1.0 means overestimated.
    /// Closer to 1.0 is better.
    control_results: Vec<f64>,
    /// Prediction/actual ratios for the experiment arm (previous model).
    experiment_results: Vec<f64>,
    /// Maximum samples to retain per arm (ring buffer).
    max_samples: usize,
}

impl ABTestState {
    fn new() -> Self {
        Self {
            config: ABTestConfig::default(),
            control_results: Vec::with_capacity(1024),
            experiment_results: Vec::with_capacity(1024),
            max_samples: 1024,
        }
    }
}

/// Determine whether a query should use the experiment model.
///
/// Assignment is deterministic based on `query_fingerprint` to ensure
/// the same query always goes to the same arm (for reproducibility).
///
/// Returns `true` if the query should use the previous (experiment) model.
pub fn should_use_experiment(query_fingerprint: u64) -> bool {
    if !AB_TESTING_ENABLED.get() {
        return false;
    }
    let fraction = AB_EXPERIMENT_FRACTION.get();
    if fraction <= 0.0 {
        return false;
    }
    // Deterministic assignment: use the lower bits of the fingerprint.
    let bucket = (query_fingerprint % 1000) as f64 / 1000.0;
    bucket < fraction
}

/// Determine which arm a query is assigned to.
pub fn assign_arm(query_fingerprint: u64) -> Arm {
    if should_use_experiment(query_fingerprint) {
        Arm::Experiment
    } else {
        Arm::Control
    }
}

/// Record a prediction/actual ratio for the appropriate arm.
///
/// The ratio is `predicted_cost / actual_time_ms`. A perfect model
/// yields ratio = 1.0. The absolute deviation from 1.0 is what we
/// compare between arms.
pub fn record_result(arm: Arm, predicted: f64, actual: f64) {
    if actual <= 0.0 {
        return;
    }
    let ratio = (predicted / actual - 1.0).abs();

    if let Ok(mut state) = AB_STATE.lock() {
        let max = state.max_samples;
        let results = match arm {
            Arm::Control => &mut state.control_results,
            Arm::Experiment => &mut state.experiment_results,
        };
        if results.len() >= max {
            results.remove(0);
        }
        results.push(ratio);
    }
}

/// Run statistical analysis on current A/B test state.
pub fn analyze() -> ABTestAnalysis {
    let Ok(state) = AB_STATE.lock() else {
        return ABTestAnalysis {
            control_count: 0,
            experiment_count: 0,
            control_mean_ratio: 0.0,
            experiment_mean_ratio: 0.0,
            p_value: 1.0,
            cohens_d: 0.0,
            recommendation: Recommendation::InsufficientData,
        };
    };

    let n_control = state.control_results.len();
    let n_experiment = state.experiment_results.len();

    if n_control < state.config.min_samples_per_arm
        || n_experiment < state.config.min_samples_per_arm
    {
        return ABTestAnalysis {
            control_count: n_control,
            experiment_count: n_experiment,
            control_mean_ratio: mean(&state.control_results),
            experiment_mean_ratio: mean(&state.experiment_results),
            p_value: 1.0,
            cohens_d: 0.0,
            recommendation: Recommendation::InsufficientData,
        };
    }

    let control_mean = mean(&state.control_results);
    let experiment_mean = mean(&state.experiment_results);
    let p_value = two_sample_t_test(&state.control_results, &state.experiment_results);
    let d = cohens_d(&state.control_results, &state.experiment_results);

    // Lower mean ratio = better predictions (closer to actual).
    // If control (new model) has lower mean ratio, it's better.
    let recommendation = if p_value >= state.config.alpha {
        Recommendation::NoSignificantDifference
    } else if d.abs() < state.config.auto_promote_threshold {
        Recommendation::NoSignificantDifference
    } else if control_mean < experiment_mean {
        // New model (control) is better.
        Recommendation::Promote
    } else {
        // Old model (experiment) is better — rollback.
        Recommendation::Rollback
    };

    ABTestAnalysis {
        control_count: n_control,
        experiment_count: n_experiment,
        control_mean_ratio: control_mean,
        experiment_mean_ratio: experiment_mean,
        p_value,
        cohens_d: d,
        recommendation,
    }
}

/// Reset the A/B test state (e.g., after a model promotion).
pub fn reset() {
    if let Ok(mut state) = AB_STATE.lock() {
        state.control_results.clear();
        state.experiment_results.clear();
    }
}

// ---------------------------------------------------------------------------
// Statistical utilities (self-contained, no external deps)
// ---------------------------------------------------------------------------

fn mean(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    data.iter().sum::<f64>() / data.len() as f64
}

fn variance(data: &[f64]) -> f64 {
    let n = data.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(data);
    data.iter().map(|&x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64
}

/// Welch's two-sample t-test (unequal variances). Returns two-tailed p-value.
fn two_sample_t_test(a: &[f64], b: &[f64]) -> f64 {
    let n_a = a.len() as f64;
    let n_b = b.len() as f64;
    let mean_a = mean(a);
    let mean_b = mean(b);
    let var_a = variance(a);
    let var_b = variance(b);

    let se_sq = var_a / n_a + var_b / n_b;
    if se_sq <= 0.0 {
        return if (mean_a - mean_b).abs() < 1e-12 {
            1.0
        } else {
            0.0
        };
    }

    let t = (mean_a - mean_b) / se_sq.sqrt();

    // Welch-Satterthwaite degrees of freedom.
    let num = se_sq.powi(2);
    let denom = (var_a / n_a).powi(2) / (n_a - 1.0) + (var_b / n_b).powi(2) / (n_b - 1.0);
    let df = if denom > 0.0 {
        (num / denom).floor() as usize
    } else {
        1
    };

    two_tailed_t_pvalue(t, df)
}

/// Cohen's d with pooled standard deviation.
fn cohens_d(a: &[f64], b: &[f64]) -> f64 {
    let n_a = a.len() as f64;
    let n_b = b.len() as f64;
    let var_a = variance(a);
    let var_b = variance(b);
    let pooled_var = ((n_a - 1.0) * var_a + (n_b - 1.0) * var_b) / (n_a + n_b - 2.0);
    let pooled_std = pooled_var.sqrt();
    if pooled_std <= 0.0 {
        return 0.0;
    }
    (mean(a) - mean(b)) / pooled_std
}

/// Two-tailed p-value from a t-statistic and degrees of freedom.
///
/// Uses the identity: p = I_{df/(df+t^2)}(df/2, 1/2)
fn two_tailed_t_pvalue(t: f64, df: usize) -> f64 {
    let df_f = df as f64;
    let t2 = t * t;
    let x = df_f / (df_f + t2);
    regularized_incomplete_beta(x, df_f / 2.0, 0.5)
}

/// Regularized incomplete beta function I_x(a, b) via continued fraction.
fn regularized_incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Use the symmetry relation when x > (a+1)/(a+b+2) for convergence.
    if x > (a + 1.0) / (a + b + 2.0) {
        return 1.0 - regularized_incomplete_beta(1.0 - x, b, a);
    }
    let ln_prefix = a * x.ln() + b * (1.0 - x).ln() - (a.ln() + ln_beta(a, b));
    let prefix = ln_prefix.exp();
    prefix * beta_cf(x, a, b) / a
}

/// Continued fraction evaluation for the incomplete beta function.
fn beta_cf(x: f64, a: f64, b: f64) -> f64 {
    let max_iter = 200;
    let eps = 1e-14;
    let tiny = 1e-30;

    let mut c = 1.0;
    let mut d = 1.0 - (a + b) * x / (a + 1.0);
    if d.abs() < tiny {
        d = tiny;
    }
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=max_iter {
        let m_f = m as f64;

        // Even step.
        let num = m_f * (b - m_f) * x / ((a + 2.0 * m_f - 1.0) * (a + 2.0 * m_f));
        d = 1.0 + num * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + num / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        h *= d * c;

        // Odd step.
        let num = -((a + m_f) * (a + b + m_f) * x) / ((a + 2.0 * m_f) * (a + 2.0 * m_f + 1.0));
        d = 1.0 + num * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + num / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;

        if (delta - 1.0).abs() < eps {
            break;
        }
    }
    h
}

/// Log of the beta function: ln B(a, b) = ln Γ(a) + ln Γ(b) - ln Γ(a+b).
fn ln_beta(a: f64, b: f64) -> f64 {
    lgamma(a) + lgamma(b) - lgamma(a + b)
}

/// Log-gamma function using Stirling's series.
fn lgamma(x: f64) -> f64 {
    if x < 7.0 {
        return lgamma(x + 1.0) - x.ln();
    }
    let inv = 1.0 / x;
    let inv2 = inv * inv;
    (x - 0.5) * x.ln() - x
        + 0.5 * (2.0 * std::f64::consts::PI).ln()
        + inv * (1.0 / 12.0 - inv2 * (1.0 / 360.0 - inv2 / 1260.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_assignment_is_deterministic() {
        let arm1 = assign_arm(12345);
        let arm2 = assign_arm(12345);
        assert_eq!(arm1, arm2);
    }

    #[test]
    fn experiment_fraction_splits_traffic() {
        // With 10% experiment fraction, fingerprints ending in 0-99
        // (out of 0-999) go to experiment.
        let mut experiment_count = 0;
        for fp in 0..10000_u64 {
            if should_use_experiment(fp) {
                experiment_count += 1;
            }
        }
        // Should be close to 10%
        assert!(
            experiment_count > 900 && experiment_count < 1100,
            "experiment_count={experiment_count}, expected ~1000"
        );
    }

    #[test]
    fn analysis_insufficient_data() {
        let result = analyze();
        assert_eq!(result.recommendation, Recommendation::InsufficientData);
    }

    #[test]
    fn cohens_d_zero_for_identical() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let d = cohens_d(&a, &a);
        assert!(d.abs() < 1e-10);
    }

    #[test]
    fn cohens_d_nonzero_for_different() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![4.0, 5.0, 6.0, 7.0, 8.0];
        let d = cohens_d(&a, &b);
        assert!(
            d.abs() > 1.0,
            "d={d} should be large for well-separated groups"
        );
    }

    #[test]
    fn t_test_identical_samples_not_significant() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let p = two_sample_t_test(&a, &a);
        assert!(p > 0.9, "p={p} should be ~1.0 for identical groups");
    }

    #[test]
    fn t_test_different_samples_significant() {
        let a = vec![1.0, 1.1, 0.9, 1.0, 1.05, 0.95, 1.02, 0.98];
        let b = vec![5.0, 5.1, 4.9, 5.0, 5.05, 4.95, 5.02, 4.98];
        let p = two_sample_t_test(&a, &b);
        assert!(p < 0.001, "p={p} should be very small for separated groups");
    }

    #[test]
    fn record_result_stores_values() {
        reset();
        record_result(Arm::Control, 50.0, 45.0);
        record_result(Arm::Control, 50.0, 50.0);
        record_result(Arm::Experiment, 50.0, 60.0);

        let state = AB_STATE.lock().unwrap();
        assert_eq!(state.control_results.len(), 2);
        assert_eq!(state.experiment_results.len(), 1);
    }

    #[test]
    fn mean_of_empty_is_zero() {
        assert!((mean(&[]) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn variance_of_constant_is_zero() {
        let data = vec![5.0, 5.0, 5.0, 5.0];
        assert!((variance(&data) - 0.0).abs() < f64::EPSILON);
    }
}
