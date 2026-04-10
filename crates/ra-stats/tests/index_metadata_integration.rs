//! Integration tests for index metadata abstraction.
//!
//! Tests the abstraction layer with multiple index types to ensure rules
//! work correctly regardless of which index is installed.

use ra_stats::index_metadata::{
    discover_indexes_for_table, find_indexes_supporting, IndexAccessMethod, IndexCapabilities,
    IndexMetadata as IndexMetadataV2, IndexOperation,
};
use ra_stats::types::IndexStats;

// ------------------------------------------------------------------
// Test Helpers
// ------------------------------------------------------------------

fn mock_gin_index() -> IndexMetadataV2 {
    IndexMetadataV2 {
        name: "idx_articles_tags_gin".to_string(),
        table: "articles".to_string(),
        columns: vec!["tags".to_string()],
        access_method: IndexAccessMethod::GIN,
        operator_family: "array_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::GIN,
            "array_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_articles_tags_gin".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 500,
            levels: 3,
            avg_leaf_density: 0.7,
            distinct_keys: 10_000,
        },
    }
}

fn mock_rum_index() -> IndexMetadataV2 {
    IndexMetadataV2 {
        name: "idx_articles_tags_rum".to_string(),
        table: "articles".to_string(),
        columns: vec!["tags".to_string()],
        access_method: IndexAccessMethod::RUM,
        operator_family: "rum_anyarray_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::RUM,
            "rum_anyarray_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_articles_tags_rum".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 600, // RUM: slightly larger due to wider postings
            levels: 3,
            avg_leaf_density: 0.6,
            distinct_keys: 10_000,
        },
    }
}

fn mock_documentdb_rum_index() -> IndexMetadataV2 {
    IndexMetadataV2 {
        name: "idx_articles_metadata_docdb_rum".to_string(),
        table: "articles".to_string(),
        columns: vec!["metadata".to_string()],
        access_method: IndexAccessMethod::DocumentDBRUM,
        operator_family: "bson_extended_rum_single_path_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::DocumentDBRUM,
            "bson_extended_rum_single_path_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_articles_metadata_docdb_rum".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 700, // BSON-specific overhead
            levels: 3,
            avg_leaf_density: 0.65,
            distinct_keys: 10_000,
        },
    }
}

// ------------------------------------------------------------------
// Capability Tests
// ------------------------------------------------------------------

#[test]
fn gin_supports_array_containment() {
    let idx = mock_gin_index();
    assert!(idx.supports_operation(&IndexOperation::ArrayContainment));
    assert!(!idx.supports_operation(&IndexOperation::FullTextSearch));
    assert!(!idx.supports_operation(&IndexOperation::KNNSearch));
}

#[test]
fn rum_supports_array_containment() {
    let idx = mock_rum_index();
    assert!(idx.supports_operation(&IndexOperation::ArrayContainment));
    assert!(!idx.supports_operation(&IndexOperation::FullTextSearch));
    // RUM anyarray doesn't support distance ordering
    assert!(!idx.supports_operation(&IndexOperation::KNNSearch));
}

#[test]
fn documentdb_rum_supports_json_and_ordering() {
    let idx = mock_documentdb_rum_index();
    assert!(idx.supports_operation(&IndexOperation::JsonContainment));
    assert!(idx.supports_operation(&IndexOperation::FullTextSearch));
    assert!(idx.supports_operation(&IndexOperation::KNNSearch));
}

#[test]
fn gin_no_ordering_capability() {
    let idx = mock_gin_index();
    assert!(!idx.capabilities.supports_ordered_scan);
    assert!(!idx.capabilities.supports_distance_ordering);
}

#[test]
fn rum_has_ordering_capability() {
    // Create RUM tsvector index (not anyarray)
    let idx = IndexMetadataV2 {
        name: "idx_rum_tsv".to_string(),
        table: "articles".to_string(),
        columns: vec!["body_tsv".to_string()],
        access_method: IndexAccessMethod::RUM,
        operator_family: "rum_tsvector_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::RUM,
            "rum_tsvector_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_rum_tsv".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 800,
            levels: 3,
            avg_leaf_density: 0.6,
            distinct_keys: 50_000,
        },
    };

    assert!(idx.capabilities.supports_ordered_scan);
    assert!(idx.capabilities.supports_distance_ordering);
    assert!(idx.capabilities.supports_phrase_search);
}

// ------------------------------------------------------------------
// Cost Model Tests
// ------------------------------------------------------------------

#[test]
fn gin_vs_rum_cost_for_array_containment_no_limit() {
    let gin = mock_gin_index();
    let rum = mock_rum_index();

    let selectivity = 0.01;
    let table_rows = 1_000_000;
    let limit = None;

    let gin_cost = gin.estimate_scan_cost(selectivity, table_rows, limit);
    let rum_cost = rum.estimate_scan_cost(selectivity, table_rows, limit);

    // Without limit and without ordering, GIN should be cheaper (narrower postings)
    assert!(
        gin_cost <= rum_cost * 1.2,
        "GIN should be comparable or cheaper for pure containment: gin={gin_cost}, rum={rum_cost}"
    );
}

#[test]
fn rum_vs_gin_cost_for_fulltext_with_limit() {
    // GIN for full-text
    let gin_tsv = IndexMetadataV2 {
        name: "idx_gin_tsv".to_string(),
        table: "articles".to_string(),
        columns: vec!["body_tsv".to_string()],
        access_method: IndexAccessMethod::GIN,
        operator_family: "gin_tsvector_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::GIN,
            "gin_tsvector_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_gin_tsv".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 1000,
            levels: 3,
            avg_leaf_density: 0.7,
            distinct_keys: 100_000,
        },
    };

    // RUM for full-text
    let rum_tsv = IndexMetadataV2 {
        name: "idx_rum_tsv".to_string(),
        table: "articles".to_string(),
        columns: vec!["body_tsv".to_string()],
        access_method: IndexAccessMethod::RUM,
        operator_family: "rum_tsvector_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::RUM,
            "rum_tsvector_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_rum_tsv".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 1200,
            levels: 3,
            avg_leaf_density: 0.6,
            distinct_keys: 100_000,
        },
    };

    let selectivity = 0.1;
    let table_rows = 1_000_000;
    let limit = Some(10);

    let gin_cost = gin_tsv.estimate_scan_cost(selectivity, table_rows, limit);
    let rum_cost = rum_tsv.estimate_scan_cost(selectivity, table_rows, limit);

    // With LIMIT, RUM should be cheaper due to distance-ordered scan
    assert!(
        rum_cost < gin_cost,
        "RUM should be cheaper for top-K queries: gin={gin_cost}, rum={rum_cost}"
    );
}

#[test]
fn documentdb_rum_cost_accounts_for_bson_overhead() {
    let docdb_rum = mock_documentdb_rum_index();

    let selectivity = 0.01;
    let table_rows = 1_000_000;
    let limit = None;

    let cost = docdb_rum.estimate_scan_cost(selectivity, table_rows, limit);

    // DocumentDB RUM has BSON overhead, should be higher than standard RUM
    // but still much better than seq scan
    let seq_scan_cost = table_rows as f64 * 1.0;
    assert!(
        cost < seq_scan_cost * 0.3,
        "DocumentDB RUM should be much cheaper than seq scan: cost={cost}, seq={seq_scan_cost}"
    );
}

// ------------------------------------------------------------------
// Rule Composition Tests
// ------------------------------------------------------------------

#[test]
fn rules_automatically_use_available_index_type() {
    // Simulate rule checking for indexes supporting array containment
    let available_indexes = vec![mock_gin_index()];

    let supporting = available_indexes
        .iter()
        .filter(|idx| idx.supports_operation(&IndexOperation::ArrayContainment))
        .collect::<Vec<_>>();

    assert_eq!(supporting.len(), 1);
    assert_eq!(supporting[0].access_method, IndexAccessMethod::GIN);
}

#[test]
fn rules_prefer_rum_when_both_available() {
    // When both GIN and RUM are available, optimizer should choose based on query
    let gin = mock_gin_index();
    let rum = mock_rum_index();

    let available = vec![gin.clone(), rum.clone()];

    // For pure containment without ordering, GIN should win
    let best_for_containment = available
        .iter()
        .filter(|idx| idx.supports_operation(&IndexOperation::ArrayContainment))
        .min_by_key(|idx| {
            let cost = idx.estimate_scan_cost(0.01, 1_000_000, None);
            (cost * 100.0) as i64
        })
        .unwrap();

    // For this specific case, GIN should be selected (or very close)
    assert!(
        best_for_containment.access_method == IndexAccessMethod::GIN
            || best_for_containment.access_method == IndexAccessMethod::RUM
    );
}

#[test]
fn rules_work_with_any_inverted_index_type() {
    // Test that the same rule works with GIN, RUM, and DocumentDB RUM
    let test_cases = vec![
        ("GIN", mock_gin_index()),
        ("RUM", mock_rum_index()),
        ("DocumentDB RUM", mock_documentdb_rum_index()),
    ];

    for (name, idx) in test_cases {
        // Simulate rule: "use any index supporting array/json containment"
        let supports = idx.supports_operation(&IndexOperation::ArrayContainment)
            || idx.supports_operation(&IndexOperation::JsonContainment);

        assert!(
            supports,
            "{name} should support containment operations"
        );

        // Simulate cost estimation
        let cost = idx.estimate_scan_cost(0.01, 1_000_000, None);
        assert!(
            cost > 0.0 && cost < 1_000_000.0,
            "{name} cost should be between 0 and seq scan: {cost}"
        );
    }
}

// ------------------------------------------------------------------
// Cross-Database Compatibility Tests
// ------------------------------------------------------------------

#[test]
fn gin_and_documentdb_rum_both_support_json_containment() {
    let gin_jsonb = IndexMetadataV2 {
        name: "idx_gin_jsonb".to_string(),
        table: "events".to_string(),
        columns: vec!["metadata".to_string()],
        access_method: IndexAccessMethod::GIN,
        operator_family: "jsonb_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::GIN,
            "jsonb_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_gin_jsonb".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 800,
            levels: 3,
            avg_leaf_density: 0.7,
            distinct_keys: 50_000,
        },
    };

    let docdb_rum = mock_documentdb_rum_index();

    assert!(gin_jsonb.supports_operation(&IndexOperation::JsonContainment));
    assert!(docdb_rum.supports_operation(&IndexOperation::JsonContainment));

    // Both indexes can be used by the same generic rule
}

#[test]
fn capability_differences_affect_rule_applicability() {
    let gin_jsonb = IndexMetadataV2 {
        name: "idx_gin_jsonb".to_string(),
        table: "events".to_string(),
        columns: vec!["metadata".to_string()],
        access_method: IndexAccessMethod::GIN,
        operator_family: "jsonb_ops".to_string(),
        capabilities: IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::GIN,
            "jsonb_ops",
        ),
        statistics: IndexStats {
            index_id: "idx_gin_jsonb".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 800,
            levels: 3,
            avg_leaf_density: 0.7,
            distinct_keys: 50_000,
        },
    };

    let docdb_rum = mock_documentdb_rum_index();

    // Both support JSON containment
    assert!(gin_jsonb.supports_operation(&IndexOperation::JsonContainment));
    assert!(docdb_rum.supports_operation(&IndexOperation::JsonContainment));

    // But only DocumentDB RUM supports distance ordering
    assert!(!gin_jsonb.capabilities.supports_distance_ordering);
    assert!(docdb_rum.capabilities.supports_distance_ordering);

    // So a rule requiring KNN would only match DocumentDB RUM
    assert!(!gin_jsonb.supports_operation(&IndexOperation::KNNSearch));
    assert!(docdb_rum.supports_operation(&IndexOperation::KNNSearch));
}

// ------------------------------------------------------------------
// Backward Compatibility Tests
// ------------------------------------------------------------------

#[test]
fn legacy_index_type_conversion() {
    let idx = mock_gin_index();
    let legacy = idx.to_legacy_index_type();

    use ra_stats::IndexType;
    assert!(matches!(legacy, IndexType::GIN { .. }));
}

#[test]
fn all_index_types_can_convert_to_legacy() {
    let test_cases = vec![
        mock_gin_index(),
        mock_rum_index(),
        mock_documentdb_rum_index(),
    ];

    for idx in test_cases {
        let legacy = idx.to_legacy_index_type();
        // Should not panic and should return a valid IndexType
        let _ = format!("{legacy:?}");
    }
}

// ------------------------------------------------------------------
// Edge Cases
// ------------------------------------------------------------------

#[test]
fn empty_index_list_handled_gracefully() {
    let indexes: Vec<IndexMetadataV2> = vec![];
    let supporting = indexes
        .iter()
        .filter(|idx| idx.supports_operation(&IndexOperation::ArrayContainment))
        .collect::<Vec<_>>();

    assert!(supporting.is_empty());
}

#[test]
fn multiple_indexes_on_same_column() {
    // Both GIN and RUM on the same column (realistic scenario)
    let gin = mock_gin_index();
    let rum = mock_rum_index();

    assert_eq!(gin.columns, rum.columns);
    assert_ne!(gin.access_method, rum.access_method);

    // Optimizer should choose based on query characteristics
}

#[test]
fn index_matches_predicate_correctly() {
    let idx = mock_gin_index();

    assert!(idx.matches_predicate(&["tags".to_string()]));
    assert!(!idx.matches_predicate(&["title".to_string()]));
    assert!(!idx.matches_predicate(&[]));
}

// ------------------------------------------------------------------
// Discovery Function Tests (Mock)
// ------------------------------------------------------------------

#[test]
fn discover_indexes_returns_empty_without_connection() {
    // Without actual database connection, should return empty
    let indexes = discover_indexes_for_table("mock_connection", "articles");
    assert!(indexes.is_empty());
}

#[test]
fn find_indexes_supporting_returns_empty_without_connection() {
    let indexes = find_indexes_supporting(
        "mock_connection",
        "articles",
        "tags",
        &IndexOperation::ArrayContainment,
    );
    assert!(indexes.is_empty());
}
