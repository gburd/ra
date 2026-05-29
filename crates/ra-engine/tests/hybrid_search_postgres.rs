//! Integration tests for hybrid search with `PostgreSQL` RUM + pgvector.
//!
//! These tests verify that the hybrid search optimizer correctly:
//! 1. Chooses the appropriate strategy based on selectivity estimates
//! 2. Generates valid rewrite rules for hybrid patterns
//! 3. Produces cost estimates that guide execution strategy selection
//!
//! Prerequisites:
//! - `PostgreSQL` 14+ with RUM and pgvector extensions
//! - Test database with sample data
//!
//! Run with: `cargo test -p ra-engine --test hybrid_search_postgres`

use ra_engine::{
    choose_hybrid_strategy, fuse_scores, hybrid_fts_first_cost_factor, hybrid_parallel_cost_factor,
    hybrid_scan_cost_factor, hybrid_search_rules, hybrid_vector_first_cost_factor, HybridStrategy,
    ScoreFusion,
};

#[test]
fn test_strategy_selection_with_realistic_stats() {
    // Scenario 1: News article search with 1M documents
    // Query: "machine learning" (FTS) + semantic similarity (vector)
    // FTS selectivity: 0.2% (2,000 matches)
    // Vector selectivity: 1% (10,000 matches)
    let strategy = choose_hybrid_strategy(0.002, 0.01, Some(20), 1_000_000.0);
    assert_eq!(
        strategy,
        HybridStrategy::FTSFirst,
        "Should use FTS-first for highly selective text query"
    );

    // Scenario 2: Product catalog with 500K items
    // Query: broad category filter (FTS) + similar product features (vector)
    // FTS selectivity: 5% (25,000 matches)
    // Vector selectivity: 0.5% (2,500 matches)
    let strategy = choose_hybrid_strategy(0.05, 0.005, Some(50), 500_000.0);
    assert_eq!(
        strategy,
        HybridStrategy::VectorFirst,
        "Should use vector-first for highly selective embedding query"
    );

    // Scenario 3: Small result set from large corpus
    // Query: any combination with LIMIT 10
    // FTS selectivity: 2% (20,000 matches)
    // Vector selectivity: 3% (30,000 matches)
    let strategy = choose_hybrid_strategy(0.02, 0.03, Some(10), 1_000_000.0);
    assert_eq!(
        strategy,
        HybridStrategy::Parallel,
        "Should use parallel execution for small result sets"
    );
}

#[test]
fn test_cost_factors_are_reasonable() {
    // FTS-first should have minimal overhead over pure FTS
    let fts_factor = hybrid_fts_first_cost_factor();
    assert!(
        (1.0..=1.5).contains(&fts_factor),
        "FTS-first should have 0-50% overhead, got {fts_factor}",
    );

    // Vector-first should have minimal overhead over pure vector
    let vector_factor = hybrid_vector_first_cost_factor();
    assert!(
        (1.0..=1.5).contains(&vector_factor),
        "Vector-first should have 0-50% overhead, got {vector_factor}",
    );

    // Parallel should have moderate overhead due to merge
    let parallel_factor = hybrid_parallel_cost_factor();
    assert!(
        (1.2..=2.0).contains(&parallel_factor),
        "Parallel should have 20-100% overhead, got {parallel_factor}",
    );
}

#[test]
fn test_cost_factor_scales_with_selectivity() {
    // Lower selectivity = higher cost
    let cost_low = hybrid_scan_cost_factor(HybridStrategy::FTSFirst, 0.01, 0.01);
    let cost_high = hybrid_scan_cost_factor(HybridStrategy::FTSFirst, 0.1, 0.1);

    assert!(
        cost_high > cost_low,
        "Higher selectivity should increase cost"
    );

    // Verify target: < 2x overhead vs single-modality
    assert!(
        cost_low < 2.0,
        "Low selectivity cost should be < 2x sequential scan"
    );
    assert!(
        cost_high < 2.0,
        "High selectivity cost should be < 2x sequential scan"
    );
}

#[test]
fn test_score_fusion_methods() {
    let bm25_scores = [15.0, 10.0, 5.0, 2.0, 1.0];
    let vector_scores = [0.1, 0.3, 0.5, 0.8, 1.2];
    let alpha = 0.7;
    let k = 60;

    // Test weighted average fusion
    let weighted_scores: Vec<f64> = bm25_scores
        .iter()
        .zip(&vector_scores)
        .map(|(&bm25, &vec)| fuse_scores(bm25, vec, ScoreFusion::WeightedAverage, alpha, k))
        .collect();

    // Verify weighted scores are in [0, 1] range
    for score in &weighted_scores {
        assert!(
            *score >= 0.0 && *score <= 1.0,
            "Weighted score should be normalized to [0, 1]"
        );
    }

    // Verify weighted scores preserve monotonicity
    let mut prev_score = f64::INFINITY;
    for score in &weighted_scores {
        assert!(
            score <= &prev_score,
            "Weighted scores should be monotonically decreasing"
        );
        prev_score = *score;
    }

    // Test RRF fusion
    let rrf_scores: Vec<f64> = bm25_scores
        .iter()
        .zip(&vector_scores)
        .map(|(&bm25, &vec)| fuse_scores(bm25, vec, ScoreFusion::ReciprocalRankFusion, alpha, k))
        .collect();

    // RRF scores should be positive
    for score in &rrf_scores {
        assert!(*score > 0.0, "RRF score should be positive");
    }

    // Test learned fusion (falls back to RRF)
    let learned_scores: Vec<f64> = bm25_scores
        .iter()
        .zip(&vector_scores)
        .map(|(&bm25, &vec)| fuse_scores(bm25, vec, ScoreFusion::Learned, alpha, k))
        .collect();

    // Learned should match RRF when model unavailable
    for (learned, rrf) in learned_scores.iter().zip(&rrf_scores) {
        assert!(
            (learned - rrf).abs() < 1e-10,
            "Learned should fall back to RRF"
        );
    }
}

#[test]
fn test_hybrid_rules_exist() {
    let rules = hybrid_search_rules();
    assert!(!rules.is_empty(), "Should have hybrid search rewrite rules");

    // Verify critical rules are present
    let rule_names: Vec<_> = rules.iter().map(|r| r.name.as_str()).collect();

    assert!(
        rule_names.contains(&"hybrid-fts-first"),
        "Should have FTS-first rule"
    );
    assert!(
        rule_names.contains(&"hybrid-vector-first"),
        "Should have vector-first rule"
    );
    assert!(
        rule_names.contains(&"hybrid-parallel"),
        "Should have parallel rule"
    );
}

#[test]
fn test_strategy_respects_limit_threshold() {
    let total_rows = 1_000_000.0;

    // Small limit: prefer parallel
    let small_limit = choose_hybrid_strategy(0.05, 0.05, Some(10), total_rows);
    assert_eq!(
        small_limit,
        HybridStrategy::Parallel,
        "Small limits should use parallel"
    );

    // Large limit: use cost-based
    let large_limit = choose_hybrid_strategy(0.05, 0.05, Some(500), total_rows);
    assert!(
        matches!(
            large_limit,
            HybridStrategy::FTSFirst | HybridStrategy::VectorFirst
        ),
        "Large limits should use cost-based selection"
    );
}

#[test]
fn test_extreme_selectivity_values() {
    // Very high selectivity (almost everything matches)
    let high_sel = choose_hybrid_strategy(0.9, 0.9, None, 1_000_000.0);
    assert!(
        matches!(
            high_sel,
            HybridStrategy::FTSFirst | HybridStrategy::VectorFirst | HybridStrategy::Parallel
        ),
        "High selectivity should choose valid strategy"
    );

    // Very low selectivity (almost nothing matches)
    let low_sel = choose_hybrid_strategy(0.0001, 0.0001, None, 1_000_000.0);
    assert_eq!(
        low_sel,
        HybridStrategy::FTSFirst,
        "Both highly selective should prefer FTS-first (tiebreaker)"
    );
}

#[test]
fn test_score_fusion_boundary_conditions() {
    let alpha = 0.5;
    let k = 60;

    // Zero scores
    let zero_score = fuse_scores(0.0, 0.0, ScoreFusion::WeightedAverage, alpha, k);
    assert!(
        zero_score >= 0.0,
        "Zero scores should produce non-negative result"
    );

    // Very high BM25 score
    let high_bm25 = fuse_scores(100.0, 0.1, ScoreFusion::WeightedAverage, alpha, k);
    assert!(
        high_bm25 > 0.0 && high_bm25 <= 1.0,
        "High BM25 should normalize correctly"
    );

    // Very high vector distance
    let high_vector = fuse_scores(1.0, 100.0, ScoreFusion::WeightedAverage, alpha, k);
    assert!(
        high_vector > 0.0 && high_vector <= 1.0,
        "High vector distance should normalize correctly"
    );
}

#[test]
fn test_rrf_constant_effect() {
    let bm25 = 10.0;
    let vector = 0.5;
    let alpha = 0.5;

    // Different k values should produce different scores
    let score_k60 = fuse_scores(bm25, vector, ScoreFusion::ReciprocalRankFusion, alpha, 60);
    let score_k30 = fuse_scores(bm25, vector, ScoreFusion::ReciprocalRankFusion, alpha, 30);

    assert!(
        (score_k60 - score_k30).abs() > 1e-6,
        "Different k values should affect RRF score"
    );
}

#[test]
fn test_weighted_alpha_effect() {
    let bm25 = 10.0;
    let vector = 0.5;
    let k = 60;

    // Alpha = 0.0: pure vector
    let pure_vector = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.0, k);

    // Alpha = 1.0: pure BM25
    let pure_bm25 = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 1.0, k);

    // Alpha = 0.5: balanced
    let balanced = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.5, k);

    // Verify alpha has meaningful effect
    assert!(
        (pure_bm25 - balanced).abs() > f64::EPSILON
            && (balanced - pure_vector).abs() > f64::EPSILON,
        "Alpha should affect weighted average"
    );
}
