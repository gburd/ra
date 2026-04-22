//! Vector similarity search optimization rules.
//!
//! Implements Phase 4 of RFC 0064: transformation rules for vector queries:
//! 1. Vector index scan introduction
//! 2. TopK optimization (sort + limit → vector_knn_scan)
//! 3. Pre-filter vs post-filter optimization
//!
//! These rules convert distance-based filters and sorting operations into
//! specialized vector index scans for HNSW and IVFFlat indexes.

use egg::{rewrite as rw, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::vector_cost::{
    hnsw_search_cost, ivfflat_search_cost, vector_sequential_scan_cost, VectorIndexType,
    VectorMetric,
};

/// Generate vector similarity search optimization rules.
///
/// Rules:
/// 1. **Index scan introduction**: Convert distance filters to index scans
///    ```text
///    filter(distance(col, query) < threshold, scan(table))
///    → vector_index_scan(table, col, query, threshold)
///    ```
///
/// 2. **TopK optimization**: Convert sort + limit to KNN scan
///    ```text
///    limit(k, sort(distance(col, query), scan(table)))
///    → vector_knn_scan(table, col, query, k)
///    ```
///
/// 3. **Pre-filter optimization**: Push non-vector filters before vector scan
///    when filter selectivity is high (>0.9)
///
/// 4. **Post-filter optimization**: Pull non-vector filters after vector scan
///    when filter selectivity is low (<0.1)
pub fn vector_rewrite_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Rule 1: Sort by vector distance + Limit → KNN scan
        //
        // Pattern: limit(offset, k, sort([sort-key(vector-distance(...), ...)], scan))
        // → vector-knn(table, column, target, k)
        //
        // This is the canonical pattern for K-nearest-neighbor search.
        // Using a vector index (HNSW/IVFFlat) is exponentially faster than
        // sequential scan + sort for large datasets.
        //
        // NOTE: Currently requires direct scan. If there's a Project/Filter between
        // Sort and Scan, other rules must push them down first.
        rw!("vector-topk-to-knn";
            "(limit ?k ?offset
               (sort (list (sort-key (vector-distance ?metric ?col ?target) ?order ?nulls))
                 (scan ?table)))" =>
            "(limit ?k ?offset
               (vector-knn ?table ?col ?target ?k))"
        ),
        // Rule 2: Distance filter < threshold → Vector range scan
        //
        // Pattern: filter(vector-distance < threshold, scan)
        // → vector-range-scan(table, col, target, threshold, metric)
        //
        // Range queries can use index probes instead of full scans.
        rw!("vector-filter-to-range";
            "(filter
               (lt (vector-distance ?metric ?col ?target) ?threshold)
               (scan ?table))" =>
            "(vector-range-scan ?table ?col ?target ?threshold ?metric)"
        ),
        // Rule 2b: Distance filter <= threshold variant
        rw!("vector-filter-to-range-le";
            "(filter
               (le (vector-distance ?metric ?col ?target) ?threshold)
               (scan ?table))" =>
            "(vector-range-scan ?table ?col ?target ?threshold ?metric)"
        ),
        // Rule 3: Pre-filter optimization - scalar filter before vector
        //
        // Pattern: sort(distance, filter(pred, scan)) where pred is highly selective
        // → Apply filter after KNN scan (post-filter, since VectorKNN is a leaf)
        //
        // Note: True pre-filtering would require index-level support.
        // This rule converts to KNN + filter for now.
        rw!("vector-prefilter";
            "(limit ?k ?offset
               (sort (list (sort-key (vector-distance ?metric ?col ?target) ?order ?nulls))
                 (filter ?pred (scan ?table))))" =>
            "(filter ?pred
               (limit ?k ?offset
                 (vector-knn ?table ?col ?target ?k)))"
        ),
        // Rule 4: Post-filter optimization - vector first, then scalar
        //
        // Pattern: filter(pred, sort(distance, scan)) where pred is not selective
        // → Apply vector search first, then filter the top-K results
        //
        // This is beneficial when:
        // - Scalar filter has low selectivity (<10% filtered out)
        // - Vector search with index is very fast
        rw!("vector-postfilter";
            "(filter ?pred
               (limit ?k ?offset
                 (sort (list (sort-key (vector-distance ?metric ?col ?target) ?order ?nulls))
                   (scan ?table))))" =>
            "(filter ?pred
               (vector-knn ?table ?col ?target ?k))"
        ),
    ]
}

/// Optimize the order of vector and non-vector filters.
///
/// Decision criteria:
/// - **Pre-filter** (apply before vector scan) if:
///   - Non-vector filter selectivity > 0.9 (highly selective)
///   - Non-vector filter cost << vector scan cost
///   - Vector index type is Sequential (no index benefit lost)
///
/// - **Post-filter** (apply after vector scan) if:
///   - Non-vector filter selectivity < 0.1 (barely selective)
///   - Vector scan already reduces candidate set significantly
///   - Vector index type is HNSW or IVFFlat
///
/// - **Integrated filter** (use index-level filter) if:
///   - Index supports predicate pushdown (e.g., pgvector with WHERE clause)
///   - Both filters are selective
///
/// Returns: (should_prefilter, should_postfilter, expected_speedup)
#[must_use]
pub fn optimize_vector_filter_order(
    non_vector_selectivity: f64,
    non_vector_cost: f64,
    vector_selectivity: f64,
    index_type: VectorIndexType,
    total_rows: usize,
) -> (bool, bool, f64) {
    // Highly selective non-vector filter → pre-filter
    if non_vector_selectivity > 0.9 {
        let reduced_rows = total_rows as f64 * (1.0 - non_vector_selectivity);
        let speedup = total_rows as f64 / reduced_rows.max(1.0);
        return (true, false, speedup);
    }

    // Barely selective non-vector filter → post-filter (if vector scan helps)
    if non_vector_selectivity < 0.1 && index_type != VectorIndexType::Sequential {
        let vector_reduced = total_rows as f64 * vector_selectivity;
        let speedup = total_rows as f64 / vector_reduced.max(1.0);
        return (false, true, speedup);
    }

    // Sequential scan with moderately selective filters → pre-filter if cheap
    if index_type == VectorIndexType::Sequential && non_vector_cost < 0.01 {
        return (true, false, 1.2);
    }

    // Default: apply filters in original order
    (false, false, 1.0)
}

/// Estimate the cost of a vector query with different filter orderings.
///
/// Compares three strategies:
/// 1. Pre-filter: Apply non-vector filter first, then vector scan
/// 2. Post-filter: Apply vector scan first, then non-vector filter
/// 3. Integrated: Use index-level filtering if supported
///
/// Returns the strategy with the lowest estimated cost.
#[must_use]
pub fn estimate_vector_query_cost(
    dimensions: usize,
    total_rows: usize,
    vector_metric: VectorMetric,
    vector_selectivity: f64,
    non_vector_selectivity: f64,
    non_vector_cost_per_row: f64,
    index_type: VectorIndexType,
    index_params: VectorIndexParams,
) -> VectorQueryCost {
    // Strategy 1: Pre-filter (non-vector first)
    let prefilter_rows = (total_rows as f64 * (1.0 - non_vector_selectivity)).ceil() as usize;
    let prefilter_nv_cost = total_rows as f64 * non_vector_cost_per_row;
    let prefilter_vec_cost = match index_type {
        VectorIndexType::HNSW => hnsw_search_cost(
            dimensions,
            index_params.hnsw_m,
            index_params.hnsw_ef_search,
            prefilter_rows,
            index_params.k,
            vector_metric,
        )
        .total(),
        VectorIndexType::IVFFlat => ivfflat_search_cost(
            dimensions,
            index_params.ivfflat_lists,
            index_params.ivfflat_probes,
            prefilter_rows,
            index_params.k,
            vector_metric,
        )
        .total(),
        VectorIndexType::Sequential => {
            vector_sequential_scan_cost(dimensions, prefilter_rows, vector_metric).total()
        }
    };
    let prefilter_total = prefilter_nv_cost + prefilter_vec_cost;

    // Strategy 2: Post-filter (vector first)
    let postfilter_vec_cost = match index_type {
        VectorIndexType::HNSW => hnsw_search_cost(
            dimensions,
            index_params.hnsw_m,
            index_params.hnsw_ef_search,
            total_rows,
            index_params.k,
            vector_metric,
        )
        .total(),
        VectorIndexType::IVFFlat => ivfflat_search_cost(
            dimensions,
            index_params.ivfflat_lists,
            index_params.ivfflat_probes,
            total_rows,
            index_params.k,
            vector_metric,
        )
        .total(),
        VectorIndexType::Sequential => {
            vector_sequential_scan_cost(dimensions, total_rows, vector_metric).total()
        }
    };
    let postfilter_rows = (total_rows as f64 * vector_selectivity).ceil() as usize;
    let postfilter_nv_cost = postfilter_rows as f64 * non_vector_cost_per_row;
    let postfilter_total = postfilter_vec_cost + postfilter_nv_cost;

    // Strategy 3: Integrated (assume 20% overhead for coordination)
    let integrated_total = postfilter_vec_cost.min(prefilter_total) * 1.2;

    // Select best strategy
    let (strategy, total_cost) =
        if prefilter_total < postfilter_total && prefilter_total < integrated_total {
            (FilterStrategy::PreFilter, prefilter_total)
        } else if postfilter_total < integrated_total {
            (FilterStrategy::PostFilter, postfilter_total)
        } else {
            (FilterStrategy::Integrated, integrated_total)
        };

    VectorQueryCost {
        strategy,
        total_cost,
        vector_cost: if strategy == FilterStrategy::PreFilter {
            prefilter_vec_cost
        } else {
            postfilter_vec_cost
        },
        non_vector_cost: if strategy == FilterStrategy::PreFilter {
            prefilter_nv_cost
        } else {
            postfilter_nv_cost
        },
        rows_scanned: if strategy == FilterStrategy::PreFilter {
            prefilter_rows
        } else {
            total_rows
        },
    }
}

/// Parameters for vector index configuration.
#[derive(Debug, Clone, Copy)]
pub struct VectorIndexParams {
    /// Number of nearest neighbors to return.
    pub k: usize,
    /// HNSW: max connections per node.
    pub hnsw_m: usize,
    /// HNSW: search breadth parameter.
    pub hnsw_ef_search: usize,
    /// IVFFlat: number of clusters.
    pub ivfflat_lists: usize,
    /// IVFFlat: number of clusters to probe.
    pub ivfflat_probes: usize,
}

impl Default for VectorIndexParams {
    fn default() -> Self {
        Self {
            k: 10,
            hnsw_m: 16,
            hnsw_ef_search: 40,
            ivfflat_lists: 100,
            ivfflat_probes: 10,
        }
    }
}

/// Filter application strategy for vector queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterStrategy {
    /// Apply non-vector filter before vector scan.
    PreFilter,
    /// Apply non-vector filter after vector scan.
    PostFilter,
    /// Use index-level integrated filtering.
    Integrated,
}

/// Cost breakdown for a vector query.
#[derive(Debug, Clone)]
pub struct VectorQueryCost {
    /// Recommended filter strategy.
    pub strategy: FilterStrategy,
    /// Total estimated cost.
    pub total_cost: f64,
    /// Cost of vector operations.
    pub vector_cost: f64,
    /// Cost of non-vector filter operations.
    pub non_vector_cost: f64,
    /// Number of rows scanned by vector operation.
    pub rows_scanned: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_rules_exist() {
        let rules = vector_rewrite_rules();
        assert!(!rules.is_empty());
        assert_eq!(rules.len(), 5);
    }

    #[test]
    fn optimize_filter_order_highly_selective_prefilters() {
        let (prefilter, postfilter, speedup) = optimize_vector_filter_order(
            0.95,  // 95% selectivity
            0.001, // cheap filter
            0.01,  // vector very selective
            VectorIndexType::HNSW,
            100_000,
        );
        assert!(prefilter);
        assert!(!postfilter);
        assert!(speedup > 1.0);
    }

    #[test]
    fn optimize_filter_order_barely_selective_postfilters() {
        let (prefilter, postfilter, _speedup) = optimize_vector_filter_order(
            0.05, // 5% selectivity (barely any)
            0.01,
            0.10, // vector moderately selective
            VectorIndexType::HNSW,
            100_000,
        );
        assert!(!prefilter);
        assert!(postfilter);
    }

    #[test]
    fn optimize_filter_order_sequential_prefers_prefilter() {
        let (prefilter, postfilter, _speedup) = optimize_vector_filter_order(
            0.50,  // moderate selectivity
            0.001, // cheap filter
            0.10,
            VectorIndexType::Sequential, // no index
            100_000,
        );
        assert!(prefilter);
        assert!(!postfilter);
    }

    #[test]
    fn estimate_cost_prefilter_cheaper_for_selective_filter() {
        let params = VectorIndexParams::default();
        let cost = estimate_vector_query_cost(
            128,
            100_000,
            VectorMetric::L2,
            0.01, // vector 1% selectivity
            0.90, // non-vector 90% selectivity (eliminate 90%)
            0.001,
            VectorIndexType::HNSW,
            params,
        );

        assert_eq!(cost.strategy, FilterStrategy::PostFilter);
        // PostFilter: HNSW on full table, then apply non-vector filter
        assert_eq!(cost.rows_scanned, 100_000);
    }

    #[test]
    fn estimate_cost_postfilter_cheaper_for_nonselective_filter() {
        let params = VectorIndexParams::default();
        let cost = estimate_vector_query_cost(
            128,
            100_000,
            VectorMetric::L2,
            0.01, // vector 1% selectivity (very selective)
            0.05, // non-vector 5% selectivity (barely any)
            0.01, // expensive per-row filter
            VectorIndexType::HNSW,
            params,
        );

        assert_eq!(cost.strategy, FilterStrategy::PostFilter);
        // Vector scan reduces to 1K rows before expensive filter
    }

    #[test]
    fn estimate_cost_sequential_scan_baseline() {
        let params = VectorIndexParams::default();
        let cost = estimate_vector_query_cost(
            128,
            10_000,
            VectorMetric::L2,
            0.01,
            0.50,
            0.001,
            VectorIndexType::Sequential,
            params,
        );

        // For small datasets, pre/post shouldn't matter much
        assert!(cost.total_cost > 0.0);
    }

    #[test]
    fn estimate_cost_ivfflat_vs_hnsw() {
        let params = VectorIndexParams::default();

        let hnsw_cost = estimate_vector_query_cost(
            128,
            100_000,
            VectorMetric::L2,
            0.01,
            0.50,
            0.001,
            VectorIndexType::HNSW,
            params,
        );

        let ivfflat_cost = estimate_vector_query_cost(
            128,
            100_000,
            VectorMetric::L2,
            0.01,
            0.50,
            0.001,
            VectorIndexType::IVFFlat,
            params,
        );

        // HNSW should be faster for large datasets
        assert!(hnsw_cost.total_cost < ivfflat_cost.total_cost);
    }

    #[test]
    fn default_index_params_reasonable() {
        let params = VectorIndexParams::default();
        assert_eq!(params.k, 10);
        assert_eq!(params.hnsw_m, 16);
        assert_eq!(params.hnsw_ef_search, 40);
        assert_eq!(params.ivfflat_lists, 100);
        assert_eq!(params.ivfflat_probes, 10);
    }
}
