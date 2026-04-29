//! Hybrid search optimization combining full-text search (FTS) and vector similarity.
//!
//! This module implements cost-based strategy selection and score fusion
//! for queries that combine BM25-based text search (via `PostgreSQL` RUM indexes)
//! with vector similarity search (via pgvector). The goal is to achieve
//! near-optimal performance (< 2x overhead vs single-modality search) while
//! delivering 2-5x improvement over naive approaches.
//!
//! # Strategy Selection
//!
//! Three execution strategies are supported:
//!
//! 1. **FTS-First**: Execute text search first, filter by vector similarity
//!    - Best when FTS is highly selective (< 1% of rows)
//!    - Avoids computing vector distances for most rows
//!
//! 2. **Vector-First**: Execute vector search first, filter by FTS match
//!    - Best when vector search is highly selective (< 1% of rows)
//!    - Avoids computing BM25 scores for most rows
//!
//! 3. **Parallel**: Execute both modalities independently, merge results
//!    - Best when both modalities have similar selectivity
//!    - Best when result set is small (< 100 rows)
//!    - Requires merging and deduplication
//!
//! # Score Fusion Methods
//!
//! Three fusion methods combine BM25 and vector similarity scores:
//!
//! 1. **Weighted Average**: `alpha * bm25 + (1 - alpha) * vector`
//!    - Simple, predictable, requires score normalization
//!    - Alpha defaults to 0.5 but can be tuned per-query
//!
//! 2. **Reciprocal Rank Fusion (RRF)**: `1 / (k + rank)`
//!    - Rank-based, no score normalization needed
//!    - k constant typically 60 (empirically validated)
//!    - More robust to score distribution differences
//!
//! 3. **Learned Fusion**: ML model trained on labeled data
//!    - Best accuracy but requires training data
//!    - Falls back to RRF when model unavailable
//!
//! # Cost Model
//!
//! Strategy selection uses a cost model based on:
//! - Selectivity estimates from table statistics
//! - Index scan costs (RUM for FTS, HNSW/IVFFlat for vectors)
//! - Merge/deduplication overhead for parallel execution
//! - Result set size (LIMIT clause)
//!
//! See: RFC 0073 Hybrid Search Optimization

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

// ------------------------------------------------------------------
// Hybrid search strategy
// ------------------------------------------------------------------

/// Execution strategy for hybrid FTS + vector search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HybridStrategy {
    /// Execute FTS first, filter candidates by vector similarity.
    /// Best when FTS is highly selective (< 1% selectivity).
    FTSFirst,
    /// Execute vector search first, filter candidates by FTS match.
    /// Best when vector search is highly selective (< 1% selectivity).
    VectorFirst,
    /// Execute both modalities independently, merge results.
    /// Best when both have similar selectivity or result set is small.
    Parallel,
}

impl HybridStrategy {
    /// Human-readable label for this strategy.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FTSFirst => "fts_first",
            Self::VectorFirst => "vector_first",
            Self::Parallel => "parallel",
        }
    }
}

impl std::fmt::Display for HybridStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ------------------------------------------------------------------
// Score fusion method
// ------------------------------------------------------------------

/// Method for combining BM25 and vector similarity scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScoreFusion {
    /// Weighted average: `alpha * bm25 + (1 - alpha) * vector`.
    /// Requires score normalization to [0, 1] range.
    WeightedAverage,
    /// Reciprocal Rank Fusion: `1 / (k + rank)`.
    /// Rank-based fusion, robust to score distribution differences.
    ReciprocalRankFusion,
    /// Learned fusion using ML model trained on labeled data.
    /// Falls back to RRF when model unavailable.
    Learned,
}

impl ScoreFusion {
    /// Human-readable label for this fusion method.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::WeightedAverage => "weighted_average",
            Self::ReciprocalRankFusion => "reciprocal_rank_fusion",
            Self::Learned => "learned",
        }
    }
}

impl std::fmt::Display for ScoreFusion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ------------------------------------------------------------------
// Strategy selection
// ------------------------------------------------------------------

/// Selectivity threshold for choosing FTS-first or vector-first strategy.
const HIGH_SELECTIVITY_THRESHOLD: f64 = 0.01;

/// Result set size threshold for preferring parallel execution.
const SMALL_RESULT_THRESHOLD: usize = 100;

/// Default RRF k constant (empirically validated).
#[cfg(test)]
const DEFAULT_RRF_K: usize = 60;

/// Default alpha for weighted average fusion.
#[cfg(test)]
const DEFAULT_ALPHA: f64 = 0.5;

/// Choose the optimal hybrid search strategy based on selectivity estimates
/// and query characteristics.
///
/// # Arguments
///
/// * `fts_selectivity` - Estimated fraction of rows matching FTS predicate
/// * `vector_selectivity` - Estimated fraction of rows matching vector predicate
/// * `limit` - Optional result set size limit from LIMIT clause
/// * `total_rows` - Total number of rows in the table
///
/// # Returns
///
/// The recommended `HybridStrategy` based on cost estimates.
///
/// # Strategy Selection Logic
///
/// 1. If FTS is highly selective (< 1%): FTS-First
/// 2. If vector search is highly selective (< 1%): Vector-First
/// 3. If result set is small (< 100 rows): Parallel
/// 4. Otherwise: cost-based decision
#[must_use]
pub fn choose_hybrid_strategy(
    fts_selectivity: f64,
    vector_selectivity: f64,
    limit: Option<usize>,
    total_rows: f64,
) -> HybridStrategy {
    // High selectivity: use the most selective modality first
    if fts_selectivity < HIGH_SELECTIVITY_THRESHOLD {
        return HybridStrategy::FTSFirst;
    }
    if vector_selectivity < HIGH_SELECTIVITY_THRESHOLD {
        return HybridStrategy::VectorFirst;
    }

    // Small result set: parallel execution with merge
    if let Some(lim) = limit {
        if lim < SMALL_RESULT_THRESHOLD {
            return HybridStrategy::Parallel;
        }
    }

    // Cost-based decision for remaining cases
    let fts_cost = estimate_fts_cost(fts_selectivity, total_rows);
    let vector_cost = estimate_vector_cost(vector_selectivity, total_rows);

    // Estimate cost for each strategy
    let fts_first_cost = fts_cost + vector_cost * fts_selectivity;
    let vector_first_cost = vector_cost + fts_cost * vector_selectivity;
    let parallel_cost = fts_cost
        + vector_cost
        + estimate_merge_cost(fts_selectivity, vector_selectivity, total_rows);

    // Choose strategy with minimum cost
    if fts_first_cost <= vector_first_cost && fts_first_cost <= parallel_cost {
        HybridStrategy::FTSFirst
    } else if vector_first_cost <= parallel_cost {
        HybridStrategy::VectorFirst
    } else {
        HybridStrategy::Parallel
    }
}

/// Estimate cost of FTS scan using RUM index.
///
/// Cost model:
/// - RUM index scan: O(M log N) where M = matches, N = total rows
/// - BM25 scoring: O(M * `avg_term_frequency`)
///
/// Simplified to: `base_cost + selectivity * total_rows * log(total_rows)`
fn estimate_fts_cost(selectivity: f64, total_rows: f64) -> f64 {
    const FTS_BASE_COST: f64 = 10.0;
    const FTS_PER_MATCH_COST: f64 = 0.5;

    let matches = selectivity * total_rows;
    FTS_BASE_COST + matches * FTS_PER_MATCH_COST * total_rows.log2()
}

/// Estimate cost of vector similarity scan using pgvector.
///
/// Cost model:
/// - HNSW index: O(M * log N) where M = matches
/// - `IVFFlat` index: O(M * sqrt(N))
/// - Distance computation: O(M * dimensions)
///
/// Simplified to: `base_cost + selectivity * total_rows * log(total_rows) * dim_factor`
fn estimate_vector_cost(selectivity: f64, total_rows: f64) -> f64 {
    const VECTOR_BASE_COST: f64 = 15.0;
    const VECTOR_PER_MATCH_COST: f64 = 1.0;
    const DIM_FACTOR: f64 = 1.2; // Accounts for high-dimensional distance computation

    let matches = selectivity * total_rows;
    VECTOR_BASE_COST + matches * VECTOR_PER_MATCH_COST * total_rows.log2() * DIM_FACTOR
}

/// Estimate cost of merging results from parallel FTS and vector scans.
///
/// Cost model:
/// - Deduplication: O(M1 + M2) using hash set
/// - Score fusion: O(M1 + M2)
/// - Sorting: O((M1 + M2) * log(M1 + M2))
fn estimate_merge_cost(fts_selectivity: f64, vector_selectivity: f64, total_rows: f64) -> f64 {
    const MERGE_BASE_COST: f64 = 5.0;
    const MERGE_PER_ROW_COST: f64 = 0.1;

    let fts_matches = fts_selectivity * total_rows;
    let vector_matches = vector_selectivity * total_rows;
    let total_matches = fts_matches + vector_matches;

    MERGE_BASE_COST + total_matches * MERGE_PER_ROW_COST * total_matches.log2()
}

// ------------------------------------------------------------------
// Score fusion
// ------------------------------------------------------------------

/// Fuse BM25 and vector similarity scores using the specified method.
///
/// # Arguments
///
/// * `bm25_score` - BM25 relevance score (unnormalized, typically 0-20)
/// * `vector_score` - Vector similarity score (distance metric, lower is better)
/// * `method` - Score fusion method
/// * `alpha` - Weight for weighted average (ignored for other methods)
/// * `k` - RRF constant (ignored for other methods)
///
/// # Returns
///
/// Combined score where higher values indicate better matches.
///
/// # Score Normalization
///
/// - BM25 scores are normalized using `score / (score + 1)` to map to [0, 1]
/// - Vector distances are inverted using `1 / (1 + distance)` to map to [0, 1]
#[must_use]
pub fn fuse_scores(
    bm25_score: f64,
    vector_score: f64,
    method: ScoreFusion,
    alpha: f64,
    k: usize,
) -> f64 {
    match method {
        ScoreFusion::WeightedAverage => {
            let norm_bm25 = normalize_bm25(bm25_score);
            let norm_vector = normalize_vector_distance(vector_score);
            alpha * norm_bm25 + (1.0 - alpha) * norm_vector
        }
        ScoreFusion::ReciprocalRankFusion => {
            // RRF operates on ranks, but we approximate with scores
            // In practice, ranks would be computed after retrieval
            let bm25_rrf = 1.0 / (k as f64 + bm25_score);
            let vector_rrf = 1.0 / (k as f64 + vector_score);
            bm25_rrf + vector_rrf
        }
        ScoreFusion::Learned => {
            // Placeholder: fall back to RRF when model unavailable
            // In production, this would call an ML model with features:
            // [bm25_score, vector_score, doc_length, query_terms, etc.]
            let bm25_rrf = 1.0 / (k as f64 + bm25_score);
            let vector_rrf = 1.0 / (k as f64 + vector_score);
            bm25_rrf + vector_rrf
        }
    }
}

/// Normalize BM25 score to [0, 1] range using `score / (score + 1)`.
fn normalize_bm25(score: f64) -> f64 {
    score / (score + 1.0)
}

/// Normalize vector distance to [0, 1] similarity using `1 / (1 + distance)`.
fn normalize_vector_distance(distance: f64) -> f64 {
    1.0 / (1.0 + distance)
}

// ------------------------------------------------------------------
// E-graph rewrite rules
// ------------------------------------------------------------------

/// Hybrid search rewrite rules for e-graph optimization.
///
/// These rules recognize patterns where FTS and vector predicates are
/// combined, and rewrite them to use efficient hybrid scan operators.
///
/// # Rules
///
/// 1. **FTS-first pattern**: `filter(fts_match, sort(vector_distance))`
///    → `hybrid_search_scan(strategy=FTSFirst)`
///
/// 2. **Vector-first pattern**: `filter(vector_distance, sort(fts_rank))`
///    → `hybrid_search_scan(strategy=VectorFirst)`
///
/// 3. **Parallel pattern**: `sort(hybrid_score, filter(fts AND vector))`
///    → `hybrid_search_scan(strategy=Parallel)`
///
/// # Example
///
/// ```sql
/// SELECT * FROM docs
/// WHERE content_tsvector @@ 'search & query'::tsquery
/// ORDER BY content_embedding <-> '[0.1, 0.2, ...]'::vector
/// LIMIT 10;
/// ```
///
/// This query combines FTS filtering with vector distance ordering.
/// The optimizer recognizes this pattern and rewrites it to use
/// `hybrid_search_scan` with strategy determined by selectivity estimates.
#[must_use]
pub fn hybrid_search_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Rule 1: FTS-first hybrid scan.
        //
        // Pattern: An FTS match filter applied to a vector-distance
        // ordered scan with a LIMIT.  The FTS predicate is highly
        // selective, so execute FTS first, then rank remaining rows
        // by vector distance.
        //
        // Before:
        //   LIMIT k (SORT vector-distance
        //     (FILTER fts-match (SCAN table)))
        //
        // After:
        //   hybrid-scan(table, fts_match, vector_distance,
        //               strategy=fts_first, k, limit)
        rewrite!("hybrid-fts-first";
            "(limit ?k ?offset
               (sort (list (sort-key
                   (vector-distance ?metric ?vcol ?target)
                   ?order ?nulls))
                 (filter (fts-match ?vendor ?fcols ?fquery ?mode)
                   (scan ?table))))" =>
            "(hybrid-scan ?table
               (fts-match ?vendor ?fcols ?fquery ?mode)
               (vector-distance ?metric ?vcol ?target)
               fts_first ?k ?k)"
        ),
        // Rule 2: Vector-first hybrid scan.
        //
        // Pattern: An FTS match filter on top of a vector KNN scan.
        // The vector search is highly selective (small k), so run
        // vector KNN first, then filter by FTS match.
        //
        // Before:
        //   FILTER fts-match (LIMIT k (vector-knn ...))
        //
        // After:
        //   hybrid-scan(table, fts_match, vector_knn,
        //               strategy=vector_first, k, k)
        rewrite!("hybrid-vector-first";
            "(filter (fts-match ?vendor ?fcols ?fquery ?mode)
               (limit ?k ?offset
                 (vector-knn ?table ?vcol ?target ?vk)))" =>
            "(hybrid-scan ?table
               (fts-match ?vendor ?fcols ?fquery ?mode)
               (vector-distance l2 ?vcol ?target)
               vector_first ?k ?k)"
        ),
        // Rule 3: Parallel hybrid scan.
        //
        // Pattern: A conjunctive filter combining an FTS match and
        // a vector distance threshold, sorted by hybrid-score.
        // Both modalities have comparable selectivity, so run them
        // in parallel and merge results.
        //
        // Before:
        //   LIMIT k (SORT hybrid-score
        //     (FILTER (fts-match AND vector-distance < thresh)
        //       (SCAN table)))
        //
        // After:
        //   hybrid-scan(table, fts_match, vector_distance,
        //               strategy=parallel, k, limit)
        rewrite!("hybrid-parallel";
            "(limit ?k ?offset
               (sort (list (sort-key
                   (hybrid-score
                     (fts-rank ?fcol ?fquery ?algo)
                     (vector-distance ?metric ?vcol ?target)
                     ?alpha ?beta ?method)
                   ?order ?nulls))
                 (filter
                   (and
                     (fts-match ?vendor ?fcols ?fquery ?mode)
                     (lt (vector-distance ?metric2 ?vcol2 ?target2)
                         ?threshold))
                   (scan ?table))))" =>
            "(hybrid-scan ?table
               (fts-match ?vendor ?fcols ?fquery ?mode)
               (vector-distance ?metric ?vcol ?target)
               parallel ?k ?k)"
        ),
    ]
}

// ------------------------------------------------------------------
// Cost factors for hybrid scans
// ------------------------------------------------------------------

/// Cost factor for FTS-first hybrid scan relative to sequential scan.
///
/// Used by the cost model to estimate total query cost.
#[must_use]
pub fn hybrid_fts_first_cost_factor() -> f64 {
    1.2 // 20% overhead vs pure FTS due to vector filtering
}

/// Cost factor for vector-first hybrid scan relative to sequential scan.
#[must_use]
pub fn hybrid_vector_first_cost_factor() -> f64 {
    1.3 // 30% overhead vs pure vector due to FTS filtering
}

/// Cost factor for parallel hybrid scan relative to sequential scan.
#[must_use]
pub fn hybrid_parallel_cost_factor() -> f64 {
    1.5 // 50% overhead due to merge/deduplication
}

/// Estimate cost factor for hybrid scan based on strategy and selectivities.
///
/// Returns a multiplier relative to sequential scan cost.
#[must_use]
pub fn hybrid_scan_cost_factor(
    strategy: HybridStrategy,
    fts_selectivity: f64,
    vector_selectivity: f64,
) -> f64 {
    match strategy {
        HybridStrategy::FTSFirst => {
            // Cost = FTS scan + vector filtering on FTS results
            let base = hybrid_fts_first_cost_factor();
            base * (1.0 + fts_selectivity * 0.5)
        }
        HybridStrategy::VectorFirst => {
            // Cost = vector scan + FTS filtering on vector results
            let base = hybrid_vector_first_cost_factor();
            base * (1.0 + vector_selectivity * 0.5)
        }
        HybridStrategy::Parallel => {
            // Cost = both scans + merge overhead
            let base = hybrid_parallel_cost_factor();
            let overlap = fts_selectivity * vector_selectivity;
            base * (1.0 + overlap * 0.3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_selection_fts_first() {
        // Highly selective FTS (0.5%)
        let strategy = choose_hybrid_strategy(0.005, 0.2, None, 1_000_000.0);
        assert_eq!(strategy, HybridStrategy::FTSFirst);
    }

    #[test]
    fn test_strategy_selection_vector_first() {
        // Highly selective vector search (0.3%)
        let strategy = choose_hybrid_strategy(0.15, 0.003, None, 1_000_000.0);
        assert_eq!(strategy, HybridStrategy::VectorFirst);
    }

    #[test]
    fn test_strategy_selection_parallel_small_limit() {
        // Small result set (10 rows)
        let strategy = choose_hybrid_strategy(0.05, 0.08, Some(10), 1_000_000.0);
        assert_eq!(strategy, HybridStrategy::Parallel);
    }

    #[test]
    fn test_strategy_selection_cost_based() {
        // Similar selectivities, large result set
        let strategy = choose_hybrid_strategy(0.05, 0.06, Some(500), 1_000_000.0);
        // Should choose based on cost estimates
        assert!(matches!(
            strategy,
            HybridStrategy::FTSFirst | HybridStrategy::VectorFirst
        ));
    }

    #[test]
    fn test_fts_cost_increases_with_selectivity() {
        let cost_low = estimate_fts_cost(0.01, 1_000_000.0);
        let cost_high = estimate_fts_cost(0.1, 1_000_000.0);
        assert!(cost_high > cost_low);
    }

    #[test]
    fn test_vector_cost_increases_with_selectivity() {
        let cost_low = estimate_vector_cost(0.01, 1_000_000.0);
        let cost_high = estimate_vector_cost(0.1, 1_000_000.0);
        assert!(cost_high > cost_low);
    }

    #[test]
    fn test_merge_cost_increases_with_matches() {
        let cost_low = estimate_merge_cost(0.01, 0.01, 1_000_000.0);
        let cost_high = estimate_merge_cost(0.1, 0.1, 1_000_000.0);
        assert!(cost_high > cost_low);
    }

    #[test]
    fn test_weighted_average_fusion() {
        let bm25 = 10.0;
        let vector = 0.5;
        let alpha = 0.7;

        let score = fuse_scores(
            bm25,
            vector,
            ScoreFusion::WeightedAverage,
            alpha,
            DEFAULT_RRF_K,
        );

        // Should be weighted combination of normalized scores
        assert!(score > 0.0 && score <= 1.0);
    }

    #[test]
    fn test_rrf_fusion() {
        let bm25 = 10.0;
        let vector = 0.5;

        let score = fuse_scores(
            bm25,
            vector,
            ScoreFusion::ReciprocalRankFusion,
            DEFAULT_ALPHA,
            DEFAULT_RRF_K,
        );

        // RRF score should be positive
        assert!(score > 0.0);
    }

    #[test]
    fn test_normalize_bm25() {
        assert!((normalize_bm25(0.0) - 0.0).abs() < 1e-6);
        assert!((normalize_bm25(1.0) - 0.5).abs() < 1e-6);
        assert!((normalize_bm25(10.0) - 10.0 / 11.0).abs() < 1e-6);
    }

    #[test]
    fn test_normalize_vector_distance() {
        assert!((normalize_vector_distance(0.0) - 1.0).abs() < 1e-6);
        assert!((normalize_vector_distance(1.0) - 0.5).abs() < 1e-6);
        assert!((normalize_vector_distance(10.0) - 1.0 / 11.0).abs() < 1e-6);
    }

    #[test]
    fn test_hybrid_fts_first_cost_factor() {
        let factor = hybrid_fts_first_cost_factor();
        assert!(factor > 1.0 && factor < 2.0);
    }

    #[test]
    fn test_hybrid_scan_cost_factor_scales() {
        let low_sel = hybrid_scan_cost_factor(HybridStrategy::FTSFirst, 0.01, 0.01);
        let high_sel = hybrid_scan_cost_factor(HybridStrategy::FTSFirst, 0.1, 0.1);
        assert!(high_sel > low_sel);
    }

    #[test]
    fn test_strategy_labels() {
        assert_eq!(HybridStrategy::FTSFirst.label(), "fts_first");
        assert_eq!(HybridStrategy::VectorFirst.label(), "vector_first");
        assert_eq!(HybridStrategy::Parallel.label(), "parallel");
    }

    #[test]
    fn test_fusion_labels() {
        assert_eq!(ScoreFusion::WeightedAverage.label(), "weighted_average");
        assert_eq!(
            ScoreFusion::ReciprocalRankFusion.label(),
            "reciprocal_rank_fusion"
        );
        assert_eq!(ScoreFusion::Learned.label(), "learned");
    }

    #[test]
    fn test_hybrid_rules_exist() {
        let rules = hybrid_search_rules();
        assert!(!rules.is_empty());

        // Verify rule names
        let rule_names: Vec<_> = rules.iter().map(|r| r.name.as_str()).collect();
        assert!(rule_names.contains(&"hybrid-fts-first"));
        assert!(rule_names.contains(&"hybrid-vector-first"));
        assert!(rule_names.contains(&"hybrid-parallel"));
    }
}
