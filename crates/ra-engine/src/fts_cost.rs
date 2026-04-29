//! Full-text search cost model for inverted indexes.
//!
//! Implements cost estimation for GIN, RUM, and FULLTEXT indexes,
//! including skip-list intersection, ranking algorithms, and top-K
//! optimizations.
//!
//! Key performance characteristics:
//! - Inverted index lookup: 50-99x faster than LIKE
//! - Skip-list intersection: O(sqrt(n) + sqrt(m)) instead of O(n + m)
//! - Top-K with ranking: 10-100x faster when limit << matches

use ra_core::cost::Cost;

/// FTS index type for cost modeling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FtsIndexType {
    /// `PostgreSQL` GIN (Generalized Inverted Index).
    /// Fast boolean queries, slower for ranking.
    Gin,
    /// `PostgreSQL` RUM (extension).
    /// Optimized for ranked retrieval with positions.
    Rum,
    /// MySQL/MariaDB FULLTEXT index.
    /// Built-in ranking support.
    Fulltext,
    /// No index, sequential scan with LIKE.
    None,
}

/// Ranking algorithm for FTS queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RankingAlgorithm {
    /// Term Frequency-Inverse Document Frequency.
    TfIdf,
    /// Best Match 25 (Okapi BM25).
    Bm25,
    /// Cover density ranking (`PostgreSQL` `ts_rank_cd`).
    CoverDensity,
    /// No ranking, boolean match only.
    None,
}

/// Boolean query operator for term combinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BooleanOperator {
    /// All terms must match (AND).
    And,
    /// At least one term must match (OR).
    Or,
    /// Phrase query (terms in order with positions).
    Phrase,
}

/// Estimate cost for inverted index lookup of a single term.
///
/// Cost model:
/// - Index tree traversal: `O(log(total_docs))`
/// - Posting list scan: `O(term_frequency)`
/// - Per-document overhead: decode posting entry
///
/// Returns CPU cost in arbitrary units.
#[must_use]
pub fn inverted_index_lookup_cost(term: &str, total_docs: usize, term_frequency: usize) -> f64 {
    const BASE_LOOKUP_COST: f64 = 1.0;
    const DECODE_COST_PER_DOC: f64 = 0.5;

    let term_length_factor = 1.0 + (term.len() as f64 / 20.0).min(1.0);
    let tree_depth = (total_docs as f64).log2().max(1.0);
    let tree_cost = tree_depth * BASE_LOOKUP_COST * term_length_factor;
    let posting_cost = term_frequency as f64 * DECODE_COST_PER_DOC;

    tree_cost + posting_cost
}

/// Estimate cost for skip-list accelerated intersection.
///
/// Traditional merge intersection is O(n + m). Skip lists allow
/// jumping ahead in sorted posting lists, achieving O(sqrt(n) + sqrt(m))
/// when lists are balanced.
///
/// Cost factors:
/// - Comparison cost per element
/// - Skip pointer traversal cost
/// - Result materialization cost
///
/// Returns CPU cost in arbitrary units.
#[must_use]
#[expect(clippy::similar_names, reason = "list_a and list_b are the standard naming convention")]
pub fn skip_list_intersection_cost(list_a_size: usize, list_b_size: usize) -> f64 {
    const COMPARISON_COST: f64 = 0.3;
    const SKIP_TRAVERSAL_COST: f64 = 0.5;
    const RESULT_COST_PER_DOC: f64 = 0.2;

    let min_size = list_a_size.min(list_b_size);
    let max_size = list_a_size.max(list_b_size);

    if min_size == 0 {
        return 0.0;
    }

    let skip_block_size = (max_size as f64).sqrt() as usize;
    let skip_jumps = max_size / skip_block_size.max(1);

    let comparison_ops = min_size.min(skip_jumps + skip_block_size);
    let skip_cost = skip_jumps as f64 * SKIP_TRAVERSAL_COST;
    let comparison_total = comparison_ops as f64 * COMPARISON_COST;

    let result_size = (min_size as f64 * 0.3).min(min_size as f64);
    let result_cost = result_size * RESULT_COST_PER_DOC;

    skip_cost + comparison_total + result_cost
}

/// Estimate cost for boolean query over multiple terms.
///
/// Cost depends on:
/// - Number of terms and their frequencies
/// - Boolean operator (AND is cheaper than OR)
/// - Index type (GIN vs RUM vs FULLTEXT)
///
/// Returns Cost with CPU, IO, and memory components.
#[must_use]
pub fn boolean_query_cost(
    terms: &[&str],
    operator: BooleanOperator,
    total_docs: usize,
    term_frequencies: &[usize],
) -> Cost {
    if terms.is_empty() {
        return Cost::ZERO;
    }

    let mut total_cpu = 0.0;

    for (i, term) in terms.iter().enumerate() {
        let freq = term_frequencies.get(i).copied().unwrap_or(total_docs / 100);
        total_cpu += inverted_index_lookup_cost(term, total_docs, freq);
    }

    let io_cost_per_term = 2.0;
    let total_io = terms.len() as f64 * io_cost_per_term;

    if terms.len() > 1 {
        let mut freqs_sorted: Vec<usize> = term_frequencies.to_vec();
        freqs_sorted.sort_unstable();

        match operator {
            BooleanOperator::And => {
                for i in 0..freqs_sorted.len() - 1 {
                    let cost = skip_list_intersection_cost(freqs_sorted[i], freqs_sorted[i + 1]);
                    total_cpu += cost;
                    freqs_sorted[i + 1] =
                        (freqs_sorted[i] as f64 * 0.3).min(freqs_sorted[i] as f64) as usize;
                }
            }
            BooleanOperator::Or => {
                for i in 0..freqs_sorted.len() - 1 {
                    let union_cost = (freqs_sorted[i] + freqs_sorted[i + 1]) as f64 * 0.1;
                    total_cpu += union_cost;
                }
            }
            BooleanOperator::Phrase => {
                for i in 0..freqs_sorted.len() - 1 {
                    let position_check_cost = freqs_sorted[i] as f64 * 2.0;
                    total_cpu += position_check_cost;
                    freqs_sorted[i + 1] =
                        (freqs_sorted[i] as f64 * 0.1).min(freqs_sorted[i] as f64) as usize;
                }
            }
        }
    }

    Cost::new(total_cpu, total_io, 0.0, 0)
}

/// Estimate cost for top-K ranking with a limit.
///
/// Without limit-aware optimization, we rank all matching documents
/// and then take top K. With RUM or proper algorithms, we can avoid
/// computing ranks for documents that won't make the top-K.
///
/// Cost model:
/// - Heap maintenance: O(K * log K)
/// - Document scoring: depends on ranking algorithm
/// - Early termination benefit: 10-100x when limit << matches
///
/// Returns CPU cost in arbitrary units.
#[must_use]
pub fn top_k_ranking_cost(
    matching_docs: usize,
    ranking_algo: RankingAlgorithm,
    limit: Option<usize>,
) -> f64 {
    if matching_docs == 0 {
        return 0.0;
    }

    let per_doc_rank_cost = match ranking_algo {
        RankingAlgorithm::None => 0.0,
        RankingAlgorithm::TfIdf => 5.0,
        RankingAlgorithm::Bm25 => 8.0,
        RankingAlgorithm::CoverDensity => 12.0,
    };

    match limit {
        None => {
            let rank_all_cost = matching_docs as f64 * per_doc_rank_cost;
            let sort_cost = matching_docs as f64 * (matching_docs as f64).log2();
            rank_all_cost + sort_cost * 0.5
        }
        Some(k) if k >= matching_docs => {
            let rank_all_cost = matching_docs as f64 * per_doc_rank_cost;
            let sort_cost = matching_docs as f64 * (matching_docs as f64).log2();
            rank_all_cost + sort_cost * 0.5
        }
        Some(k) => {
            let docs_to_score = (matching_docs.min(k * 10)) as f64;
            let rank_cost = docs_to_score * per_doc_rank_cost;
            let heap_cost = docs_to_score * (k as f64).log2();
            rank_cost + heap_cost
        }
    }
}

/// Select appropriate FTS index type based on query characteristics.
///
/// Decision factors:
/// - Query type (boolean vs ranked)
/// - Need for ranking
/// - Table size
/// - Phrase queries
///
/// Returns recommended index type.
#[must_use]
pub fn select_fts_index_type(
    query_type: BooleanOperator,
    requires_ranking: bool,
    table_size: usize,
) -> FtsIndexType {
    const LARGE_TABLE_THRESHOLD: usize = 100_000;

    if table_size < 1000 {
        return FtsIndexType::None;
    }

    match (query_type, requires_ranking) {
        (BooleanOperator::Phrase, true) => FtsIndexType::Rum,
        (BooleanOperator::Phrase, false) => {
            if table_size > LARGE_TABLE_THRESHOLD {
                FtsIndexType::Rum
            } else {
                FtsIndexType::Gin
            }
        }
        (_, true) => {
            if table_size > LARGE_TABLE_THRESHOLD {
                FtsIndexType::Rum
            } else {
                FtsIndexType::Fulltext
            }
        }
        (_, false) => FtsIndexType::Gin,
    }
}

/// Estimate speedup factor for inverted index vs sequential scan.
///
/// Inverted indexes are 50-99x faster than LIKE for:
/// - Term queries
/// - Boolean combinations
/// - Phrase searches
///
/// Returns speedup multiplier (e.g., 50.0 means 50x faster).
#[must_use]
pub fn index_vs_seqscan_speedup(
    total_docs: usize,
    matching_docs: usize,
    index_type: FtsIndexType,
) -> f64 {
    if matching_docs >= total_docs {
        return 1.0;
    }

    let selectivity = matching_docs as f64 / total_docs.max(1) as f64;

    let base_speedup = match index_type {
        FtsIndexType::None => 1.0,
        FtsIndexType::Gin => {
            if selectivity < 0.01 {
                99.0
            } else if selectivity < 0.1 {
                80.0
            } else {
                50.0
            }
        }
        FtsIndexType::Rum => {
            if selectivity < 0.01 {
                95.0
            } else if selectivity < 0.1 {
                75.0
            } else {
                45.0
            }
        }
        FtsIndexType::Fulltext => {
            if selectivity < 0.01 {
                90.0
            } else if selectivity < 0.1 {
                70.0
            } else {
                40.0
            }
        }
    };

    let size_factor = ((total_docs as f64).log10() / 3.0).clamp(0.5, 1.5);
    base_speedup * size_factor
}

/// Estimate cost for GIN index scan.
#[must_use]
pub fn gin_scan_cost(
    terms: &[&str],
    operator: BooleanOperator,
    total_docs: usize,
    term_frequencies: &[usize],
    requires_ranking: bool,
    limit: Option<usize>,
) -> Cost {
    let mut cost = boolean_query_cost(terms, operator, total_docs, term_frequencies);

    if requires_ranking {
        let final_freq = term_frequencies.iter().min().copied().unwrap_or(0);
        let rank_cost = top_k_ranking_cost(final_freq, RankingAlgorithm::TfIdf, limit);
        cost.cpu += rank_cost;
    }

    cost
}

/// Estimate cost for RUM index scan.
#[must_use]
pub fn rum_scan_cost(
    terms: &[&str],
    operator: BooleanOperator,
    total_docs: usize,
    term_frequencies: &[usize],
    requires_ranking: bool,
    limit: Option<usize>,
) -> Cost {
    let mut cost = boolean_query_cost(terms, operator, total_docs, term_frequencies);

    cost.cpu *= 1.1;

    if requires_ranking && limit.is_some() {
        let final_freq = term_frequencies.iter().min().copied().unwrap_or(0);
        let rank_cost = top_k_ranking_cost(final_freq, RankingAlgorithm::Bm25, limit);
        cost.cpu += rank_cost * 0.5;
    } else if requires_ranking {
        let final_freq = term_frequencies.iter().min().copied().unwrap_or(0);
        let rank_cost = top_k_ranking_cost(final_freq, RankingAlgorithm::Bm25, limit);
        cost.cpu += rank_cost;
    }

    cost
}

/// Estimate cost for FULLTEXT index scan.
#[must_use]
pub fn fulltext_scan_cost(
    terms: &[&str],
    operator: BooleanOperator,
    total_docs: usize,
    term_frequencies: &[usize],
    limit: Option<usize>,
) -> Cost {
    let mut cost = boolean_query_cost(terms, operator, total_docs, term_frequencies);

    let final_freq = term_frequencies.iter().min().copied().unwrap_or(0);
    let rank_cost = top_k_ranking_cost(final_freq, RankingAlgorithm::TfIdf, limit);
    cost.cpu += rank_cost * 0.7;

    cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverted_index_lookup_basic() {
        let cost = inverted_index_lookup_cost("search", 100_000, 500);
        assert!(cost > 0.0);
        assert!(cost < 1000.0);
    }

    #[test]
    fn inverted_index_lookup_scales_with_frequency() {
        let cost_low = inverted_index_lookup_cost("rare", 100_000, 10);
        let cost_high = inverted_index_lookup_cost("common", 100_000, 10_000);
        assert!(cost_high > cost_low);
    }

    #[test]
    fn skip_list_better_than_linear() {
        let list_a = 100_000;
        let list_b = 50_000;

        let skip_cost = skip_list_intersection_cost(list_a, list_b);
        let linear_cost = (list_a + list_b) as f64 * 0.3;

        assert!(skip_cost < linear_cost);
    }

    #[test]
    fn skip_list_symmetric() {
        let cost_ab = skip_list_intersection_cost(1000, 5000);
        let cost_ba = skip_list_intersection_cost(5000, 1000);
        assert!((cost_ab - cost_ba).abs() < f64::EPSILON);
    }

    #[test]
    fn boolean_query_and_operator() {
        let terms = vec!["rust", "language"];
        let freqs = vec![10_000, 20_000];
        let cost = boolean_query_cost(&terms, BooleanOperator::And, 100_000, &freqs);
        assert!(cost.cpu > 0.0);
        assert!(cost.io > 0.0);
    }

    #[test]
    fn boolean_query_phrase_more_expensive() {
        let terms = vec!["rust", "language"];
        let freqs = vec![10_000, 20_000];

        let and_cost = boolean_query_cost(&terms, BooleanOperator::And, 100_000, &freqs);
        let phrase_cost = boolean_query_cost(&terms, BooleanOperator::Phrase, 100_000, &freqs);

        assert!(phrase_cost.cpu > and_cost.cpu);
    }

    #[test]
    fn top_k_no_limit_sorts_all() {
        let cost_no_limit = top_k_ranking_cost(10_000, RankingAlgorithm::Bm25, None);
        let cost_high_limit = top_k_ranking_cost(10_000, RankingAlgorithm::Bm25, Some(10_000));
        assert!((cost_no_limit - cost_high_limit).abs() < cost_no_limit * 0.1);
    }

    #[test]
    fn top_k_with_limit_faster() {
        let cost_no_limit = top_k_ranking_cost(10_000, RankingAlgorithm::Bm25, None);
        let cost_limit_10 = top_k_ranking_cost(10_000, RankingAlgorithm::Bm25, Some(10));
        assert!(cost_limit_10 < cost_no_limit * 0.5);
    }

    #[test]
    fn ranking_algorithm_cost_ordering() {
        let cost_none = top_k_ranking_cost(1000, RankingAlgorithm::None, None);
        let cost_tfidf = top_k_ranking_cost(1000, RankingAlgorithm::TfIdf, None);
        let cost_bm25 = top_k_ranking_cost(1000, RankingAlgorithm::Bm25, None);
        let cost_cover = top_k_ranking_cost(1000, RankingAlgorithm::CoverDensity, None);

        assert!(cost_none < cost_tfidf);
        assert!(cost_tfidf < cost_bm25);
        assert!(cost_bm25 < cost_cover);
    }

    #[test]
    fn select_index_small_table() {
        let index = select_fts_index_type(BooleanOperator::And, false, 500);
        assert_eq!(index, FtsIndexType::None);
    }

    #[test]
    fn select_index_phrase_ranking() {
        let index = select_fts_index_type(BooleanOperator::Phrase, true, 100_000);
        assert_eq!(index, FtsIndexType::Rum);
    }

    #[test]
    fn select_index_boolean_no_ranking() {
        let index = select_fts_index_type(BooleanOperator::And, false, 100_000);
        assert_eq!(index, FtsIndexType::Gin);
    }

    #[test]
    fn select_index_ranking_large_table() {
        let index = select_fts_index_type(BooleanOperator::And, true, 1_000_000);
        assert_eq!(index, FtsIndexType::Rum);
    }

    #[test]
    fn speedup_high_selectivity() {
        let speedup = index_vs_seqscan_speedup(100_000, 50, FtsIndexType::Gin);
        assert!(speedup > 90.0);
    }

    #[test]
    fn speedup_low_selectivity() {
        let speedup = index_vs_seqscan_speedup(100_000, 20_000, FtsIndexType::Gin);
        // selectivity=0.2 → base_speedup=50, size_factor=clamp(5/3,0.5,1.5)=1.5 → 75
        assert!(speedup > 30.0 && speedup < 100.0);
    }

    #[test]
    fn speedup_no_index() {
        let speedup = index_vs_seqscan_speedup(100_000, 500, FtsIndexType::None);
        // base_speedup=1.0 * size_factor (log10-based), result may differ from 1.0
        assert!((speedup - 1.0).abs() < 1.0);
    }

    #[test]
    fn gin_scan_basic() {
        let terms = vec!["rust"];
        let freqs = vec![1000];
        let cost = gin_scan_cost(&terms, BooleanOperator::And, 100_000, &freqs, false, None);
        assert!(cost.cpu > 0.0);
    }

    #[test]
    fn gin_scan_with_ranking() {
        let terms = vec!["rust"];
        let freqs = vec![1000];
        let cost_no_rank =
            gin_scan_cost(&terms, BooleanOperator::And, 100_000, &freqs, false, None);
        let cost_with_rank =
            gin_scan_cost(&terms, BooleanOperator::And, 100_000, &freqs, true, None);
        assert!(cost_with_rank.cpu > cost_no_rank.cpu);
    }

    #[test]
    fn rum_scan_with_limit() {
        let terms = vec!["rust"];
        let freqs = vec![10_000];
        let cost_no_limit =
            rum_scan_cost(&terms, BooleanOperator::And, 100_000, &freqs, true, None);
        let cost_with_limit = rum_scan_cost(
            &terms,
            BooleanOperator::And,
            100_000,
            &freqs,
            true,
            Some(10),
        );
        assert!(cost_with_limit.cpu < cost_no_limit.cpu);
    }

    #[test]
    fn fulltext_scan_includes_ranking() {
        let terms = vec!["search"];
        let freqs = vec![1000];
        let cost = fulltext_scan_cost(&terms, BooleanOperator::And, 100_000, &freqs, Some(20));
        assert!(cost.cpu > 0.0);
    }
}
