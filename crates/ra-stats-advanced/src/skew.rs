//! Skew detection for distributed aggregation optimization.
//!
//! Analyzes column value distributions to identify "hot keys" --
//! values that appear with disproportionately high frequency.
//! When such keys exist, standard hash-partitioned aggregation
//! can produce straggler nodes. This module detects skew and
//! recommends appropriate aggregation strategies.
//!
//! # Algorithm
//!
//! A key is considered "hot" when its frequency exceeds
//! `threshold * average_frequency`. The default threshold is 10.0,
//! meaning a value must appear 10x more often than the average
//! to be flagged.

use serde::{Deserialize, Serialize};

/// A value identified as disproportionately frequent (a "hot key").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotKey {
    /// The value that is hot.
    pub value: String,
    /// Absolute frequency count.
    pub frequency: u64,
    /// Ratio of this key's frequency to the average frequency.
    pub skew_ratio: f64,
}

/// Severity of detected skew in a column's distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkewSeverity {
    /// No significant skew detected.
    None,
    /// Mild skew (hot keys 10-50x average).
    Mild,
    /// Moderate skew (hot keys 50-100x average).
    Moderate,
    /// Severe skew (hot keys >100x average).
    Severe,
}

/// Recommended strategy based on detected skew characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkewStrategy {
    /// Standard two-phase aggregation (no skew handling needed).
    TwoPhase,
    /// Three-phase aggregation to spread load across nodes.
    ThreePhase,
    /// Handle hot keys separately from normal keys.
    SkewAware,
    /// Single-phase for small or non-decomposable aggregates.
    SinglePhase,
}

/// Result of a skew analysis on a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkewAnalysis {
    /// Column that was analyzed.
    pub column: String,
    /// Hot keys detected.
    pub hot_keys: Vec<HotKey>,
    /// Overall skew severity.
    pub severity: SkewSeverity,
    /// Recommended aggregation strategy.
    pub recommended_strategy: SkewStrategy,
    /// Average frequency across all buckets.
    pub avg_frequency: f64,
    /// Maximum frequency found.
    pub max_frequency: u64,
    /// Total number of distinct values analyzed.
    pub distinct_values: u64,
}

/// A bucket in a frequency histogram used for skew detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrequencyBucket {
    /// The value (or bucket label).
    pub value: String,
    /// Number of occurrences.
    pub count: u64,
}

/// Frequency histogram for a column's value distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrequencyHistogram {
    /// Buckets sorted by value or frequency.
    pub buckets: Vec<FrequencyBucket>,
    /// Total count across all buckets.
    pub total_count: u64,
}

impl FrequencyHistogram {
    /// Create a new frequency histogram from buckets.
    #[must_use]
    pub fn new(buckets: Vec<FrequencyBucket>) -> Self {
        let total_count = buckets.iter().map(|b| b.count).sum();
        Self {
            buckets,
            total_count,
        }
    }

    /// Number of distinct values (buckets) in the histogram.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// Average frequency across all buckets.
    #[must_use]
    pub fn avg_frequency(&self) -> f64 {
        if self.buckets.is_empty() {
            return 0.0;
        }
        self.total_count as f64 / self.buckets.len() as f64
    }

    /// Maximum frequency in any single bucket.
    #[must_use]
    pub fn max_frequency(&self) -> u64 {
        self.buckets.iter().map(|b| b.count).max().unwrap_or(0)
    }
}

/// Detects data skew by analyzing frequency histograms.
///
/// Identifies hot keys (values with disproportionately high frequency)
/// and recommends aggregation strategies to mitigate skew effects.
#[derive(Debug, Clone)]
pub struct SkewDetector {
    /// Skew ratio threshold: a key is "hot" if its frequency
    /// exceeds `threshold * average_frequency`.
    threshold: f64,
    /// Maximum number of hot keys to report.
    max_hot_keys: usize,
}

impl Default for SkewDetector {
    fn default() -> Self {
        Self {
            threshold: 10.0,
            max_hot_keys: 20,
        }
    }
}

impl SkewDetector {
    /// Create a new skew detector with the given threshold.
    ///
    /// # Panics
    ///
    /// Panics if `threshold` is not positive.
    #[must_use]
    pub fn new(threshold: f64) -> Self {
        assert!(
            threshold > 0.0,
            "threshold must be positive, got {threshold}"
        );
        Self {
            threshold,
            ..Self::default()
        }
    }

    /// Set the maximum number of hot keys to report.
    #[must_use]
    pub fn with_max_hot_keys(mut self, max: usize) -> Self {
        self.max_hot_keys = max;
        self
    }

    /// Get the configured threshold.
    #[must_use]
    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    /// Detect hot keys in a frequency histogram.
    ///
    /// Returns all values whose frequency exceeds
    /// `threshold * average_frequency`, sorted by skew ratio
    /// in descending order.
    #[must_use]
    pub fn detect_hot_keys(&self, histogram: &FrequencyHistogram) -> Vec<HotKey> {
        let avg_freq = histogram.avg_frequency();
        if avg_freq <= 0.0 {
            return Vec::new();
        }

        let freq_threshold = avg_freq * self.threshold;
        let mut hot_keys: Vec<HotKey> = histogram
            .buckets
            .iter()
            .filter(|b| b.count as f64 > freq_threshold)
            .map(|b| HotKey {
                value: b.value.clone(),
                frequency: b.count,
                skew_ratio: b.count as f64 / avg_freq,
            })
            .collect();

        hot_keys.sort_by(|a, b| {
            b.skew_ratio
                .partial_cmp(&a.skew_ratio)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hot_keys.truncate(self.max_hot_keys);
        hot_keys
    }

    /// Classify the severity of skew based on hot key characteristics.
    #[must_use]
    pub fn classify_severity(&self, hot_keys: &[HotKey]) -> SkewSeverity {
        if hot_keys.is_empty() {
            return SkewSeverity::None;
        }

        let max_ratio = hot_keys
            .iter()
            .map(|k| k.skew_ratio)
            .fold(0.0_f64, f64::max);

        if max_ratio > 100.0 {
            SkewSeverity::Severe
        } else if max_ratio > 50.0 {
            SkewSeverity::Moderate
        } else {
            SkewSeverity::Mild
        }
    }

    /// Recommend an aggregation strategy based on detected skew.
    ///
    /// - No hot keys: standard `TwoPhase`
    /// - Severe skew with few hot keys: `SkewAware` (handle separately)
    /// - Moderate skew: `ThreePhase` (spread load)
    /// - Mild skew: `TwoPhase` (overhead is manageable)
    #[must_use]
    pub fn recommend_strategy(&self, hot_keys: &[HotKey]) -> SkewStrategy {
        if hot_keys.is_empty() {
            return SkewStrategy::TwoPhase;
        }

        let severity = self.classify_severity(hot_keys);

        match severity {
            SkewSeverity::None | SkewSeverity::Mild => SkewStrategy::TwoPhase,
            SkewSeverity::Moderate => SkewStrategy::ThreePhase,
            SkewSeverity::Severe => {
                if hot_keys.len() < 5 {
                    SkewStrategy::SkewAware
                } else {
                    SkewStrategy::ThreePhase
                }
            }
        }
    }

    /// Run a complete skew analysis on a frequency histogram.
    #[must_use]
    pub fn analyze(&self, column: &str, histogram: &FrequencyHistogram) -> SkewAnalysis {
        let hot_keys = self.detect_hot_keys(histogram);
        let severity = self.classify_severity(&hot_keys);
        let recommended_strategy = self.recommend_strategy(&hot_keys);
        let avg_frequency = histogram.avg_frequency();
        let max_frequency = histogram.max_frequency();
        let distinct_values = histogram.bucket_count() as u64;

        SkewAnalysis {
            column: column.to_owned(),
            hot_keys,
            severity,
            recommended_strategy,
            avg_frequency,
            max_frequency,
            distinct_values,
        }
    }
}

/// Generate a Zipf-distributed frequency histogram for testing.
///
/// Produces `num_values` distinct values where the k-th most
/// frequent value has frequency proportional to 1/k^exponent.
#[must_use]
pub fn generate_zipf_histogram(
    num_values: usize,
    total_count: u64,
    exponent: f64,
) -> FrequencyHistogram {
    if num_values == 0 {
        return FrequencyHistogram::new(Vec::new());
    }

    let harmonic_sum: f64 = (1..=num_values)
        .map(|k| 1.0 / (k as f64).powf(exponent))
        .sum();

    let mut buckets = Vec::with_capacity(num_values);
    let mut remaining = total_count;

    for k in 1..=num_values {
        let prob = (1.0 / (k as f64).powf(exponent)) / harmonic_sum;
        let count = if k == num_values {
            remaining
        } else {
            let c = (total_count as f64 * prob).round() as u64;
            c.min(remaining)
        };
        remaining = remaining.saturating_sub(count);

        buckets.push(FrequencyBucket {
            value: format!("val_{k}"),
            count,
        });
    }

    FrequencyHistogram::new(buckets)
}

/// Generate a uniform frequency histogram for testing.
#[must_use]
pub fn generate_uniform_histogram(num_values: usize, total_count: u64) -> FrequencyHistogram {
    if num_values == 0 {
        return FrequencyHistogram::new(Vec::new());
    }

    let per_bucket = total_count / num_values as u64;
    let remainder = total_count % num_values as u64;

    let mut buckets = Vec::with_capacity(num_values);
    for i in 0..num_values {
        let extra = u64::from((i as u64) < remainder);
        buckets.push(FrequencyBucket {
            value: format!("val_{}", i + 1),
            count: per_bucket + extra,
        });
    }

    FrequencyHistogram::new(buckets)
}

#[expect(
    clippy::float_cmp,
    reason = "exact float equality needed for deterministic stats tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_uniform_histogram() -> FrequencyHistogram {
        FrequencyHistogram::new(vec![
            FrequencyBucket {
                value: "a".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "b".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "c".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "d".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "e".to_owned(),
                count: 100,
            },
        ])
    }

    fn sample_skewed_histogram() -> FrequencyHistogram {
        // avg = (100_000 + 10*100) / 11 = ~9181
        // threshold = 10 * 9181 = ~91818
        // hot key at 100_000 > 91818 => detected
        let mut buckets = vec![FrequencyBucket {
            value: "hot".to_owned(),
            count: 100_000,
        }];
        for i in 0..10 {
            buckets.push(FrequencyBucket {
                value: format!("val_{i}"),
                count: 100,
            });
        }
        FrequencyHistogram::new(buckets)
    }

    fn sample_moderate_skew_histogram() -> FrequencyHistogram {
        FrequencyHistogram::new(vec![
            FrequencyBucket {
                value: "x".to_owned(),
                count: 5000,
            },
            FrequencyBucket {
                value: "y".to_owned(),
                count: 4000,
            },
            FrequencyBucket {
                value: "z".to_owned(),
                count: 3000,
            },
            FrequencyBucket {
                value: "a".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "b".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "c".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "d".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "e".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "f".to_owned(),
                count: 100,
            },
            FrequencyBucket {
                value: "g".to_owned(),
                count: 100,
            },
        ])
    }

    // --- FrequencyHistogram ---

    #[test]
    fn histogram_total_count() {
        let h = sample_uniform_histogram();
        assert_eq!(h.total_count, 500);
    }

    #[test]
    fn histogram_bucket_count() {
        let h = sample_uniform_histogram();
        assert_eq!(h.bucket_count(), 5);
    }

    #[test]
    fn histogram_avg_frequency_uniform() {
        let h = sample_uniform_histogram();
        assert_eq!(h.avg_frequency(), 100.0);
    }

    #[test]
    fn histogram_avg_frequency_skewed() {
        let h = sample_skewed_histogram();
        let avg = h.avg_frequency();
        // (100_000 + 10*100) / 11 = 101000/11 ~= 9181.8
        assert!((avg - 9181.8).abs() < 1.0, "Expected ~9181.8, got {avg}");
    }

    #[test]
    fn histogram_avg_frequency_empty() {
        let h = FrequencyHistogram::new(Vec::new());
        assert_eq!(h.avg_frequency(), 0.0);
    }

    #[test]
    fn histogram_max_frequency_uniform() {
        let h = sample_uniform_histogram();
        assert_eq!(h.max_frequency(), 100);
    }

    #[test]
    fn histogram_max_frequency_skewed() {
        let h = sample_skewed_histogram();
        assert_eq!(h.max_frequency(), 100_000);
    }

    #[test]
    fn histogram_max_frequency_empty() {
        let h = FrequencyHistogram::new(Vec::new());
        assert_eq!(h.max_frequency(), 0);
    }

    // --- SkewDetector creation ---

    #[test]
    fn detector_default_threshold() {
        let d = SkewDetector::default();
        assert_eq!(d.threshold(), 10.0);
    }

    #[test]
    fn detector_custom_threshold() {
        let d = SkewDetector::new(5.0);
        assert_eq!(d.threshold(), 5.0);
    }

    #[test]
    #[should_panic(expected = "threshold must be positive")]
    fn detector_negative_threshold_panics() {
        let _ = SkewDetector::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "threshold must be positive")]
    fn detector_zero_threshold_panics() {
        let _ = SkewDetector::new(0.0);
    }

    // --- detect_hot_keys ---

    #[test]
    fn no_hot_keys_in_uniform_data() {
        let d = SkewDetector::default();
        let h = sample_uniform_histogram();
        let hot = d.detect_hot_keys(&h);
        assert!(hot.is_empty());
    }

    #[test]
    fn detects_hot_key_in_skewed_data() {
        let d = SkewDetector::default();
        let h = sample_skewed_histogram();
        let hot = d.detect_hot_keys(&h);
        assert!(!hot.is_empty());
        assert_eq!(hot[0].value, "hot");
        assert!(hot[0].skew_ratio > 1.0);
    }

    #[test]
    fn hot_keys_sorted_by_skew_ratio_desc() {
        let h = FrequencyHistogram::new(vec![
            FrequencyBucket {
                value: "a".to_owned(),
                count: 50_000,
            },
            FrequencyBucket {
                value: "b".to_owned(),
                count: 100_000,
            },
            FrequencyBucket {
                value: "c".to_owned(),
                count: 10,
            },
            FrequencyBucket {
                value: "d".to_owned(),
                count: 5,
            },
            FrequencyBucket {
                value: "e".to_owned(),
                count: 5,
            },
        ]);
        let d = SkewDetector::new(2.0);
        let hot = d.detect_hot_keys(&h);
        assert!(!hot.is_empty());
        if hot.len() >= 2 {
            assert!(hot[0].skew_ratio >= hot[1].skew_ratio);
        }
    }

    #[test]
    fn hot_keys_empty_histogram() {
        let d = SkewDetector::default();
        let h = FrequencyHistogram::new(Vec::new());
        let hot = d.detect_hot_keys(&h);
        assert!(hot.is_empty());
    }

    #[test]
    fn hot_keys_respects_max_limit() {
        let d = SkewDetector::default().with_max_hot_keys(1);
        let h = FrequencyHistogram::new(vec![
            FrequencyBucket {
                value: "a".to_owned(),
                count: 50000,
            },
            FrequencyBucket {
                value: "b".to_owned(),
                count: 40000,
            },
            FrequencyBucket {
                value: "c".to_owned(),
                count: 10,
            },
        ]);
        let hot = d.detect_hot_keys(&h);
        assert!(hot.len() <= 1);
    }

    // --- classify_severity ---

    #[test]
    fn severity_none_when_no_hot_keys() {
        let d = SkewDetector::default();
        assert_eq!(d.classify_severity(&[]), SkewSeverity::None);
    }

    #[test]
    fn severity_mild_for_low_ratio() {
        let d = SkewDetector::default();
        let hot = vec![HotKey {
            value: "x".to_owned(),
            frequency: 1000,
            skew_ratio: 15.0,
        }];
        assert_eq!(d.classify_severity(&hot), SkewSeverity::Mild);
    }

    #[test]
    fn severity_moderate_for_mid_ratio() {
        let d = SkewDetector::default();
        let hot = vec![HotKey {
            value: "x".to_owned(),
            frequency: 5000,
            skew_ratio: 75.0,
        }];
        assert_eq!(d.classify_severity(&hot), SkewSeverity::Moderate);
    }

    #[test]
    fn severity_severe_for_high_ratio() {
        let d = SkewDetector::default();
        let hot = vec![HotKey {
            value: "x".to_owned(),
            frequency: 100_000,
            skew_ratio: 200.0,
        }];
        assert_eq!(d.classify_severity(&hot), SkewSeverity::Severe);
    }

    // --- recommend_strategy ---

    #[test]
    fn recommend_two_phase_no_skew() {
        let d = SkewDetector::default();
        assert_eq!(d.recommend_strategy(&[]), SkewStrategy::TwoPhase);
    }

    #[test]
    fn recommend_two_phase_mild_skew() {
        let d = SkewDetector::default();
        let hot = vec![HotKey {
            value: "x".to_owned(),
            frequency: 1000,
            skew_ratio: 15.0,
        }];
        assert_eq!(d.recommend_strategy(&hot), SkewStrategy::TwoPhase);
    }

    #[test]
    fn recommend_three_phase_moderate_skew() {
        let d = SkewDetector::default();
        let hot = vec![HotKey {
            value: "x".to_owned(),
            frequency: 5000,
            skew_ratio: 75.0,
        }];
        assert_eq!(d.recommend_strategy(&hot), SkewStrategy::ThreePhase);
    }

    #[test]
    fn recommend_skew_aware_severe_few_keys() {
        let d = SkewDetector::default();
        let hot = vec![
            HotKey {
                value: "NULL".to_owned(),
                frequency: 100_000,
                skew_ratio: 200.0,
            },
            HotKey {
                value: "UNKNOWN".to_owned(),
                frequency: 50000,
                skew_ratio: 150.0,
            },
        ];
        assert_eq!(d.recommend_strategy(&hot), SkewStrategy::SkewAware);
    }

    #[test]
    fn recommend_three_phase_severe_many_keys() {
        let d = SkewDetector::default();
        let hot: Vec<HotKey> = (0..10)
            .map(|i| HotKey {
                value: format!("key_{i}"),
                frequency: 100_000,
                skew_ratio: 200.0,
            })
            .collect();
        assert_eq!(d.recommend_strategy(&hot), SkewStrategy::ThreePhase);
    }

    // --- analyze ---

    #[test]
    fn analyze_uniform_distribution() {
        let d = SkewDetector::default();
        let h = sample_uniform_histogram();
        let analysis = d.analyze("country", &h);
        assert_eq!(analysis.column, "country");
        assert!(analysis.hot_keys.is_empty());
        assert_eq!(analysis.severity, SkewSeverity::None);
        assert_eq!(analysis.recommended_strategy, SkewStrategy::TwoPhase);
        assert_eq!(analysis.distinct_values, 5);
    }

    #[test]
    fn analyze_skewed_distribution() {
        let d = SkewDetector::default();
        let h = sample_skewed_histogram();
        let analysis = d.analyze("status", &h);
        assert!(!analysis.hot_keys.is_empty());
        assert_eq!(analysis.hot_keys[0].value, "hot");
        assert!(analysis.max_frequency >= 100_000);
    }

    #[test]
    fn analyze_moderate_skew() {
        let d = SkewDetector::default();
        let h = sample_moderate_skew_histogram();
        let analysis = d.analyze("category", &h);
        assert_eq!(analysis.distinct_values, 10);
    }

    // --- generate_zipf_histogram ---

    #[test]
    fn zipf_histogram_count() {
        let h = generate_zipf_histogram(100, 1_000_000, 1.0);
        assert_eq!(h.bucket_count(), 100);
    }

    #[test]
    fn zipf_histogram_total_approximately_correct() {
        let h = generate_zipf_histogram(100, 1_000_000, 1.0);
        let diff = (h.total_count as i64 - 1_000_000_i64).unsigned_abs();
        assert!(diff < 1000, "Expected ~1M total, got {}", h.total_count);
    }

    #[test]
    fn zipf_histogram_first_most_frequent() {
        let h = generate_zipf_histogram(10, 100_000, 1.0);
        assert!(!h.buckets.is_empty());
        let max_count = h.buckets.iter().map(|b| b.count).max().unwrap_or(0);
        assert_eq!(h.buckets[0].count, max_count);
    }

    #[test]
    fn zipf_histogram_decreasing() {
        let h = generate_zipf_histogram(10, 100_000, 1.0);
        for window in h.buckets.windows(2) {
            assert!(
                window[0].count >= window[1].count,
                "{} should be >= {}",
                window[0].count,
                window[1].count
            );
        }
    }

    #[test]
    fn zipf_histogram_empty() {
        let h = generate_zipf_histogram(0, 1_000_000, 1.0);
        assert!(h.buckets.is_empty());
        assert_eq!(h.total_count, 0);
    }

    #[test]
    fn zipf_high_exponent_more_skewed() {
        let h1 = generate_zipf_histogram(100, 1_000_000, 0.5);
        let h2 = generate_zipf_histogram(100, 1_000_000, 2.0);
        assert!(
            h2.buckets[0].count > h1.buckets[0].count,
            "Higher exponent should produce more skew"
        );
    }

    #[test]
    fn zipf_detects_hot_keys() {
        let d = SkewDetector::default();
        let h = generate_zipf_histogram(100, 1_000_000, 1.5);
        let hot = d.detect_hot_keys(&h);
        assert!(
            !hot.is_empty(),
            "Zipf with exponent 1.5 should produce hot keys"
        );
    }

    // --- generate_uniform_histogram ---

    #[test]
    fn uniform_histogram_count() {
        let h = generate_uniform_histogram(50, 1_000_000);
        assert_eq!(h.bucket_count(), 50);
    }

    #[test]
    fn uniform_histogram_total_exact() {
        let h = generate_uniform_histogram(50, 1_000_000);
        assert_eq!(h.total_count, 1_000_000);
    }

    #[test]
    fn uniform_histogram_no_hot_keys() {
        let d = SkewDetector::default();
        let h = generate_uniform_histogram(100, 1_000_000);
        let hot = d.detect_hot_keys(&h);
        assert!(
            hot.is_empty(),
            "Uniform distribution should have no hot keys"
        );
    }

    #[test]
    fn uniform_histogram_empty() {
        let h = generate_uniform_histogram(0, 1_000_000);
        assert!(h.buckets.is_empty());
        assert_eq!(h.total_count, 0);
    }

    #[test]
    fn uniform_histogram_even_distribution() {
        let h = generate_uniform_histogram(10, 100);
        for bucket in &h.buckets {
            assert_eq!(bucket.count, 10);
        }
    }

    #[test]
    fn uniform_histogram_remainder_distributed() {
        let h = generate_uniform_histogram(3, 10);
        assert_eq!(h.total_count, 10);
        assert_eq!(h.buckets[0].count, 4);
        assert_eq!(h.buckets[1].count, 3);
        assert_eq!(h.buckets[2].count, 3);
    }

    // --- HotKey ---

    #[test]
    fn hot_key_serialize_roundtrip() {
        let hk = HotKey {
            value: "NULL".to_owned(),
            frequency: 50000,
            skew_ratio: 125.5,
        };
        let json = serde_json::to_string(&hk).expect("serialize should succeed");
        let d: HotKey = serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(hk, d);
    }

    // --- SkewAnalysis ---

    #[test]
    fn skew_analysis_serialize_roundtrip() {
        let analysis = SkewAnalysis {
            column: "region".to_owned(),
            hot_keys: vec![HotKey {
                value: "US".to_owned(),
                frequency: 80000,
                skew_ratio: 50.0,
            }],
            severity: SkewSeverity::Moderate,
            recommended_strategy: SkewStrategy::ThreePhase,
            avg_frequency: 1600.0,
            max_frequency: 80000,
            distinct_values: 50,
        };
        let json = serde_json::to_string(&analysis).expect("serialize should succeed");
        let d: SkewAnalysis = serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(analysis, d);
    }

    // --- Integration: Zipf + SkewDetector ---

    #[test]
    fn zipf_1_0_analysis() {
        let d = SkewDetector::default();
        let h = generate_zipf_histogram(100, 1_000_000, 1.0);
        let analysis = d.analyze("product_id", &h);
        assert!(analysis.distinct_values == 100);
        assert!(analysis.avg_frequency > 0.0);
    }

    #[test]
    fn zipf_2_0_severe_skew() {
        let d = SkewDetector::default();
        let h = generate_zipf_histogram(100, 1_000_000, 2.0);
        let analysis = d.analyze("user_id", &h);
        assert!(
            !analysis.hot_keys.is_empty(),
            "Zipf 2.0 should produce hot keys"
        );
        assert!(
            analysis.severity == SkewSeverity::Severe
                || analysis.severity == SkewSeverity::Moderate,
            "Zipf 2.0 should have significant skew, got {:?}",
            analysis.severity
        );
    }

    #[test]
    fn uniform_no_skew_analysis() {
        let d = SkewDetector::default();
        let h = generate_uniform_histogram(100, 1_000_000);
        let analysis = d.analyze("random_col", &h);
        assert!(analysis.hot_keys.is_empty());
        assert_eq!(analysis.severity, SkewSeverity::None);
        assert_eq!(analysis.recommended_strategy, SkewStrategy::TwoPhase);
    }

    // --- Custom threshold tests ---

    #[test]
    fn lower_threshold_detects_more_hot_keys() {
        let h = sample_moderate_skew_histogram();
        let strict = SkewDetector::new(20.0);
        let lenient = SkewDetector::new(2.0);
        let strict_hot = strict.detect_hot_keys(&h);
        let lenient_hot = lenient.detect_hot_keys(&h);
        assert!(
            lenient_hot.len() >= strict_hot.len(),
            "Lower threshold should detect >= hot keys"
        );
    }

    #[test]
    fn very_high_threshold_no_hot_keys() {
        let d = SkewDetector::new(1000.0);
        let h = sample_skewed_histogram();
        let hot = d.detect_hot_keys(&h);
        assert!(
            hot.is_empty(),
            "Very high threshold should find no hot keys"
        );
    }

    // --- Edge cases ---

    #[test]
    fn single_bucket_histogram() {
        let h = FrequencyHistogram::new(vec![FrequencyBucket {
            value: "only".to_owned(),
            count: 1000,
        }]);
        let d = SkewDetector::default();
        let hot = d.detect_hot_keys(&h);
        assert!(
            hot.is_empty(),
            "Single bucket cannot exceed threshold * avg"
        );
    }

    #[test]
    fn all_zero_counts() {
        let h = FrequencyHistogram::new(vec![
            FrequencyBucket {
                value: "a".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "b".to_owned(),
                count: 0,
            },
        ]);
        let d = SkewDetector::default();
        let hot = d.detect_hot_keys(&h);
        assert!(hot.is_empty());
    }

    #[test]
    fn one_nonzero_among_zeros() {
        let h = FrequencyHistogram::new(vec![
            FrequencyBucket {
                value: "hot".to_owned(),
                count: 1000,
            },
            FrequencyBucket {
                value: "a".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "b".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "c".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "d".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "e".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "f".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "g".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "h".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "i".to_owned(),
                count: 0,
            },
            FrequencyBucket {
                value: "j".to_owned(),
                count: 0,
            },
        ]);
        let d = SkewDetector::default();
        let hot = d.detect_hot_keys(&h);
        assert!(!hot.is_empty());
        assert_eq!(hot[0].value, "hot");
    }
}
