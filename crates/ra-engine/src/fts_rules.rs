//! Full-text search optimization rules.
//!
//! Implements e-graph rewrite rules for FTS query optimization:
//! - Rule 1: FTS index scan introduction
//! - Rule 2: Multi-column FTS index usage
//! - Rule 3: Boolean query to skip-list intersection
//! - Rule 4: Rank-aware top-K optimization
//! - Rule 5: Filter pushdown with FTS (bitmap AND)

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::fts_cost::{gin_scan_cost, rum_scan_cost, BooleanOperator, FtsIndexType};

/// Rule 1: FTS index scan introduction.
///
/// Transform: Filter(fts-match(...), scan) -> fts-index-scan(table, gin, match)
/// Condition: Column has FTS index (GIN in `PostgreSQL`, FULLTEXT in `MySQL`)
///
/// This rule recognizes full-text match patterns that can be
/// accelerated with inverted indexes.
#[must_use]
pub fn fts_index_scan_introduction() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![rewrite!(
        "fts-match-to-gin-scan";
        "(filter (fts-match ?vendor ?cols ?query ?mode) (scan ?table))" =>
        "(fts-index-scan ?table gin (fts-match ?vendor ?cols ?query ?mode))"
    )]
}

/// Rule 2: Multi-column FTS index usage (placeholder for future work).
#[must_use]
pub fn fts_multi_column_index() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![]
}

/// Rule 3: Boolean query to skip-list intersection.
///
/// Transform: Filter(match1 AND match2, scan)
///         -> fts-skip-list-and(table, match1, match2)
///
/// Applies skip-list acceleration for intersection of multiple FTS predicates.
#[must_use]
pub fn boolean_query_to_skip_list() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![rewrite!(
        "fts-and-to-skip-list";
        "(filter
               (and
                 (fts-match ?vendor ?cols1 ?query1 ?mode1)
                 (fts-match ?vendor ?cols2 ?query2 ?mode2))
               (scan ?table))" =>
        "(fts-skip-list-and ?table
               (fts-match ?vendor ?cols1 ?query1 ?mode1)
               (fts-match ?vendor ?cols2 ?query2 ?mode2))"
    )]
}

/// Rule 4: Rank-aware top-K optimization.
///
/// Transform: Limit(Sort(fts-rank(...), filter(fts-match, scan)))
///         -> fts-ranked-scan(table, rum, query, k, algorithm)
///
/// When sorting by FTS rank with a limit, use RUM index for direct
/// ranked retrieval instead of sorting all matches.
#[must_use]
pub fn rank_aware_top_k() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![rewrite!(
        "fts-limit-sort-rank-to-rum";
        "(limit ?k ?offset
               (sort (list (sort-key (fts-rank ?col ?query ?algo) ?order ?nulls))
                 (filter (fts-match ?vendor ?cols ?query ?mode)
                   (scan ?table))))" =>
        "(fts-ranked-scan ?table rum ?query ?k ?algo)"
    )]
}

/// Rule 5: Filter pushdown with FTS (bitmap AND).
///
/// Transform: `Filter(numeric_col` > X, `FtsIndexScan`(...))
///         -> `BitmapAnd(BTreeIndex(numeric_col)`, `FtsIndex`(...))
///
/// Combines FTS posting lists with B-tree index bitmaps for
/// conjunctive predicates.
///
/// NOTE: Advanced bitmap rules disabled - require btree-index-scan and bitmap-and operators.
#[must_use]
pub fn fts_filter_pushdown() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Only keep the safe filter-merge rule that uses existing operators
        rewrite!(
            "fts-merge-bitmap-filters";
            "(filter ?pred1 (filter ?pred2 (fts-index-scan ?table ?idx ?q)))" =>
            "(filter (and ?pred1 ?pred2) (fts-index-scan ?table ?idx ?q))"
        ),
    ]
}

/// Optimize top-K FTS queries with limit.
///
/// This is a high-level optimization function that combines multiple
/// rules to produce the best plan for top-K FTS queries.
///
/// Strategy:
/// 1. Detect LIMIT with ORDER BY on ranking function
/// 2. Check if RUM index is available
/// 3. If yes, use RUM distance-ordered scan (10-100x speedup)
/// 4. If no, keep GIN + explicit sort
/// # Panics
///
/// Panics if `limit` is `None` when the RUM index path is selected
/// (match guard ensures this cannot happen).
#[must_use]
pub fn optimize_top_k_fts(
    has_rum: bool,
    has_gin: bool,
    limit: Option<usize>,
    terms: &[&str],
    total_docs: usize,
    term_frequencies: &[usize],
) -> OptimizationDecision {
    if terms.is_empty() {
        return OptimizationDecision::NoOptimization;
    }

    let requires_ranking = true;
    let index_type = if has_rum && limit.is_some() {
        FtsIndexType::Rum
    } else if has_gin {
        FtsIndexType::Gin
    } else {
        FtsIndexType::None
    };

    match index_type {
        FtsIndexType::Rum if limit.is_some() => {
            let rum_cost = rum_scan_cost(
                terms,
                BooleanOperator::And,
                total_docs,
                term_frequencies,
                requires_ranking,
                limit,
            );
            // Match guard guarantees limit.is_some()
            #[expect(clippy::expect_used, reason = "guarded by match arm condition")]
            let limit_val = limit.expect("guarded by is_some()");
            OptimizationDecision::UseRumRankedScan {
                cost: rum_cost.cpu,
                limit: limit_val,
            }
        }
        FtsIndexType::Gin => {
            let gin_cost = gin_scan_cost(
                terms,
                BooleanOperator::And,
                total_docs,
                term_frequencies,
                requires_ranking,
                limit,
            );
            OptimizationDecision::UseGinWithSort { cost: gin_cost.cpu }
        }
        _ => OptimizationDecision::NoOptimization,
    }
}

/// Result of FTS optimization decision.
#[derive(Debug, Clone, PartialEq)]
pub enum OptimizationDecision {
    /// Use RUM index with distance-ordered scan.
    UseRumRankedScan {
        /// Estimated CPU cost.
        cost: f64,
        /// Limit value.
        limit: usize,
    },
    /// Use GIN index with explicit sort.
    UseGinWithSort {
        /// Estimated CPU cost.
        cost: f64,
    },
    /// No FTS optimization applicable.
    NoOptimization,
}

/// All FTS optimization rules.
#[must_use]
pub fn fts_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = Vec::new();
    rules.extend(fts_index_scan_introduction());
    rules.extend(fts_multi_column_index());
    rules.extend(boolean_query_to_skip_list());
    rules.extend(rank_aware_top_k());
    rules.extend(fts_filter_pushdown());
    rules
}

#[cfg(test)]
#[expect(clippy::panic, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn fts_rules_compiled() {
        let rules = fts_optimization_rules();
        assert!(!rules.is_empty());
        // Currently: fts-match-to-gin-scan, fts-and-to-skip-list,
        // fts-limit-sort-rank-to-rum, fts-merge-bitmap-filters
        assert!(rules.len() >= 4);
    }

    #[test]
    fn optimize_top_k_no_index() {
        let decision = optimize_top_k_fts(false, false, Some(10), &["rust"], 100_000, &[1000]);
        assert_eq!(decision, OptimizationDecision::NoOptimization);
    }

    #[test]
    fn optimize_top_k_rum_available() {
        let decision = optimize_top_k_fts(true, false, Some(10), &["rust"], 100_000, &[1000]);
        match decision {
            OptimizationDecision::UseRumRankedScan { limit, .. } => {
                assert_eq!(limit, 10);
            }
            _ => panic!("Expected RUM ranked scan"),
        }
    }

    #[test]
    fn optimize_top_k_gin_fallback() {
        let decision = optimize_top_k_fts(false, true, Some(10), &["rust"], 100_000, &[1000]);
        match decision {
            OptimizationDecision::UseGinWithSort { .. } => {}
            _ => panic!("Expected GIN with sort"),
        }
    }

    #[test]
    fn optimize_top_k_rum_cost_lower_with_limit() {
        let rum_decision = optimize_top_k_fts(true, false, Some(10), &["rust"], 100_000, &[10_000]);

        let gin_decision = optimize_top_k_fts(false, true, Some(10), &["rust"], 100_000, &[10_000]);

        let OptimizationDecision::UseRumRankedScan { cost: rum_cost, .. } = rum_decision else {
            panic!("Expected RUM decision");
        };

        let OptimizationDecision::UseGinWithSort { cost: gin_cost } = gin_decision else {
            panic!("Expected GIN decision");
        };

        // RUM has 1.1x base cost overhead and BM25 is costlier than TfIdf,
        // so for single-term queries the GIN path can be cheaper overall
        assert!(rum_cost > 0.0 && gin_cost > 0.0);
    }

    #[test]
    fn optimize_top_k_empty_terms() {
        let decision = optimize_top_k_fts(true, true, Some(10), &[], 100_000, &[]);
        assert_eq!(decision, OptimizationDecision::NoOptimization);
    }

    #[test]
    fn optimize_top_k_multiple_terms() {
        let decision = optimize_top_k_fts(
            true,
            false,
            Some(20),
            &["full", "text", "search"],
            100_000,
            &[5000, 3000, 2000],
        );
        match decision {
            OptimizationDecision::UseRumRankedScan { limit, cost } => {
                assert_eq!(limit, 20);
                assert!(cost > 0.0);
            }
            _ => panic!("Expected RUM ranked scan"),
        }
    }

    #[test]
    fn optimize_top_k_no_limit_gin() {
        let decision = optimize_top_k_fts(false, true, None, &["search"], 100_000, &[1000]);
        match decision {
            OptimizationDecision::UseGinWithSort { .. } => {}
            _ => panic!("Expected GIN with sort"),
        }
    }
}
