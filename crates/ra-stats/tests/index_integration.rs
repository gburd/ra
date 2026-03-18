//! Integration tests for index types, cost model, and selection algorithm.
#![allow(
    clippy::redundant_closure_for_method_calls,
    clippy::inefficient_to_string
)]

use ra_stats::index_types::{
    IndexCostFactors, IndexMetadata, IndexRecommendation,
    IndexType, SortDirection, select_best_index,
};

// ---------------------------------------------------------------------------
// Helper builders
// ---------------------------------------------------------------------------

fn btree_index(id: &str, table: &str, cols: &[&str]) -> IndexMetadata {
    IndexMetadata {
        index_id: id.to_string(),
        table_name: table.to_string(),
        index_type: IndexType::NonClustered {
            columns: cols.iter().map(|c| c.to_string()).collect(),
            included_columns: vec![],
        },
        is_primary: false,
        is_unique: false,
        is_valid: true,
        size_bytes: 50_000_000,
        leaf_pages: 5000,
        levels: 3,
        fill_factor: 0.9,
        distinct_keys: 500_000,
        clustering_factor: 25_000.0,
    }
}

fn clustered_index(id: &str, table: &str, cols: &[&str]) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Clustered {
            columns: cols.iter().map(|c| c.to_string()).collect(),
        },
        is_primary: true,
        clustering_factor: 1.0,
        ..btree_index(id, table, cols)
    }
}

fn hash_index(id: &str, table: &str, cols: &[&str]) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Hash {
            columns: cols.iter().map(|c| c.to_string()).collect(),
        },
        ..btree_index(id, table, cols)
    }
}

fn gin_index(id: &str, table: &str, col: &str, opclass: &str) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Gin {
            column: col.to_string(),
            opclass: opclass.to_string(),
        },
        ..btree_index(id, table, &[col])
    }
}

fn gist_index(id: &str, table: &str, col: &str, opclass: &str) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Gist {
            column: col.to_string(),
            opclass: opclass.to_string(),
        },
        ..btree_index(id, table, &[col])
    }
}

fn fulltext_index(id: &str, table: &str, cols: &[&str]) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::FullText {
            columns: cols.iter().map(|c| c.to_string()).collect(),
            language: "english".to_string(),
            custom_stopwords: false,
        },
        ..btree_index(id, table, cols)
    }
}

fn spatial_index(id: &str, table: &str, col: &str) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Spatial {
            column: col.to_string(),
            srid: 4326,
        },
        ..btree_index(id, table, &[col])
    }
}

fn columnstore_index(id: &str, table: &str, cols: &[&str]) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Columnstore {
            columns: cols.iter().map(|c| c.to_string()).collect(),
        },
        ..btree_index(id, table, cols)
    }
}

fn unique_index(id: &str, table: &str, cols: &[&str]) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Unique {
            columns: cols.iter().map(|c| c.to_string()).collect(),
        },
        is_unique: true,
        ..btree_index(id, table, cols)
    }
}

fn filtered_index(
    id: &str,
    table: &str,
    cols: &[&str],
    pred: &str,
) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Filtered {
            columns: cols.iter().map(|c| c.to_string()).collect(),
            filter_predicate: pred.to_string(),
        },
        leaf_pages: 500,
        size_bytes: 5_000_000,
        ..btree_index(id, table, cols)
    }
}

fn composite_index(
    id: &str,
    table: &str,
    cols: &[&str],
    dirs: &[SortDirection],
) -> IndexMetadata {
    IndexMetadata {
        index_type: IndexType::Composite {
            columns: cols.iter().map(|c| c.to_string()).collect(),
            column_order: dirs.to_vec(),
        },
        ..btree_index(id, table, cols)
    }
}

fn s(val: &str) -> String {
    val.to_string()
}

// ===========================================================================
// 1. IndexType variant tests (11 types)
// ===========================================================================

#[test]
fn test_clustered_index_type() {
    let idx = clustered_index("pk_orders", "orders", &["id"]);
    assert!(matches!(idx.index_type, IndexType::Clustered { .. }));
    assert!(idx.is_primary);
    assert_eq!(idx.key_columns(), vec![&s("id")]);
    assert!(idx.supports_range_scan());
    assert!(idx.supports_equality_lookup());
    assert!(idx.supports_ordering());
}

#[test]
fn test_nonclustered_index_type() {
    let idx = btree_index("idx_email", "users", &["email"]);
    assert!(matches!(idx.index_type, IndexType::NonClustered { .. }));
    assert!(idx.supports_range_scan());
    assert!(idx.supports_equality_lookup());
    assert!(idx.supports_ordering());
}

#[test]
fn test_composite_index_type() {
    let idx = composite_index(
        "idx_comp",
        "orders",
        &["status", "date"],
        &[SortDirection::Ascending, SortDirection::Descending],
    );
    assert!(matches!(idx.index_type, IndexType::Composite { .. }));
    assert_eq!(idx.key_columns().len(), 2);
    assert!(idx.supports_range_scan());
    assert!(idx.supports_ordering());
}

#[test]
fn test_fulltext_index_type() {
    let idx = fulltext_index("idx_ft", "articles", &["title", "body"]);
    assert!(matches!(idx.index_type, IndexType::FullText { .. }));
    assert!(!idx.supports_equality_lookup());
    assert!(!idx.supports_range_scan());
    assert!(!idx.supports_ordering());
}

#[test]
fn test_unique_index_type() {
    let idx = unique_index("idx_uq", "users", &["email"]);
    assert!(matches!(idx.index_type, IndexType::Unique { .. }));
    assert!(idx.is_unique);
    assert!(idx.supports_range_scan());
    assert!(idx.supports_equality_lookup());
}

#[test]
fn test_filtered_index_type() {
    let idx = filtered_index(
        "idx_active",
        "orders",
        &["customer_id"],
        "status = 'active'",
    );
    assert!(matches!(idx.index_type, IndexType::Filtered { .. }));
    assert!(idx.supports_range_scan());
    assert!(idx.leaf_pages < 5000, "Filtered index should be smaller");
}

#[test]
fn test_spatial_index_type() {
    let idx = spatial_index("idx_geo", "parcels", "geom");
    assert!(matches!(idx.index_type, IndexType::Spatial { .. }));
    assert_eq!(idx.key_columns(), vec![&s("geom")]);
    assert!(!idx.supports_ordering());
    assert!(!idx.supports_range_scan());
}

#[test]
fn test_columnstore_index_type() {
    let idx = columnstore_index("idx_cs", "sales", &["region", "amount"]);
    assert!(matches!(idx.index_type, IndexType::Columnstore { .. }));
    assert_eq!(idx.key_columns().len(), 2);
    assert!(!idx.supports_ordering());
}

#[test]
fn test_hash_index_type() {
    let idx = hash_index("idx_hash", "sessions", &["token"]);
    assert!(matches!(idx.index_type, IndexType::Hash { .. }));
    assert!(idx.supports_equality_lookup());
    assert!(!idx.supports_range_scan());
    assert!(!idx.supports_ordering());
}

#[test]
fn test_gin_index_type() {
    let idx = gin_index("idx_gin", "events", "payload", "jsonb_ops");
    assert!(matches!(idx.index_type, IndexType::Gin { .. }));
    assert_eq!(idx.key_columns(), vec![&s("payload")]);
    assert!(idx.supports_equality_lookup());
}

#[test]
fn test_gist_index_type() {
    let idx = gist_index("idx_gist", "reservations", "period", "range_ops");
    assert!(matches!(idx.index_type, IndexType::Gist { .. }));
    assert_eq!(idx.key_columns(), vec![&s("period")]);
    assert!(idx.supports_range_scan());
}

// ===========================================================================
// 2. Cost model tests
// ===========================================================================

#[test]
fn test_default_cost_factors_vary_by_type() {
    let btree = btree_index("a", "t", &["x"]);
    let hash = hash_index("b", "t", &["x"]);
    let gin = gin_index("c", "t", "x", "ops");
    let gist = gist_index("d", "t", "x", "ops");
    let cs = columnstore_index("e", "t", &["x"]);
    let ft = fulltext_index("f", "t", &["x"]);
    let sp = spatial_index("g", "t", "x");

    let f_bt = btree.default_cost_factors();
    let f_hash = hash.default_cost_factors();
    let f_gin = gin.default_cost_factors();
    let f_gist = gist.default_cost_factors();
    let f_cs = cs.default_cost_factors();
    let f_ft = ft.default_cost_factors();
    let f_sp = sp.default_cost_factors();

    assert!((f_bt.type_multiplier - 1.0).abs() < f64::EPSILON);
    assert!(f_hash.type_multiplier < f_bt.type_multiplier);
    assert!(f_gin.type_multiplier > f_bt.type_multiplier);
    assert!(f_gist.type_multiplier > f_gin.type_multiplier);
    assert!(f_cs.type_multiplier < f_bt.type_multiplier);
    assert!(f_ft.type_multiplier > f_gist.type_multiplier);
    assert!(f_sp.type_multiplier > f_gist.type_multiplier);
}

#[test]
fn test_clustered_scan_has_zero_heap_io() {
    let idx = clustered_index("pk", "orders", &["id"]);
    let factors = IndexCostFactors::default();
    let cost = idx.estimate_scan_cost(0.05, 50_000, &factors);
    assert!(
        cost.heap_io_cost.abs() < f64::EPSILON,
        "Clustered index should have zero heap I/O, got {}",
        cost.heap_io_cost
    );
    assert!(cost.index_io_cost > 0.0);
    assert!(cost.cpu_cost > 0.0);
    assert!(cost.total_cost > 0.0);
}

#[test]
fn test_nonclustered_scan_has_heap_io() {
    let idx = btree_index("idx", "orders", &["email"]);
    let factors = IndexCostFactors::default();
    let cost = idx.estimate_scan_cost(0.01, 10_000, &factors);
    assert!(cost.heap_io_cost > 0.0, "NonClustered should have heap I/O");
    assert!(cost.index_io_cost > 0.0);
}

#[test]
fn test_higher_selectivity_means_higher_cost() {
    let idx = btree_index("idx", "orders", &["date"]);
    let factors = IndexCostFactors::default();
    let cost_1pct = idx.estimate_scan_cost(0.01, 10_000, &factors);
    let cost_50pct = idx.estimate_scan_cost(0.50, 10_000, &factors);
    assert!(
        cost_50pct.total_cost > cost_1pct.total_cost,
        "50% selectivity should be more expensive than 1%"
    );
}

#[test]
fn test_cache_hit_ratio_reduces_cost() {
    let idx = btree_index("idx", "orders", &["date"]);
    let no_cache = IndexCostFactors {
        cache_hit_ratio: 0.0,
        ..IndexCostFactors::default()
    };
    let high_cache = IndexCostFactors {
        cache_hit_ratio: 0.9,
        ..IndexCostFactors::default()
    };
    let cost_no_cache = idx.estimate_scan_cost(0.05, 10_000, &no_cache);
    let cost_cached = idx.estimate_scan_cost(0.05, 10_000, &high_cache);
    assert!(
        cost_cached.total_cost < cost_no_cache.total_cost,
        "High cache hit ratio should reduce cost"
    );
}

#[test]
fn test_columnstore_cost_factors() {
    let factors = IndexCostFactors::columnstore_index();
    assert!(
        factors.sequential_page_cost < 1.0,
        "Columnstore should have low seq I/O cost"
    );
    assert!(
        factors.cpu_index_tuple_cost < 0.005,
        "Columnstore batch mode should have low per-tuple cost"
    );
    assert!(
        (factors.cpu_heap_tuple_cost - 0.0).abs() < f64::EPSILON,
        "Columnstore has no separate heap"
    );
}

// ===========================================================================
// 3. Index selection algorithm tests
// ===========================================================================

#[test]
fn test_selection_prefers_clustered_for_range() {
    let clustered = clustered_index("pk", "orders", &["id"]);
    let secondary = btree_index("idx_id", "orders", &["id"]);
    let candidates = vec![clustered, secondary];

    let result = select_best_index(
        &candidates,
        &[s("id")],
        &[s("id")],
        false,
        0.05,
        10_000,
    );
    assert!(result.is_some());
    assert_eq!(
        result.map(|r| &r.index_id),
        Some(&s("pk")),
        "Should prefer clustered index for range on clustering key"
    );
}

#[test]
fn test_selection_picks_covering_index() {
    let narrow = btree_index("idx_narrow", "orders", &["customer_id"]);
    let mut covering = btree_index("idx_covering", "orders", &["customer_id"]);
    covering.index_type = IndexType::NonClustered {
        columns: vec![s("customer_id")],
        included_columns: vec![s("total"), s("status")],
    };
    let candidates = vec![narrow, covering];

    let result = select_best_index(
        &candidates,
        &[s("customer_id"), s("total"), s("status")],
        &[s("customer_id")],
        false,
        0.01,
        10_000,
    );
    assert!(result.is_some());
}

#[test]
fn test_selection_skips_invalid_indexes() {
    let mut invalid = btree_index("idx_broken", "orders", &["date"]);
    invalid.is_valid = false;
    let valid = btree_index("idx_ok", "orders", &["date"]);
    let candidates = vec![invalid, valid];

    let result = select_best_index(
        &candidates,
        &[s("date")],
        &[s("date")],
        false,
        0.01,
        10_000,
    );
    assert!(result.is_some());
    assert_eq!(result.map(|r| &r.index_id), Some(&s("idx_ok")));
}

#[test]
fn test_selection_requires_ordering_skips_hash() {
    let hash = hash_index("idx_hash", "users", &["email"]);
    let btree = btree_index("idx_btree", "users", &["email"]);
    let candidates = vec![hash, btree];

    let result = select_best_index(
        &candidates,
        &[s("email")],
        &[s("email")],
        true,
        0.01,
        10_000,
    );
    assert!(result.is_some());
    assert_eq!(
        result.map(|r| &r.index_id),
        Some(&s("idx_btree")),
        "Hash index cannot provide ordering"
    );
}

#[test]
fn test_selection_no_match_returns_none() {
    let idx = btree_index("idx_a", "orders", &["status"]);
    let candidates = vec![idx];

    let result = select_best_index(
        &candidates,
        &[s("date")],
        &[s("date")],
        false,
        0.01,
        10_000,
    );
    assert!(result.is_none(), "No index covers 'date' predicate");
}

#[test]
fn test_selection_empty_candidates() {
    let result = select_best_index(
        &[],
        &[s("id")],
        &[s("id")],
        false,
        0.01,
        10_000,
    );
    assert!(result.is_none());
}

#[test]
fn test_selection_all_invalid_returns_none() {
    let mut idx1 = btree_index("a", "t", &["x"]);
    idx1.is_valid = false;
    let mut idx2 = btree_index("b", "t", &["x"]);
    idx2.is_valid = false;
    let candidates = vec![idx1, idx2];

    let result = select_best_index(
        &candidates,
        &[s("x")],
        &[s("x")],
        false,
        0.01,
        10_000,
    );
    assert!(result.is_none());
}

#[test]
fn test_selection_prefers_lower_cost() {
    let small = IndexMetadata {
        leaf_pages: 100,
        size_bytes: 1_000_000,
        ..btree_index("idx_small", "orders", &["date"])
    };
    let large = IndexMetadata {
        leaf_pages: 10_000,
        size_bytes: 100_000_000,
        ..btree_index("idx_large", "orders", &["date"])
    };
    let candidates = vec![large, small];

    let result = select_best_index(
        &candidates,
        &[s("date")],
        &[s("date")],
        false,
        0.01,
        10_000,
    );
    assert!(result.is_some());
    assert_eq!(
        result.map(|r| &r.index_id),
        Some(&s("idx_small")),
        "Should prefer smaller/cheaper index"
    );
}

// ===========================================================================
// 4. Index recommendation structure test
// ===========================================================================

#[test]
fn test_index_recommendation_creation() {
    let rec = IndexRecommendation {
        index_type: IndexType::NonClustered {
            columns: vec![s("email")],
            included_columns: vec![],
        },
        table_name: s("users"),
        estimated_benefit: 0.75,
        reason: s("Frequent equality lookups on email column"),
    };
    assert!((rec.estimated_benefit - 0.75).abs() < f64::EPSILON);
    assert_eq!(rec.table_name, "users");
}

// ===========================================================================
// 5. Cost factor factory methods
// ===========================================================================

#[test]
fn test_postgresql_cost_factors() {
    let pg = IndexCostFactors::postgresql();
    assert!((pg.random_page_cost - 4.0).abs() < f64::EPSILON);
    assert!((pg.sequential_page_cost - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_gin_cost_factors() {
    let gin = IndexCostFactors::gin_index();
    assert!((gin.type_multiplier - 1.5).abs() < f64::EPSILON);
    assert!(gin.cpu_index_tuple_cost > IndexCostFactors::default().cpu_index_tuple_cost);
}

#[test]
fn test_gist_cost_factors() {
    let gist = IndexCostFactors::gist_index();
    assert!((gist.type_multiplier - 2.0).abs() < f64::EPSILON);
}

#[test]
fn test_fulltext_cost_factors() {
    let ft = IndexCostFactors::fulltext_index();
    assert!((ft.type_multiplier - 3.0).abs() < f64::EPSILON);
}

#[test]
fn test_spatial_cost_factors() {
    let sp = IndexCostFactors::spatial_index();
    assert!((sp.type_multiplier - 2.5).abs() < f64::EPSILON);
}
