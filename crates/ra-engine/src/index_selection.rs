// ! Basic index scan selection rules.
//!
//! Introduces `IndexScan` operators when an appropriate index exists
//! for a filter predicate. Unlike covering indexes (index-only scans),
//! these scans still need to fetch rows from the heap after using the
//! index to find matching row IDs.
//!
//! # Performance Characteristics
//!
//! Index scans provide significant speedup when:
//! - Selectivity is low (< 5-10% of rows match)
//! - Index matches the filter predicate
//! - Random access cost < sequential scan cost
//!
//! Typical speedup:
//! - High selectivity (0.1%): 100-1000x faster
//! - Medium selectivity (1%): 10-100x faster
//! - Low selectivity (10%): 2-10x faster
//! - Very low selectivity (>20%): Sequential scan may be faster!
//!
//! # Example
//!
//! ```sql
//! -- Index on customer_id
//! CREATE INDEX idx_orders_customer ON orders(customer_id);
//!
//! -- Query
//! SELECT * FROM orders WHERE customer_id = 123;
//! ```
//!
//! Rewritten from:
//! ```text
//! Filter(customer_id = 123) → Scan(orders)
//! ```
//!
//! To:
//! ```text
//! IndexScan(orders, idx_orders_customer, customer_id = 123)
//! ```

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Rewrite rules for basic index scan selection.
///
/// These rules introduce `IndexScan` operators when filtering on
/// indexed columns. The cost model will decide whether to actually
/// use the index based on selectivity.
///
/// # Design
///
/// Since egg's pattern matching cannot access external metadata
/// (the facts provider), we introduce index scans optimistically
/// with a sentinel "auto" index name. The cost model then:
/// 1. Checks if an appropriate index exists via FactsProvider
/// 2. Estimates index scan cost vs sequential scan cost
/// 3. Assigns ∞ cost if no index exists or seq scan is cheaper
///
/// This way the e-graph explores both scan strategies and the
/// extractor picks the best one.
#[must_use]
pub fn index_selection_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Basic bitmap index scan introduction:
        // filter(pred, scan(table)) => bitmap-index-scan(table, "auto", pred)
        //
        // The "auto" sentinel tells the cost model to:
        // 1. Find an index matching the predicate
        // 2. Estimate selectivity and cost
        // 3. Use bitmap index scan if cost < sequential scan cost
        rewrite!("filter-scan-to-bitmap-index";
            "(filter ?pred (scan ?table))" =>
            "(bitmap-index-scan ?table auto ?pred)"
        ),

        // Reverse direction for e-graph exploration
        rewrite!("bitmap-index-to-filter-scan";
            "(bitmap-index-scan ?table auto ?pred)" =>
            "(filter ?pred (scan ?table))"
        ),

        // Bitmap index scan with additional filter:
        // Some predicates can't be evaluated purely by the index
        // and need an additional filter step
        rewrite!("bitmap-index-scan-with-filter";
            "(filter ?pred1 (bitmap-index-scan ?table ?idx ?pred2))" =>
            "(filter ?pred1 (bitmap-index-scan ?table ?idx ?pred2))"
        ),
    ]
}

/// Cost factor for index scan vs sequential scan.
///
/// This function should be called by the cost model to determine
/// whether to use an index. It considers:
/// - Index selectivity (what % of rows match)
/// - Random vs sequential I/O costs
/// - Index size vs table size
///
/// Returns a cost multiplier:
/// - < 1.0: Index scan is faster
/// - = 1.0: Same cost
/// - > 1.0: Sequential scan is faster
#[must_use]
pub fn index_scan_cost_factor(
    selectivity: f64,
    table_rows: u64,
    index_pages: u64,
    table_pages: u64,
) -> f64 {
    // Rough heuristic for index scan cost:
    // - Read index pages: index_pages * 0.5 (smaller, more compact)
    // - Read matching heap pages: table_pages * selectivity * 4.0 (random I/O)
    //
    // Sequential scan cost:
    // - Read all table pages: table_pages * 1.0 (sequential I/O)

    let rows_to_fetch = (table_rows as f64 * selectivity).max(1.0);

    // Index I/O cost (assume ~50% of index needs to be read for lookup)
    let index_io_cost = (index_pages as f64 * 0.5).max(1.0);

    // Heap fetch cost (random I/O is 4x more expensive than sequential)
    let heap_fetch_cost = rows_to_fetch * 4.0 / table_rows as f64 * table_pages as f64;

    // Total index scan cost
    let index_scan_cost = index_io_cost + heap_fetch_cost;

    // Sequential scan cost
    let seq_scan_cost = table_pages as f64;

    // Return cost ratio
    index_scan_cost / seq_scan_cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_scan_rules_exist() {
        let rules = index_selection_rules();
        assert!(!rules.is_empty(), "Should have index selection rules");
        assert_eq!(rules.len(), 3, "Should have 3 index selection rules");
    }

    #[test]
    fn test_index_cost_factor_high_selectivity() {
        // High selectivity (0.1% of rows) - index should be much faster
        let factor = index_scan_cost_factor(0.001, 1_000_000, 100, 10_000);
        assert!(factor < 0.1, "Index should be 10x+ faster for 0.1% selectivity");
    }

    #[test]
    fn test_index_cost_factor_medium_selectivity() {
        // Medium selectivity (5% of rows) - index should be somewhat faster
        let factor = index_scan_cost_factor(0.05, 1_000_000, 100, 10_000);
        assert!(factor < 1.0, "Index should be faster for 5% selectivity");
    }

    #[test]
    fn test_index_cost_factor_low_selectivity() {
        // Low selectivity (50% of rows) - sequential scan should be faster
        let factor = index_scan_cost_factor(0.5, 1_000_000, 100, 10_000);
        assert!(factor > 1.0, "Sequential scan should be faster for 50% selectivity");
    }

    #[test]
    fn test_index_cost_factor_full_scan() {
        // Full table scan (100% of rows) - sequential scan definitely faster
        let factor = index_scan_cost_factor(1.0, 1_000_000, 100, 10_000);
        assert!(factor > 2.0, "Sequential scan should be much faster for full table");
    }
}
