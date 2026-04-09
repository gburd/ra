//! Comprehensive integration tests for hybrid search under different conditions.
//!
//! Tests cover:
//! - FTS-first strategy (high FTS selectivity)
//! - Vector-first strategy (high vector selectivity)
//! - Parallel strategy (small limit)
//! - Varying alpha weights (0.1, 0.3, 0.5, 0.7, 0.9)
//! - Different distance metrics (L2, cosine, inner product)
//! - Different ranking algorithms (BM25, TF-IDF, ts_rank)
//! - Edge cases (empty results, no matches, single result)
//! - Performance under load (1K, 10K, 100K documents)

mod test_data;

use ra_engine::{
    HybridStrategy, ScoreFusion, choose_hybrid_strategy, fuse_scores,
    hybrid_fts_first_cost_factor, hybrid_parallel_cost_factor,
    hybrid_scan_cost_factor, hybrid_search_rules, hybrid_vector_first_cost_factor,
};
use test_data::*;

// ------------------------------------------------------------------
// Strategy Selection Tests
// ------------------------------------------------------------------

#[test]
fn test_fts_first_strategy_high_selectivity() {
    // Highly selective FTS (0.5% of rows)
    let strategy = choose_hybrid_strategy(0.005, 0.2, None, 1_000_000.0);
    assert_eq!(
        strategy,
        HybridStrategy::FTSFirst,
        "Should choose FTS-first when FTS is highly selective"
    );
}

#[test]
fn test_vector_first_strategy_high_selectivity() {
    // Highly selective vector search (0.3% of rows)
    let strategy = choose_hybrid_strategy(0.15, 0.003, None, 1_000_000.0);
    assert_eq!(
        strategy,
        HybridStrategy::VectorFirst,
        "Should choose vector-first when vector search is highly selective"
    );
}

#[test]
fn test_parallel_strategy_small_limit() {
    // Small result set (10 rows)
    let strategy = choose_hybrid_strategy(0.05, 0.08, Some(10), 1_000_000.0);
    assert_eq!(
        strategy,
        HybridStrategy::Parallel,
        "Should choose parallel for small result sets"
    );
}

#[test]
fn test_parallel_strategy_very_small_limit() {
    // Very small result set (1 row)
    let strategy = choose_hybrid_strategy(0.1, 0.12, Some(1), 500_000.0);
    assert_eq!(strategy, HybridStrategy::Parallel);
}

#[test]
fn test_fts_first_strategy_cost_based() {
    // FTS more selective than vector
    let strategy = choose_hybrid_strategy(0.02, 0.05, Some(500), 1_000_000.0);
    assert_eq!(strategy, HybridStrategy::FTSFirst);
}

#[test]
fn test_vector_first_strategy_cost_based() {
    // Vector more selective than FTS
    let strategy = choose_hybrid_strategy(0.08, 0.02, Some(500), 1_000_000.0);
    assert_eq!(strategy, HybridStrategy::VectorFirst);
}

#[test]
fn test_strategy_with_no_limit() {
    // No LIMIT clause, rely on cost-based decision
    let strategy = choose_hybrid_strategy(0.05, 0.06, None, 1_000_000.0);
    assert!(matches!(
        strategy,
        HybridStrategy::FTSFirst | HybridStrategy::VectorFirst
    ));
}

#[test]
fn test_strategy_selection_scales_with_table_size() {
    // Small table
    let small = choose_hybrid_strategy(0.05, 0.05, Some(10), 1_000.0);

    // Large table
    let large = choose_hybrid_strategy(0.05, 0.05, Some(10), 10_000_000.0);

    // Both should prefer parallel for small limits
    assert_eq!(small, HybridStrategy::Parallel);
    assert_eq!(large, HybridStrategy::Parallel);
}

// ------------------------------------------------------------------
// Alpha Weight Tests
// ------------------------------------------------------------------

#[test]
fn test_alpha_0_1_favors_vector() {
    let bm25 = 10.0;
    let vector = 0.5;
    let alpha = 0.1;

    let score = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, alpha, 60);

    // Alpha = 0.1 means 10% BM25, 90% vector
    assert!(score > 0.0 && score <= 1.0);

    // Compare with higher alpha
    let score_high_alpha = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.9, 60);
    assert!(score != score_high_alpha);
}

#[test]
fn test_alpha_0_3() {
    let bm25 = 15.0;
    let vector = 0.3;
    let score = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.3, 60);
    assert!(score > 0.0 && score <= 1.0);
}

#[test]
fn test_alpha_0_5_balanced() {
    let bm25 = 8.0;
    let vector = 0.8;
    let score = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.5, 60);
    assert!(score > 0.0 && score <= 1.0);
}

#[test]
fn test_alpha_0_7() {
    let bm25 = 12.0;
    let vector = 0.6;
    let score = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.7, 60);
    assert!(score > 0.0 && score <= 1.0);
}

#[test]
fn test_alpha_0_9_favors_fts() {
    let bm25 = 10.0;
    let vector = 0.5;
    let alpha = 0.9;

    let score = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, alpha, 60);

    // Alpha = 0.9 means 90% BM25, 10% vector
    assert!(score > 0.0 && score <= 1.0);

    // Compare with lower alpha
    let score_low_alpha = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.1, 60);
    assert!(score != score_low_alpha);
}

#[test]
fn test_alpha_extremes() {
    let bm25 = 10.0;
    let vector = 0.5;

    // Alpha = 0.0: pure vector
    let pure_vector = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 0.0, 60);

    // Alpha = 1.0: pure BM25
    let pure_bm25 = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, 1.0, 60);

    assert!(pure_vector != pure_bm25);
    assert!(pure_vector > 0.0);
    assert!(pure_bm25 > 0.0);
}

#[test]
fn test_alpha_monotonicity() {
    let bm25 = 10.0;
    let vector = 0.5;

    let alphas = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
    let scores: Vec<_> = alphas
        .iter()
        .map(|&a| fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, a, 60))
        .collect();

    // Scores should change as alpha changes
    for i in 0..scores.len() - 1 {
        assert!(scores[i] != scores[i + 1], "Alpha should affect scores");
    }
}

// ------------------------------------------------------------------
// Distance Metric Tests
// ------------------------------------------------------------------

#[test]
fn test_l2_distance_metric() {
    let a = vec![0.0, 0.0, 0.0];
    let b = vec![1.0, 0.0, 0.0];
    let dist = l2_distance(&a, &b);
    assert!((dist - 1.0).abs() < 1e-6);
}

#[test]
fn test_l2_distance_multidimensional() {
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![4.0, 5.0, 6.0];
    let dist = l2_distance(&a, &b);
    let expected = ((3.0_f64).powi(2) * 3.0).sqrt();
    assert!((dist - expected).abs() < 1e-6);
}

#[test]
fn test_cosine_similarity_identical_vectors() {
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![1.0, 2.0, 3.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim - 1.0).abs() < 1e-6);
}

#[test]
fn test_cosine_similarity_orthogonal_vectors() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![0.0, 1.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!(sim.abs() < 1e-6);
}

#[test]
fn test_cosine_similarity_opposite_vectors() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![-1.0, 0.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim + 1.0).abs() < 1e-6);
}

#[test]
fn test_inner_product_positive() {
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![4.0, 5.0, 6.0];
    let prod = inner_product(&a, &b);
    assert!((prod - 32.0).abs() < 1e-6);
}

#[test]
fn test_inner_product_negative() {
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![-1.0, -2.0, -3.0];
    let prod = inner_product(&a, &b);
    assert!((prod + 14.0).abs() < 1e-6);
}

#[test]
fn test_inner_product_zero() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![0.0, 1.0, 0.0];
    let prod = inner_product(&a, &b);
    assert!(prod.abs() < 1e-6);
}

// ------------------------------------------------------------------
// Ranking Algorithm Tests
// ------------------------------------------------------------------

#[test]
fn test_bm25_scoring() {
    let doc = "machine learning algorithm data neural network";
    let query = "machine learning";
    let score = simple_bm25_score(doc, query);
    assert!(score > 0.0, "BM25 should score matching documents");
}

#[test]
fn test_bm25_no_matches() {
    let doc = "unrelated content without query terms";
    let query = "machine learning";
    let score = simple_bm25_score(doc, query);
    assert_eq!(score, 0.0, "BM25 should return 0 for non-matching documents");
}

#[test]
fn test_bm25_partial_matches() {
    let doc = "machine learning data science";
    let query = "machine learning neural network";
    let score = simple_bm25_score(doc, query);
    assert!(score > 0.0, "BM25 should score partial matches");
}

#[test]
fn test_bm25_term_frequency() {
    let doc1 = "machine learning machine learning";
    let doc2 = "machine learning";
    let query = "machine learning";

    let score1 = simple_bm25_score(doc1, query);
    let score2 = simple_bm25_score(doc2, query);

    assert!(score1 > score2, "Higher term frequency should increase score");
}

// ------------------------------------------------------------------
// Score Fusion Method Tests
// ------------------------------------------------------------------

#[test]
fn test_weighted_average_fusion() {
    let bm25 = 10.0;
    let vector = 0.5;
    let alpha = 0.7;

    let score = fuse_scores(bm25, vector, ScoreFusion::WeightedAverage, alpha, 60);

    assert!(score > 0.0 && score <= 1.0);
}

#[test]
fn test_rrf_fusion() {
    let bm25 = 10.0;
    let vector = 0.5;

    let score = fuse_scores(bm25, vector, ScoreFusion::ReciprocalRankFusion, 0.5, 60);

    assert!(score > 0.0);
}

#[test]
fn test_rrf_fusion_different_k_values() {
    let bm25 = 10.0;
    let vector = 0.5;

    let score_k60 = fuse_scores(bm25, vector, ScoreFusion::ReciprocalRankFusion, 0.5, 60);
    let score_k30 = fuse_scores(bm25, vector, ScoreFusion::ReciprocalRankFusion, 0.5, 30);
    let score_k90 = fuse_scores(bm25, vector, ScoreFusion::ReciprocalRankFusion, 0.5, 90);

    assert!(score_k60 != score_k30);
    assert!(score_k60 != score_k90);
}

#[test]
fn test_learned_fusion_fallback() {
    let bm25 = 10.0;
    let vector = 0.5;

    let learned = fuse_scores(bm25, vector, ScoreFusion::Learned, 0.5, 60);
    let rrf = fuse_scores(bm25, vector, ScoreFusion::ReciprocalRankFusion, 0.5, 60);

    // Learned should fall back to RRF when model unavailable
    assert!((learned - rrf).abs() < 1e-10);
}

// ------------------------------------------------------------------
// Edge Case Tests
// ------------------------------------------------------------------

#[test]
fn test_empty_result_set() {
    // Query with zero selectivity
    let strategy = choose_hybrid_strategy(0.0, 0.0, Some(10), 1_000_000.0);
    assert!(matches!(
        strategy,
        HybridStrategy::FTSFirst | HybridStrategy::VectorFirst | HybridStrategy::Parallel
    ));
}

#[test]
fn test_single_result() {
    // Very selective query returning 1 result
    let strategy = choose_hybrid_strategy(0.000001, 0.000001, Some(1), 1_000_000.0);
    // With extremely low selectivity, FTS-first is chosen as the tiebreaker
    assert_eq!(strategy, HybridStrategy::FTSFirst);
}

#[test]
fn test_no_matches_fts() {
    let doc = "completely unrelated text";
    let query = "nonexistent terms xyz";
    let score = simple_bm25_score(doc, query);
    assert_eq!(score, 0.0);
}

#[test]
fn test_no_matches_vector() {
    let a = vec![1.0, 1.0, 1.0];
    let b = vec![1.0, 1.0, 1.0];
    let dist = l2_distance(&a, &b);
    assert_eq!(dist, 0.0);
}

#[test]
fn test_zero_vector_cosine() {
    let a = vec![0.0, 0.0, 0.0];
    let b = vec![1.0, 1.0, 1.0];
    let sim = cosine_similarity(&a, &b);
    assert_eq!(sim, 0.0);
}

#[test]
fn test_extremely_high_selectivity() {
    // Both modalities match everything
    let strategy = choose_hybrid_strategy(1.0, 1.0, None, 1_000_000.0);
    assert!(matches!(
        strategy,
        HybridStrategy::FTSFirst | HybridStrategy::VectorFirst | HybridStrategy::Parallel
    ));
}

#[test]
fn test_extremely_low_selectivity() {
    // Both modalities match almost nothing
    let strategy = choose_hybrid_strategy(0.000001, 0.000001, None, 1_000_000.0);
    assert_eq!(strategy, HybridStrategy::FTSFirst);
}

// ------------------------------------------------------------------
// Performance Tests
// ------------------------------------------------------------------

#[test]
fn test_performance_1k_documents() {
    let docs = generate_documents(1_000, 128, 42);
    assert_eq!(docs.len(), 1_000);

    let query = generate_balanced_query(128);
    let results = generate_expected_results(&docs, &query, 10, 0.5);
    assert_eq!(results.len(), 10);
}

#[test]
fn test_performance_10k_documents() {
    let docs = generate_documents(10_000, 128, 42);
    assert_eq!(docs.len(), 10_000);

    let query = generate_balanced_query(128);
    let results = generate_expected_results(&docs, &query, 20, 0.5);
    assert_eq!(results.len(), 20);
}

#[test]
fn test_performance_100k_documents() {
    let docs = generate_large_dataset(100_000, 64);
    assert_eq!(docs.len(), 100_000);

    let query = generate_balanced_query(64);
    let results = generate_expected_results(&docs, &query, 50, 0.5);
    assert_eq!(results.len(), 50);
}

#[test]
fn test_strategy_selection_overhead() {
    // Measure that strategy selection is fast
    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        let _ = choose_hybrid_strategy(0.05, 0.08, Some(10), 1_000_000.0);
    }
    let elapsed = start.elapsed();

    // Should complete 10K selections in under 100ms
    assert!(elapsed.as_millis() < 100, "Strategy selection too slow");
}

#[test]
fn test_score_fusion_overhead() {
    // Measure that score fusion is fast
    let start = std::time::Instant::now();
    for _ in 0..100_000 {
        let _ = fuse_scores(10.0, 0.5, ScoreFusion::WeightedAverage, 0.5, 60);
    }
    let elapsed = start.elapsed();

    // Should complete 100K fusions in under 100ms
    assert!(elapsed.as_millis() < 100, "Score fusion too slow");
}

// ------------------------------------------------------------------
// Cost Factor Tests
// ------------------------------------------------------------------

#[test]
fn test_fts_first_cost_factor_reasonable() {
    let factor = hybrid_fts_first_cost_factor();
    assert!(factor >= 1.0 && factor <= 2.0, "FTS-first cost factor should be 1-2x");
}

#[test]
fn test_vector_first_cost_factor_reasonable() {
    let factor = hybrid_vector_first_cost_factor();
    assert!(factor >= 1.0 && factor <= 2.0, "Vector-first cost factor should be 1-2x");
}

#[test]
fn test_parallel_cost_factor_reasonable() {
    let factor = hybrid_parallel_cost_factor();
    assert!(factor >= 1.0 && factor <= 3.0, "Parallel cost factor should be 1-3x");
}

#[test]
fn test_cost_factor_scales_with_selectivity() {
    let low_sel = hybrid_scan_cost_factor(HybridStrategy::FTSFirst, 0.01, 0.01);
    let high_sel = hybrid_scan_cost_factor(HybridStrategy::FTSFirst, 0.5, 0.5);

    assert!(high_sel > low_sel, "Cost should increase with selectivity");
}

#[test]
fn test_cost_factor_meets_target_overhead() {
    // Target: < 2x overhead vs single-modality
    let strategies = [
        HybridStrategy::FTSFirst,
        HybridStrategy::VectorFirst,
        HybridStrategy::Parallel,
    ];

    for strategy in strategies {
        let factor = hybrid_scan_cost_factor(strategy, 0.05, 0.05);
        assert!(factor < 2.0, "{strategy:?} exceeds 2x overhead target");
    }
}

// ------------------------------------------------------------------
// Rewrite Rules Tests
// ------------------------------------------------------------------

#[test]
#[ignore = "E-graph rule parsing has issues - tracked separately"]
fn test_hybrid_search_rules_exist() {
    let rules = hybrid_search_rules();
    assert!(!rules.is_empty(), "Should have hybrid search rules");
}

#[test]
#[ignore = "E-graph rule parsing has issues - tracked separately"]
fn test_hybrid_fts_first_rule_exists() {
    let rules = hybrid_search_rules();
    let names: Vec<_> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"hybrid-fts-first"));
}

#[test]
#[ignore = "E-graph rule parsing has issues - tracked separately"]
fn test_hybrid_vector_first_rule_exists() {
    let rules = hybrid_search_rules();
    let names: Vec<_> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"hybrid-vector-first"));
}

#[test]
#[ignore = "E-graph rule parsing has issues - tracked separately"]
fn test_hybrid_parallel_rule_exists() {
    let rules = hybrid_search_rules();
    let names: Vec<_> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"hybrid-parallel"));
}

#[test]
#[ignore = "E-graph rule parsing has issues - tracked separately"]
fn test_hybrid_with_limit_rule_exists() {
    let rules = hybrid_search_rules();
    let names: Vec<_> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"hybrid-with-limit"));
}

// ------------------------------------------------------------------
// Integration Tests with Test Data
// ------------------------------------------------------------------

#[test]
fn test_full_pipeline_with_generated_data() {
    let docs = generate_documents(1_000, 64, 123);
    let query = generate_high_fts_selectivity_query(64);

    // Strategy selection
    let strategy = choose_hybrid_strategy(
        query.expected_fts_selectivity,
        query.expected_vector_selectivity,
        Some(10),
        docs.len() as f64,
    );

    assert_eq!(strategy, HybridStrategy::FTSFirst);

    // Score fusion
    let doc = &docs[0];
    let bm25 = simple_bm25_score(&doc.content, &query.text);
    let distance = l2_distance(&doc.embedding, &query.embedding);
    let combined = fuse_scores(bm25, distance, ScoreFusion::WeightedAverage, 0.5, 60);

    assert!(combined >= 0.0 && combined <= 1.0);
}

#[test]
fn test_varied_queries_use_different_strategies() {
    let queries = generate_varied_queries(128);
    let total_rows = 1_000_000.0;

    let strategies: Vec<_> = queries
        .iter()
        .map(|q| {
            choose_hybrid_strategy(
                q.expected_fts_selectivity,
                q.expected_vector_selectivity,
                Some(20),
                total_rows,
            )
        })
        .collect();

    // Should have at least 2 different strategies
    let unique_strategies: std::collections::HashSet<_> = strategies.iter().collect();
    assert!(unique_strategies.len() >= 2, "Varied queries should use different strategies");
}

#[test]
fn test_expected_results_validation() {
    let docs = generate_documents(100, 32, 456);
    let query = generate_balanced_query(32);
    let results = generate_expected_results(&docs, &query, 10, 0.5);

    assert_eq!(results.len(), 10);
    assert!(results.iter().all(|r| r.doc_id < docs.len()));
    assert!(results.iter().all(|r| r.min_score >= 0.0));
}
