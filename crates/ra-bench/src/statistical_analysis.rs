//! Statistical analysis framework for Ra vs PostgreSQL benchmark comparisons.
//!
//! Implements rigorous statistical methods for performance evaluation:
//! - Paired two-tailed t-tests with exact p-values via the regularized
//!   incomplete beta function
//! - 95% (configurable) confidence intervals
//! - Cohen's d effect sizes with standard magnitude interpretation
//! - Bonferroni correction for family-wise error rate control
//! - Modified Z-score outlier detection using median absolute deviation
//!
//! # Example
//!
//! ```ignore
//! use ra_bench::statistical_analysis::{
//!     BenchmarkComparison, StatisticalAnalyzer, AnalyzerConfig,
//! };
//!
//! let analyzer = StatisticalAnalyzer::with_defaults();
//! let comparisons = vec![BenchmarkComparison {
//!     query_id: "Q1".to_string(),
//!     ra_times_ms: vec![10.2, 9.8, 10.5, 10.1],
//!     postgres_times_ms: vec![14.1, 13.9, 14.3, 14.0],
//! }];
//! let report = analyzer.analyze_workload("tpch_sf1", &comparisons);
//! ```

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from the statistical analysis pipeline.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// Not enough samples to run a valid statistical test.
    #[error("insufficient samples: need {required}, have {actual}")]
    InsufficientSamples { required: usize, actual: usize },

    /// Degenerate input data (e.g., all zeros).
    #[error("invalid data: {0}")]
    InvalidData(String),
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// A confidence interval for a quantity (e.g., mean difference).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceInterval {
    /// Point estimate (mean).
    pub mean: f64,
    /// Lower bound of the interval.
    pub lower: f64,
    /// Upper bound of the interval.
    pub upper: f64,
    /// Nominal confidence level (e.g., 0.95 for 95%).
    pub confidence_level: f64,
}

/// Result of a paired two-tailed t-test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTestResult {
    /// The t-statistic (signed: negative means Ra is slower).
    pub t_statistic: f64,
    /// Two-tailed p-value.
    pub p_value: f64,
    /// Degrees of freedom (n − 1).
    pub degrees_of_freedom: usize,
    /// Confidence interval on the mean paired difference (Ra − Postgres).
    pub confidence_interval: ConfidenceInterval,
    /// Whether p_value < α (uncorrected).
    pub is_significant: bool,
}

/// Conventional magnitude bands for Cohen's d.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectMagnitude {
    /// |d| < 0.2 – practically negligible.
    Negligible,
    /// 0.2 ≤ |d| < 0.5 – small.
    Small,
    /// 0.5 ≤ |d| < 0.8 – medium.
    Medium,
    /// |d| ≥ 0.8 – large.
    Large,
}

/// Cohen's d effect size between two groups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectSize {
    /// Cohen's d (negative when Ra is slower).
    pub cohens_d: f64,
    /// Conventional magnitude interpretation.
    pub magnitude: EffectMagnitude,
}

/// Raw timing observations for one query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    /// Human-readable query identifier (e.g., "Q3", "tpch_q14").
    pub query_id: String,
    /// Repeated execution times under the Ra planner (milliseconds).
    pub ra_times_ms: Vec<f64>,
    /// Repeated execution times under the standard PostgreSQL planner (milliseconds).
    pub postgres_times_ms: Vec<f64>,
}

/// Statistical analysis result for a single query comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    /// Query identifier.
    pub query_id: String,
    /// Mean Ra execution time (ms) after outlier removal.
    pub ra_mean_ms: f64,
    /// Mean PostgreSQL execution time (ms) after outlier removal.
    pub postgres_mean_ms: f64,
    /// Relative improvement: (pg − ra) / pg × 100.
    /// Positive ⇒ Ra is faster; negative ⇒ Ra is slower (regression).
    pub improvement_pct: f64,
    /// Paired t-test result.
    pub t_test: TTestResult,
    /// Cohen's d effect size.
    pub effect_size: EffectSize,
    /// Whether the result is significant after Bonferroni correction.
    pub significant_after_correction: bool,
    /// Number of outliers removed from Ra times.
    pub ra_outliers_removed: usize,
    /// Number of outliers removed from Postgres times.
    pub pg_outliers_removed: usize,
}

/// Aggregated analysis for an entire benchmark workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadAnalysis {
    /// Workload name (e.g., "tpch_sf10", "job_imdb").
    pub workload_name: String,
    /// Per-query comparison results (only for successfully analyzed queries).
    pub results: Vec<ComparisonResult>,
    /// Bonferroni-corrected significance threshold.
    pub corrected_alpha: f64,
    /// Percentage of analyzed queries with a significant improvement after correction.
    pub pct_significantly_improved: f64,
    /// Mean improvement across all analyzed queries (%).
    pub mean_improvement_pct: f64,
    /// Confidence interval on the mean improvement.
    pub improvement_ci: ConfidenceInterval,
    /// Number of statistically significant regressions detected.
    pub regression_count: usize,
    /// Number of queries skipped due to insufficient samples.
    pub skipped_count: usize,
}

// ---------------------------------------------------------------------------
// Analyzer configuration
// ---------------------------------------------------------------------------

/// Configuration for the statistical analyzer.
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// Confidence level for intervals (default: 0.95).
    pub confidence_level: f64,
    /// Family-wise error rate α for Bonferroni correction (default: 0.05).
    pub alpha: f64,
    /// Minimum valid sample count (default: 30).
    pub min_samples: usize,
    /// Maximum acceptable coefficient of variation before flagging instability (default: 0.05).
    pub max_cv: f64,
    /// Modified Z-score threshold for outlier removal (default: 3.5).
    pub outlier_threshold: f64,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
            alpha: 0.05,
            min_samples: 30,
            max_cv: 0.05,
            outlier_threshold: 3.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Main analyzer
// ---------------------------------------------------------------------------

/// Statistical analysis engine for Ra vs PostgreSQL performance comparisons.
pub struct StatisticalAnalyzer {
    config: AnalyzerConfig,
}

impl StatisticalAnalyzer {
    /// Create an analyzer with the given configuration.
    pub fn new(config: AnalyzerConfig) -> Self {
        Self { config }
    }

    /// Create an analyzer with default configuration (95% CI, α=0.05, n≥30).
    pub fn with_defaults() -> Self {
        Self::new(AnalyzerConfig::default())
    }

    /// Analyze a single query comparison against a family of n_total comparisons.
    ///
    /// The `n_total_comparisons` parameter drives Bonferroni correction: pass
    /// the total number of queries in the workload so the FWER is controlled
    /// at the workload level.
    pub fn analyze_comparison(
        &self,
        comparison: &BenchmarkComparison,
        n_total_comparisons: usize,
    ) -> Result<ComparisonResult, AnalysisError> {
        let ra_clean = self.remove_outliers(&comparison.ra_times_ms);
        let pg_clean = self.remove_outliers(&comparison.postgres_times_ms);

        let ra_outliers = comparison.ra_times_ms.len().saturating_sub(ra_clean.len());
        let pg_outliers = comparison.postgres_times_ms.len().saturating_sub(pg_clean.len());

        let n = ra_clean.len().min(pg_clean.len());
        if n < self.config.min_samples {
            return Err(AnalysisError::InsufficientSamples {
                required: self.config.min_samples,
                actual: n,
            });
        }

        let ra_mean = mean(&ra_clean[..n]);
        let pg_mean = mean(&pg_clean[..n]);

        let improvement_pct = if pg_mean > 0.0 {
            (pg_mean - ra_mean) / pg_mean * 100.0
        } else {
            0.0
        };

        let diffs: Vec<f64> = ra_clean[..n]
            .iter()
            .zip(pg_clean[..n].iter())
            .map(|(&a, &b)| a - b)
            .collect();

        let t_test = self.paired_t_test(&diffs, self.config.confidence_level)?;
        let effect_size = self.cohens_d(&ra_clean[..n], &pg_clean[..n]);

        let corrected_alpha = self.config.alpha / n_total_comparisons.max(1) as f64;
        let significant_after_correction = t_test.p_value < corrected_alpha;

        Ok(ComparisonResult {
            query_id: comparison.query_id.clone(),
            ra_mean_ms: ra_mean,
            postgres_mean_ms: pg_mean,
            improvement_pct,
            t_test,
            effect_size,
            significant_after_correction,
            ra_outliers_removed: ra_outliers,
            pg_outliers_removed: pg_outliers,
        })
    }

    /// Analyze all comparisons in a workload and produce aggregated statistics.
    pub fn analyze_workload(
        &self,
        workload_name: &str,
        comparisons: &[BenchmarkComparison],
    ) -> WorkloadAnalysis {
        let n_total = comparisons.len();
        let corrected_alpha = self.config.alpha / n_total.max(1) as f64;

        let mut results = Vec::with_capacity(n_total);
        let mut skipped = 0usize;

        for comp in comparisons {
            match self.analyze_comparison(comp, n_total) {
                Ok(r) => results.push(r),
                Err(_) => skipped += 1,
            }
        }

        let pct_sig_improved = if results.is_empty() {
            0.0
        } else {
            let count = results
                .iter()
                .filter(|r| r.significant_after_correction && r.improvement_pct > 0.0)
                .count();
            count as f64 / results.len() as f64 * 100.0
        };

        let improvements: Vec<f64> = results.iter().map(|r| r.improvement_pct).collect();
        let mean_improvement_pct = mean(&improvements);
        let improvement_ci =
            confidence_interval_for_mean(&improvements, self.config.confidence_level);

        let regression_count = results
            .iter()
            .filter(|r| r.significant_after_correction && r.improvement_pct < 0.0)
            .count();

        WorkloadAnalysis {
            workload_name: workload_name.to_string(),
            results,
            corrected_alpha,
            pct_significantly_improved: pct_sig_improved,
            mean_improvement_pct,
            improvement_ci,
            regression_count,
            skipped_count: skipped,
        }
    }

    /// Remove outliers using the modified Z-score (MAD-based) method.
    ///
    /// Points with |modified Z| > threshold are discarded.
    /// At least 3 observations are needed; returns input unchanged otherwise.
    pub fn remove_outliers(&self, data: &[f64]) -> Vec<f64> {
        if data.len() < 3 {
            return data.to_vec();
        }
        let med = median(data);
        let deviations: Vec<f64> = data.iter().map(|&x| (x - med).abs()).collect();
        let mad = median(&deviations);
        if mad == 0.0 {
            return data.to_vec();
        }
        data.iter()
            .filter(|&&x| {
                let modified_z = 0.674_5 * (x - med).abs() / mad;
                modified_z <= self.config.outlier_threshold
            })
            .copied()
            .collect()
    }

    /// Paired two-tailed t-test on a vector of differences (Ra − Postgres).
    fn paired_t_test(
        &self,
        differences: &[f64],
        confidence_level: f64,
    ) -> Result<TTestResult, AnalysisError> {
        let n = differences.len();
        if n < 2 {
            return Err(AnalysisError::InsufficientSamples { required: 2, actual: n });
        }
        let mean_diff = mean(differences);
        let sd = std_dev(differences);

        // Zero SD means all differences are identical — perfectly consistent result.
        // t → ±∞, p → 0 (trivially significant if mean_diff ≠ 0).
        if sd == 0.0 {
            let t_stat = if mean_diff > 0.0 {
                f64::INFINITY
            } else if mean_diff < 0.0 {
                f64::NEG_INFINITY
            } else {
                0.0
            };
            let p_value = if mean_diff == 0.0 { 1.0 } else { 0.0 };
            return Ok(TTestResult {
                t_statistic: t_stat,
                p_value,
                degrees_of_freedom: n - 1,
                confidence_interval: ConfidenceInterval {
                    mean: mean_diff,
                    lower: mean_diff,
                    upper: mean_diff,
                    confidence_level,
                },
                is_significant: mean_diff != 0.0,
            });
        }

        let se = sd / (n as f64).sqrt();
        let t_stat = mean_diff / se;
        let df = n - 1;
        let p_value = two_tailed_t_pvalue(t_stat, df);
        let alpha_half = (1.0 - confidence_level) / 2.0;
        let t_crit = t_critical_value(alpha_half, df);
        let margin = t_crit * se;
        Ok(TTestResult {
            t_statistic: t_stat,
            p_value,
            degrees_of_freedom: df,
            confidence_interval: ConfidenceInterval {
                mean: mean_diff,
                lower: mean_diff - margin,
                upper: mean_diff + margin,
                confidence_level,
            },
            is_significant: p_value < self.config.alpha,
        })
    }

    /// Compute Cohen's d (pooled standard deviation) between two groups.
    fn cohens_d(&self, a: &[f64], b: &[f64]) -> EffectSize {
        let mean_a = mean(a);
        let mean_b = mean(b);
        let n_a = a.len() as f64;
        let n_b = b.len() as f64;
        let var_a = variance(a);
        let var_b = variance(b);
        let pooled_var = ((n_a - 1.0) * var_a + (n_b - 1.0) * var_b) / (n_a + n_b - 2.0);
        let pooled_std = pooled_var.sqrt();
        let d = if pooled_std == 0.0 { 0.0 } else { (mean_a - mean_b) / pooled_std };
        let magnitude = classify_effect(d);
        EffectSize { cohens_d: d, magnitude }
    }
}

fn classify_effect(d: f64) -> EffectMagnitude {
    match d.abs() {
        v if v < 0.2 => EffectMagnitude::Negligible,
        v if v < 0.5 => EffectMagnitude::Small,
        v if v < 0.8 => EffectMagnitude::Medium,
        _ => EffectMagnitude::Large,
    }
}

// ---------------------------------------------------------------------------
// Public statistical utilities
// ---------------------------------------------------------------------------

/// Confidence interval for the mean of a sample using the t-distribution.
pub fn confidence_interval_for_mean(data: &[f64], confidence_level: f64) -> ConfidenceInterval {
    let n = data.len();
    let m = mean(data);
    if n < 2 {
        return ConfidenceInterval { mean: m, lower: m, upper: m, confidence_level };
    }
    let se = std_dev(data) / (n as f64).sqrt();
    let alpha_half = (1.0 - confidence_level) / 2.0;
    let t_crit = t_critical_value(alpha_half, n - 1);
    let margin = t_crit * se;
    ConfidenceInterval { mean: m, lower: m - margin, upper: m + margin, confidence_level }
}

/// Coefficient of variation (σ / μ). Returns 0.0 when the mean is zero.
pub fn coeff_variation(data: &[f64]) -> f64 {
    let m = mean(data);
    if m == 0.0 {
        return 0.0;
    }
    std_dev(data) / m
}

/// Arithmetic mean. Returns 0.0 for empty input.
pub fn mean(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    data.iter().sum::<f64>() / data.len() as f64
}

/// Median (sorts a copy). Returns 0.0 for empty input.
pub fn median(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut s = data.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n % 2 == 0 {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    } else {
        s[n / 2]
    }
}

/// Sample variance (Bessel's correction: n − 1). Returns 0.0 for fewer than 2 points.
pub fn variance(data: &[f64]) -> f64 {
    let n = data.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(data);
    data.iter().map(|&x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64
}

/// Sample standard deviation (Bessel's correction).
pub fn std_dev(data: &[f64]) -> f64 {
    variance(data).sqrt()
}

// ---------------------------------------------------------------------------
// T-distribution: exact p-values via the regularized incomplete beta function
// ---------------------------------------------------------------------------

/// Two-tailed p-value for a t-statistic with `df` degrees of freedom.
///
/// Uses the identity: p = I_{df/(df+t²)}(df/2, 1/2)
/// where I is the regularized incomplete beta function.
pub fn two_tailed_t_pvalue(t: f64, df: usize) -> f64 {
    let df_f = df as f64;
    let t2 = t * t;
    let x = df_f / (df_f + t2);
    regularized_incomplete_beta(x, df_f / 2.0, 0.5)
}

/// Critical t-value such that P(T > t_crit | df) = `alpha_one_tail`.
///
/// Computed via bisection on `two_tailed_t_pvalue`.
pub fn t_critical_value(alpha_one_tail: f64, df: usize) -> f64 {
    let target = 2.0 * alpha_one_tail;
    // p decreases monotonically as t increases, so:
    //   p(0, df) = 1.0 > target (for any positive alpha)
    //   p(hi, df) < target for large enough hi
    let mut lo = 0.0_f64;
    let mut hi = 1000.0_f64;

    // If p(hi) is still above target (very small alpha), return hi as upper bound
    if two_tailed_t_pvalue(hi, df) >= target {
        return hi;
    }
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if two_tailed_t_pvalue(mid, df) > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

// ---------------------------------------------------------------------------
// Regularized incomplete beta function and helpers
// ---------------------------------------------------------------------------

/// Log-gamma function ln Γ(x) for x > 0.
///
/// Uses Stirling's asymptotic series for x ≥ 7, and the recurrence
/// ln Γ(x) = ln Γ(x+1) − ln x to shift smaller arguments into range.
fn lgamma(x: f64) -> f64 {
    if x < 7.0 {
        return lgamma(x + 1.0) - x.ln();
    }
    // Stirling's series: ln Γ(x) ≈ (x−½)ln x − x + ½ ln(2π) + 1/(12x) − 1/(360x³) + …
    let inv = 1.0 / x;
    let inv2 = inv * inv;
    (x - 0.5) * x.ln()
        - x
        + 0.5 * (2.0 * std::f64::consts::PI).ln()
        + inv * (1.0 / 12.0 - inv2 * (1.0 / 360.0 - inv2 / 1260.0))
}

/// Regularized incomplete beta function I_x(a, b), x ∈ [0, 1].
///
/// Implementation based on Numerical Recipes §6.4 (continued-fraction method).
fn regularized_incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Logarithm of the prefactor (Numerical Recipes eq. 6.4.5)
    let ln_bt = lgamma(a + b) - lgamma(a) - lgamma(b) + a * x.ln() + b * (1.0 - x).ln();
    let bt = ln_bt.exp();

    if x < (a + 1.0) / (a + b + 2.0) {
        bt * beta_continued_fraction(x, a, b) / a
    } else {
        1.0 - bt * beta_continued_fraction(1.0 - x, b, a) / b
    }
}

/// Lentz's continued-fraction evaluation for the incomplete beta function.
///
/// Computes the continued fraction CF(x; a, b) such that
/// I_x(a,b) = bt × CF / a (see `regularized_incomplete_beta`).
fn beta_continued_fraction(x: f64, a: f64, b: f64) -> f64 {
    const MAX_ITER: usize = 250;
    const EPS: f64 = 3.0e-10;
    const FPMIN: f64 = 1.0e-300;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;

    let mut c = 1.0_f64;
    let init = 1.0 - qab * x / qap;
    let mut d = if init.abs() < FPMIN { FPMIN } else { init };
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=MAX_ITER {
        let mf = m as f64;
        let m2 = 2.0 * mf;

        // Even step
        let aa = mf * (b - mf) * x / ((qam + m2) * (a + m2));
        d = {
            let v = 1.0 + aa * d;
            1.0 / if v.abs() < FPMIN { FPMIN } else { v }
        };
        c = {
            let v = 1.0 + aa / c;
            if v.abs() < FPMIN { FPMIN } else { v }
        };
        h *= d * c;

        // Odd step
        let aa = -(a + mf) * (qab + mf) * x / ((a + m2) * (qap + m2));
        d = {
            let v = 1.0 + aa * d;
            1.0 / if v.abs() < FPMIN { FPMIN } else { v }
        };
        c = {
            let v = 1.0 + aa / c;
            if v.abs() < FPMIN { FPMIN } else { v }
        };
        let del = d * c;
        h *= del;

        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Numerical functions ---

    #[test]
    fn test_mean_basic() {
        assert!((mean(&[1.0, 2.0, 3.0]) - 2.0).abs() < 1e-10);
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn test_median_odd_even() {
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < 1e-10);
        assert!((median(&[1.0, 2.0, 3.0, 4.0]) - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_variance_known() {
        // Variance of [2,4,4,4,5,5,7,9] = 4.571... (Bessel's correction)
        let data = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let v = variance(&data);
        assert!((v - 4.5714285).abs() < 1e-5, "variance={v}");
    }

    #[test]
    fn test_lgamma_known_values() {
        // Γ(1) = 1 → lgamma(1) = 0
        assert!(lgamma(1.0).abs() < 1e-8, "lgamma(1)={}", lgamma(1.0));
        // Γ(2) = 1 → lgamma(2) = 0
        assert!(lgamma(2.0).abs() < 1e-8, "lgamma(2)={}", lgamma(2.0));
        // Γ(0.5) = √π → lgamma(0.5) ≈ 0.5724
        let expected = (std::f64::consts::PI.sqrt()).ln();
        assert!((lgamma(0.5) - expected).abs() < 1e-7, "lgamma(0.5)={}", lgamma(0.5));
    }

    #[test]
    fn test_t_pvalue_large_t() {
        // Large t → p approaches 0
        let p = two_tailed_t_pvalue(100.0, 50);
        assert!(p < 1e-10, "p={p}");
    }

    #[test]
    fn test_t_pvalue_zero_t() {
        // t=0 → p=1
        let p = two_tailed_t_pvalue(0.0, 10);
        assert!((p - 1.0).abs() < 1e-6, "p={p}");
    }

    #[test]
    fn test_t_critical_matches_table() {
        // Standard table values: t_{0.025}(df) for two-tailed 95% CI
        // df=10: t_crit ≈ 2.228
        let t = t_critical_value(0.025, 10);
        assert!((t - 2.228).abs() < 0.01, "t_crit(df=10)={t}");
        // df=30: t_crit ≈ 2.042
        let t30 = t_critical_value(0.025, 30);
        assert!((t30 - 2.042).abs() < 0.01, "t_crit(df=30)={t30}");
        // Large df → approaches 1.96
        let t_inf = t_critical_value(0.025, 1000);
        assert!((t_inf - 1.96).abs() < 0.01, "t_crit(df=1000)={t_inf}");
    }

    // --- Outlier removal ---

    #[test]
    fn test_remove_outliers_removes_extreme() {
        let analyzer = StatisticalAnalyzer::with_defaults();
        let mut data: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        data.push(1000.0); // extreme outlier
        let clean = analyzer.remove_outliers(&data);
        assert!(!clean.contains(&1000.0), "outlier should be removed");
        assert_eq!(clean.len(), 20);
    }

    #[test]
    fn test_remove_outliers_small_input() {
        let analyzer = StatisticalAnalyzer::with_defaults();
        let data = vec![1.0, 2.0];
        let clean = analyzer.remove_outliers(&data);
        assert_eq!(clean, data); // unchanged
    }

    // --- Full comparison analysis ---

    #[test]
    fn test_analyze_comparison_insufficient_samples() {
        let analyzer = StatisticalAnalyzer::with_defaults();
        let comp = BenchmarkComparison {
            query_id: "Q1".to_string(),
            ra_times_ms: vec![10.0; 5], // < min_samples=30
            postgres_times_ms: vec![12.0; 5],
        };
        let result = analyzer.analyze_comparison(&comp, 1);
        assert!(matches!(result, Err(AnalysisError::InsufficientSamples { .. })));
    }

    #[test]
    fn test_analyze_comparison_detects_improvement() {
        let analyzer = StatisticalAnalyzer::new(AnalyzerConfig {
            min_samples: 5,
            ..Default::default()
        });
        // Ra faster by ~20%; add slight variation so paired diffs have non-zero SD.
        let ra_times: Vec<f64> = (0..10).map(|i| 10.0 + (i as f64) * 0.1).collect();
        let pg_times: Vec<f64> = (0..10).map(|i| 12.5 + (i as f64) * 0.15).collect();
        let comp = BenchmarkComparison {
            query_id: "Q1".to_string(),
            ra_times_ms: ra_times,
            postgres_times_ms: pg_times,
        };
        let result = analyzer.analyze_comparison(&comp, 1).unwrap();
        assert!(result.improvement_pct > 0.0, "expected positive improvement");
    }

    #[test]
    fn test_analyze_comparison_zero_std_is_valid() {
        // When all paired diffs are identical (perfect consistency), sd=0 is valid.
        let analyzer = StatisticalAnalyzer::new(AnalyzerConfig {
            min_samples: 3,
            ..Default::default()
        });
        // Ra always exactly 2ms faster.
        let comp = BenchmarkComparison {
            query_id: "Q_perf".to_string(),
            ra_times_ms: vec![10.0, 10.0, 10.0, 10.0],
            postgres_times_ms: vec![12.0, 12.0, 12.0, 12.0],
        };
        let result = analyzer.analyze_comparison(&comp, 1).unwrap();
        assert!(result.improvement_pct > 0.0);
        assert!(result.t_test.is_significant);
        assert_eq!(result.t_test.p_value, 0.0);
    }

    #[test]
    fn test_workload_analysis_regression_count() {
        let analyzer = StatisticalAnalyzer::new(AnalyzerConfig {
            min_samples: 3,
            ..Default::default()
        });
        // Ra slower (regression) for one query
        let regression = BenchmarkComparison {
            query_id: "slow".to_string(),
            ra_times_ms: vec![20.0, 20.1, 20.2],
            postgres_times_ms: vec![10.0, 10.1, 10.2],
        };
        let workload = analyzer.analyze_workload("test", &[regression]);
        // We just check the workload runs without panic
        assert_eq!(workload.workload_name, "test");
    }

    #[test]
    fn test_coeff_variation() {
        let data = [1.0, 2.0, 3.0]; // mean=2, sd≈1, cv≈0.5
        let cv = coeff_variation(&data);
        assert!(cv > 0.0 && cv < 1.0, "cv={cv}");
        assert_eq!(coeff_variation(&[]), 0.0);
        assert_eq!(coeff_variation(&[0.0, 0.0]), 0.0);
    }
}
