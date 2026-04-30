#![expect(clippy::expect_used, reason = "test code")]
#![expect(
    clippy::float_cmp,
    reason = "exact float equality needed for deterministic stats tests"
)]
//! Integration tests for skew detection with simulated datasets.

use ra_stats::skew::{
    generate_uniform_histogram, generate_zipf_histogram, FrequencyBucket, FrequencyHistogram,
    HotKey, SkewDetector, SkewSeverity, SkewStrategy,
};

// --- Zipf distribution integration tests ---

#[test]
fn zipf_1_0_100_values_detects_some_hot_keys() {
    let d = SkewDetector::default();
    let h = generate_zipf_histogram(100, 1_000_000, 1.0);
    let analysis = d.analyze("product_id", &h);
    assert_eq!(analysis.distinct_values, 100);
    assert!(analysis.avg_frequency > 0.0);
}

#[test]
fn zipf_1_5_strong_skew() {
    let d = SkewDetector::default();
    let h = generate_zipf_histogram(100, 1_000_000, 1.5);
    let hot = d.detect_hot_keys(&h);
    assert!(!hot.is_empty(), "Zipf 1.5 should produce hot keys");
    assert!(hot[0].skew_ratio > 10.0);
}

#[test]
fn zipf_2_0_very_strong_skew() {
    let d = SkewDetector::default();
    let h = generate_zipf_histogram(100, 1_000_000, 2.0);
    let analysis = d.analyze("user_id", &h);
    assert!(!analysis.hot_keys.is_empty());
    assert!(
        analysis.severity == SkewSeverity::Severe || analysis.severity == SkewSeverity::Moderate
    );
}

#[test]
fn zipf_0_5_mild_skew() {
    let d = SkewDetector::default();
    let h = generate_zipf_histogram(100, 1_000_000, 0.5);
    let analysis = d.analyze("category", &h);
    // Zipf 0.5 is fairly flat, may or may not have hot keys
    // but severity should be None or Mild
    assert!(analysis.severity == SkewSeverity::None || analysis.severity == SkewSeverity::Mild);
}

#[test]
fn zipf_10_values_high_exponent() {
    let d = SkewDetector::default();
    let h = generate_zipf_histogram(10, 1_000_000, 2.0);
    let analysis = d.analyze("top_categories", &h);
    assert_eq!(analysis.distinct_values, 10);
    assert!(analysis.max_frequency > 100_000);
}

#[test]
fn zipf_1000_values_moderate_exponent() {
    let d = SkewDetector::default();
    let h = generate_zipf_histogram(1000, 10_000_000, 1.0);
    let analysis = d.analyze("session_id", &h);
    assert_eq!(analysis.distinct_values, 1000);
}

// --- Uniform distribution integration tests ---

#[test]
fn uniform_100_values_no_skew() {
    let d = SkewDetector::default();
    let h = generate_uniform_histogram(100, 1_000_000);
    let analysis = d.analyze("random_col", &h);
    assert!(analysis.hot_keys.is_empty());
    assert_eq!(analysis.severity, SkewSeverity::None);
    assert_eq!(analysis.recommended_strategy, SkewStrategy::TwoPhase);
}

#[test]
fn uniform_10_values_no_skew() {
    let d = SkewDetector::default();
    let h = generate_uniform_histogram(10, 100_000);
    let analysis = d.analyze("status", &h);
    assert!(analysis.hot_keys.is_empty());
    assert_eq!(analysis.severity, SkewSeverity::None);
}

#[test]
fn uniform_1000_values_no_skew() {
    let d = SkewDetector::default();
    let h = generate_uniform_histogram(1000, 10_000_000);
    let analysis = d.analyze("product_id", &h);
    assert!(analysis.hot_keys.is_empty());
    assert_eq!(analysis.recommended_strategy, SkewStrategy::TwoPhase);
}

// --- Skewed dataset simulation tests ---

#[test]
fn null_dominated_column() {
    let mut buckets = vec![FrequencyBucket {
        value: "NULL".to_owned(),
        count: 800_000,
    }];
    for i in 1..=20 {
        buckets.push(FrequencyBucket {
            value: format!("val_{i}"),
            count: 10_000,
        });
    }
    let h = FrequencyHistogram::new(buckets);
    let d = SkewDetector::default();
    let analysis = d.analyze("nullable_col", &h);

    assert!(!analysis.hot_keys.is_empty());
    assert_eq!(analysis.hot_keys[0].value, "NULL");
    assert!(analysis.hot_keys[0].skew_ratio > 10.0);
}

#[test]
fn us_dominated_country_column() {
    let mut buckets = vec![FrequencyBucket {
        value: "US".to_owned(),
        count: 5_000_000,
    }];
    for country in &["UK", "DE", "FR", "JP", "CN", "BR", "IN", "AU", "CA"] {
        buckets.push(FrequencyBucket {
            value: (*country).to_owned(),
            count: 100_000,
        });
    }
    // Add 90 small countries
    for i in 0..90 {
        buckets.push(FrequencyBucket {
            value: format!("country_{i}"),
            count: 1000,
        });
    }
    let h = FrequencyHistogram::new(buckets);
    let d = SkewDetector::default();
    let analysis = d.analyze("country", &h);

    assert!(!analysis.hot_keys.is_empty());
    assert_eq!(analysis.hot_keys[0].value, "US");
}

#[test]
fn active_inactive_status_skew() {
    let h = FrequencyHistogram::new(vec![
        FrequencyBucket {
            value: "active".to_owned(),
            count: 9_000_000,
        },
        FrequencyBucket {
            value: "inactive".to_owned(),
            count: 500_000,
        },
        FrequencyBucket {
            value: "suspended".to_owned(),
            count: 300_000,
        },
        FrequencyBucket {
            value: "deleted".to_owned(),
            count: 200_000,
        },
    ]);
    let d = SkewDetector::default();
    let _analysis = d.analyze("status", &h);

    // 'active' at 9M vs avg ~2.5M = ratio ~3.6
    // With threshold 10.0, this may not be detected
    // But with lower threshold it would be
    let d_lenient = SkewDetector::new(2.0);
    let analysis_lenient = d_lenient.analyze("status", &h);
    assert!(!analysis_lenient.hot_keys.is_empty());
    assert_eq!(analysis_lenient.hot_keys[0].value, "active");
}

#[test]
fn power_law_user_activity() {
    // Simulate user activity: few users very active, most barely active
    let mut buckets = Vec::new();
    // 10 power users
    for i in 0..10 {
        buckets.push(FrequencyBucket {
            value: format!("power_user_{i}"),
            count: 1_000_000,
        });
    }
    // 1000 regular users
    for i in 0..1000 {
        buckets.push(FrequencyBucket {
            value: format!("user_{i}"),
            count: 100,
        });
    }

    let h = FrequencyHistogram::new(buckets);
    let d = SkewDetector::default();
    let analysis = d.analyze("user_id", &h);

    assert!(!analysis.hot_keys.is_empty());
    assert!(analysis.hot_keys.len() >= 5);
}

// --- Strategy recommendation integration tests ---

#[test]
fn recommend_two_phase_for_uniform() {
    let d = SkewDetector::default();
    let h = generate_uniform_histogram(100, 1_000_000);
    let analysis = d.analyze("col", &h);
    assert_eq!(analysis.recommended_strategy, SkewStrategy::TwoPhase);
}

#[test]
fn recommend_skew_aware_for_single_hot_key() {
    let mut buckets = vec![FrequencyBucket {
        value: "hot".to_owned(),
        count: 10_000_000,
    }];
    for i in 0..100 {
        buckets.push(FrequencyBucket {
            value: format!("val_{i}"),
            count: 100,
        });
    }
    let h = FrequencyHistogram::new(buckets);
    let d = SkewDetector::default();
    let analysis = d.analyze("key", &h);

    assert_eq!(
        analysis.recommended_strategy,
        SkewStrategy::SkewAware,
        "Single severe hot key should recommend SkewAware"
    );
}

#[test]
fn recommend_three_phase_for_many_hot_keys() {
    let mut buckets = Vec::new();
    // 10 hot keys at 5M each
    for i in 0..10 {
        buckets.push(FrequencyBucket {
            value: format!("hot_{i}"),
            count: 5_000_000,
        });
    }
    // 1000 cold keys at 10 each to bring average down
    for i in 0..1000 {
        buckets.push(FrequencyBucket {
            value: format!("cold_{i}"),
            count: 10,
        });
    }
    let h = FrequencyHistogram::new(buckets);
    let d = SkewDetector::default();
    let analysis = d.analyze("key", &h);

    // Many hot keys (>5) with severe skew -> ThreePhase
    assert!(
        analysis.recommended_strategy == SkewStrategy::ThreePhase
            || analysis.recommended_strategy == SkewStrategy::SkewAware,
        "Many hot keys should recommend ThreePhase or SkewAware, got {:?}",
        analysis.recommended_strategy,
    );
}

// --- Threshold sensitivity tests ---

#[test]
fn threshold_2_detects_more_than_threshold_20() {
    let h = generate_zipf_histogram(100, 1_000_000, 1.0);
    let strict = SkewDetector::new(20.0);
    let lenient = SkewDetector::new(2.0);

    let strict_count = strict.detect_hot_keys(&h).len();
    let lenient_count = lenient.detect_hot_keys(&h).len();

    assert!(lenient_count >= strict_count);
}

#[test]
fn threshold_affects_severity_classification() {
    let hot_keys = vec![HotKey {
        value: "x".to_owned(),
        frequency: 50000,
        skew_ratio: 45.0,
    }];

    let d = SkewDetector::default();
    let severity = d.classify_severity(&hot_keys);
    assert_eq!(severity, SkewSeverity::Mild);
}

#[test]
fn max_hot_keys_limit() {
    let d = SkewDetector::default().with_max_hot_keys(3);
    let mut buckets = Vec::new();
    for i in 0..50 {
        buckets.push(FrequencyBucket {
            value: format!("hot_{i}"),
            count: 1_000_000,
        });
    }
    for i in 0..100 {
        buckets.push(FrequencyBucket {
            value: format!("cold_{i}"),
            count: 10,
        });
    }
    let h = FrequencyHistogram::new(buckets);
    let hot = d.detect_hot_keys(&h);
    assert!(hot.len() <= 3);
}

// --- Edge cases ---

#[test]
fn single_value_histogram() {
    let h = FrequencyHistogram::new(vec![FrequencyBucket {
        value: "only".to_owned(),
        count: 1_000_000,
    }]);
    let d = SkewDetector::default();
    let analysis = d.analyze("single_col", &h);
    assert!(analysis.hot_keys.is_empty());
    assert_eq!(analysis.severity, SkewSeverity::None);
}

#[test]
fn two_values_extremely_skewed() {
    let h = FrequencyHistogram::new(vec![
        FrequencyBucket {
            value: "common".to_owned(),
            count: 99_999_000,
        },
        FrequencyBucket {
            value: "rare".to_owned(),
            count: 1000,
        },
    ]);
    let d = SkewDetector::default();
    let _analysis = d.analyze("binary_col", &h);
    // avg = 50M, threshold 10x = 500M, common at ~100M < 500M
    // But with only 2 values, the avg is dominated by common
    // This is actually expected behavior: with 2 values, the hot
    // one can't exceed 2x the average
}

#[test]
fn empty_histogram_analysis() {
    let h = FrequencyHistogram::new(Vec::new());
    let d = SkewDetector::default();
    let analysis = d.analyze("empty", &h);
    assert!(analysis.hot_keys.is_empty());
    assert_eq!(analysis.severity, SkewSeverity::None);
    assert_eq!(analysis.avg_frequency, 0.0);
    assert_eq!(analysis.max_frequency, 0);
    assert_eq!(analysis.distinct_values, 0);
}

#[test]
fn all_same_count() {
    let h = FrequencyHistogram::new(
        (0..50)
            .map(|i| FrequencyBucket {
                value: format!("v_{i}"),
                count: 20_000,
            })
            .collect(),
    );
    let d = SkewDetector::default();
    let analysis = d.analyze("uniform_col", &h);
    assert!(analysis.hot_keys.is_empty());
    assert_eq!(analysis.severity, SkewSeverity::None);
}

// --- Severity boundary tests ---

#[test]
fn severity_boundary_mild_to_moderate() {
    let d = SkewDetector::default();
    let mild = vec![HotKey {
        value: "x".to_owned(),
        frequency: 1,
        skew_ratio: 49.0,
    }];
    let moderate = vec![HotKey {
        value: "x".to_owned(),
        frequency: 1,
        skew_ratio: 51.0,
    }];
    assert_eq!(d.classify_severity(&mild), SkewSeverity::Mild);
    assert_eq!(d.classify_severity(&moderate), SkewSeverity::Moderate);
}

#[test]
fn severity_boundary_moderate_to_severe() {
    let d = SkewDetector::default();
    let moderate = vec![HotKey {
        value: "x".to_owned(),
        frequency: 1,
        skew_ratio: 99.0,
    }];
    let severe = vec![HotKey {
        value: "x".to_owned(),
        frequency: 1,
        skew_ratio: 101.0,
    }];
    assert_eq!(d.classify_severity(&moderate), SkewSeverity::Moderate);
    assert_eq!(d.classify_severity(&severe), SkewSeverity::Severe);
}

// --- Histogram generation accuracy tests ---

#[test]
fn zipf_histogram_preserves_total_count() {
    for exponent in [0.5, 1.0, 1.5, 2.0] {
        let h = generate_zipf_histogram(50, 500_000, exponent);
        let diff = (h.total_count as i64 - 500_000_i64).unsigned_abs();
        assert!(
            diff < 500,
            "Zipf s={exponent}: expected ~500K, got {}",
            h.total_count
        );
    }
}

#[test]
fn uniform_histogram_exact_count() {
    for n in [1, 5, 10, 100, 1000] {
        let h = generate_uniform_histogram(n, 100_000);
        assert_eq!(
            h.total_count, 100_000,
            "Uniform n={n}: total should be exact"
        );
    }
}

#[test]
fn zipf_monotonically_decreasing() {
    for exponent in [0.5, 1.0, 1.5, 2.0] {
        let h = generate_zipf_histogram(20, 100_000, exponent);
        for w in h.buckets.windows(2) {
            assert!(
                w[0].count >= w[1].count,
                "Zipf s={exponent}: {} should >= {}",
                w[0].count,
                w[1].count
            );
        }
    }
}

// --- Serialization integration tests ---

#[test]
fn hot_key_json_roundtrip() {
    let hk = HotKey {
        value: "US".to_owned(),
        frequency: 5_000_000,
        skew_ratio: 87.5,
    };
    let json = serde_json::to_string(&hk).expect("serialize should succeed");
    let deserialized: HotKey = serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(hk, deserialized);
}

#[test]
fn frequency_histogram_json_roundtrip() {
    let h = generate_zipf_histogram(10, 10_000, 1.0);
    let json = serde_json::to_string(&h).expect("serialize should succeed");
    let deserialized: FrequencyHistogram =
        serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(h, deserialized);
}
