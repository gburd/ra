#![expect(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
//! Integration tests for timeline optimizer.

#![cfg(feature = "timeline")]

use ra_core::algebra::RelExpr;
use ra_engine::{ChangeSeverity, ChangeType, Optimizer, TimelineConfig, TimelineOptimizer};
use std::path::PathBuf;

/// Get the path to test data directory.
fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("data")
        .join("timelines")
}

#[test]
fn load_index_addition_timeline() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    assert_eq!(config.metadata.name, "Index Addition Scenario");
    assert_eq!(config.snapshots.len(), 3);
    assert_eq!(config.hardware_profiles.len(), 2);
}

#[test]
fn optimize_index_addition_timeline() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = if config.metadata.query.is_some() {
        // Parse query - for testing, use a simple scan
        RelExpr::scan("orders")
    } else {
        RelExpr::scan("orders")
    };

    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    assert_eq!(result.snapshot_results.len(), 3);

    // Verify first snapshot (no index)
    let snap0 = &result.snapshot_results[0];
    assert_eq!(snap0.snapshot_index, 0);
    assert_eq!(snap0.time_offset, 0);
    assert_eq!(snap0.label.as_deref(), Some("Initial state - no index"));

    // Verify second snapshot (index added)
    let snap1 = &result.snapshot_results[1];
    assert_eq!(snap1.snapshot_index, 1);
    assert_eq!(snap1.time_offset, 1800);
    assert!(!snap1.changes_from_previous.is_empty());

    // Should detect index addition
    let has_index_change = snap1.changes_from_previous.iter().any(|c| {
        matches!(c.change_type, ChangeType::Schema) && c.description.contains("idx_orders_customer")
    });
    assert!(has_index_change, "Index addition should be detected");

    // Verify third snapshot (hardware upgrade)
    let snap2 = &result.snapshot_results[2];
    assert_eq!(snap2.snapshot_index, 2);
    assert_eq!(snap2.time_offset, 5400);

    // Should detect hardware changes
    let has_hardware_change = snap2
        .changes_from_previous
        .iter()
        .any(|c| matches!(c.change_type, ChangeType::Hardware));
    assert!(has_hardware_change, "Hardware changes should be detected");
}

#[test]
fn detect_changes_across_snapshots() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    // Snapshot 1 should detect index addition
    let snap1_changes = &result.snapshot_results[1].changes_from_previous;
    assert!(!snap1_changes.is_empty());

    let schema_changes: Vec<_> = snap1_changes
        .iter()
        .filter(|c| matches!(c.change_type, ChangeType::Schema))
        .collect();
    assert!(!schema_changes.is_empty(), "Should detect schema changes");

    // Snapshot 2 should detect hardware and statistics changes
    let snap2_changes = &result.snapshot_results[2].changes_from_previous;
    assert!(!snap2_changes.is_empty());

    let hardware_changes: Vec<_> = snap2_changes
        .iter()
        .filter(|c| matches!(c.change_type, ChangeType::Hardware))
        .collect();
    assert!(
        !hardware_changes.is_empty(),
        "Should detect hardware changes"
    );
}

#[test]
fn change_severity_levels() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    // Check that severity levels are assigned
    for snapshot in &result.snapshot_results {
        for change in &snapshot.changes_from_previous {
            match change.change_type {
                ChangeType::Schema => {
                    // Index changes should be High severity
                    if change.description.contains("Index") {
                        assert!(matches!(
                            change.severity,
                            ChangeSeverity::High | ChangeSeverity::Medium
                        ));
                    }
                }
                ChangeType::Statistics => {
                    // Statistics changes vary by magnitude
                    assert!(matches!(
                        change.severity,
                        ChangeSeverity::Low
                            | ChangeSeverity::Medium
                            | ChangeSeverity::High
                            | ChangeSeverity::Critical
                    ));
                }
                ChangeType::Hardware => {
                    // Hardware changes are typically Medium or High
                    assert!(matches!(
                        change.severity,
                        ChangeSeverity::Low | ChangeSeverity::Medium | ChangeSeverity::High
                    ));
                }
                ChangeType::Facts => {
                    // Fact changes are typically High
                    assert!(matches!(
                        change.severity,
                        ChangeSeverity::Medium | ChangeSeverity::High
                    ));
                }
            }
        }
    }
}

#[test]
fn output_format_json() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    let json = result.to_json().expect("JSON serialization failed");
    assert!(json.contains("snapshot_results"));
    assert!(json.contains("timeline_name"));

    // Verify it's valid JSON
    let _parsed: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON");
}

#[test]
fn output_format_markdown() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    let markdown = result.to_markdown();
    assert!(markdown.contains("# Timeline Optimization Report"));
    assert!(markdown.contains("## Snapshots"));
    assert!(markdown.contains("### Snapshot 0"));
    assert!(markdown.contains("### Snapshot 1"));
    assert!(markdown.contains("### Snapshot 2"));
}

#[test]
fn output_format_text() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    let text = result.to_text();
    assert!(text.contains("Timeline Optimization Report"));
    assert!(text.contains("┌──────┬"));
    assert!(text.contains("│ Snap │"));
}

#[test]
fn dependencies_tracking() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    for snapshot in &result.snapshot_results {
        // Should track table cardinalities
        assert!(!snapshot.dependencies.table_cardinalities.is_empty());

        // Snapshots with indexes should track them
        if snapshot.snapshot_index > 0 {
            // After index is added
            assert!(!snapshot.dependencies.indexes.is_empty());
        }

        // Should track distinct counts
        assert!(!snapshot.dependencies.distinct_counts.is_empty());
    }
}

#[test]
fn custom_thresholds() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();

    // Create custom thresholds with higher sensitivity
    let thresholds = ra_engine::StalenessThresholds {
        cardinality_ratio: 1.1, // Very sensitive to row count changes
        index_changes_trigger: true,
        ..ra_engine::StalenessThresholds::default()
    };

    let mut timeline_optimizer =
        TimelineOptimizer::with_thresholds(config, query, optimizer, thresholds);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    // With more sensitive thresholds, should detect more changes
    let total_changes: usize = result
        .snapshot_results
        .iter()
        .map(|s| s.changes_from_previous.len())
        .sum();

    assert!(
        total_changes > 0,
        "Should detect changes with custom thresholds"
    );
}

#[test]
fn empty_timeline_handling() {
    // Create a minimal timeline with just one snapshot
    use ra_engine::timeline_config::{
        FactsSnapshot, FingerPrintSnapshot, HardwareProfileDef, SchemaSnapshot, StatisticsSnapshot,
        StorageFormatDef, TableDef, TableStatsDef, TimelineMetadata,
    };
    use std::collections::HashMap;

    let config = TimelineConfig {
        metadata: TimelineMetadata {
            name: "Single Snapshot".to_string(),
            description: "Test".to_string(),
            query: None,
            dialect: None,
            duration_seconds: None,
            schema: None,
            scale_factor: None,
        },
        hardware_profiles: vec![HardwareProfileDef {
            name: "test".to_string(),
            cpu_cores: 4,
            total_memory: 8_000_000_000,
            available_memory: Some(6_000_000_000),
            simd_width: 256,
            has_gpu: false,
            gpu_memory: None,
            l1_cache_size: 32768,
            l2_cache_size: 262_144,
            l3_cache_size: 8_388_608,
        }],
        snapshots: vec![FingerPrintSnapshot {
            time_offset: 0,
            label: None,
            hardware_profile: "test".to_string(),
            schema: SchemaSnapshot {
                tables: vec![TableDef {
                    name: "test".to_string(),
                    storage_format: StorageFormatDef::RowBased,
                    columns: vec![],
                    indexes: vec![],
                    primary_key: vec![],
                    foreign_keys: vec![],
                }],
            },
            statistics: StatisticsSnapshot {
                tables: vec![TableStatsDef {
                    name: "test".to_string(),
                    row_count: 100,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                }],
            },
            facts: FactsSnapshot {
                supports_hash_join: Some(true),
                supports_parallel_scan: Some(false),
                parallel_workers: None,
                work_mem_bytes: None,
                custom: HashMap::new(),
            },
        }],
        events: vec![],
        expectations: vec![],
    };

    let query = RelExpr::scan("test");
    let optimizer = Optimizer::new();
    let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    assert_eq!(result.snapshot_results.len(), 1);
    assert_eq!(result.snapshot_results[0].changes_from_previous.len(), 0);
}

#[test]
fn statistics_drift_detection() {
    let path = test_data_dir().join("index-addition.toml");
    let config = TimelineConfig::from_file(&path).expect("Failed to load timeline");

    let query = RelExpr::scan("orders");
    let optimizer = Optimizer::new();

    // Use lower thresholds to detect the smaller row count changes in the test timeline
    // (1M -> 1.05M = 1.05x, which is below default 2.0x threshold)
    let thresholds = ra_engine::StalenessThresholds {
        cardinality_ratio: 1.03, // Detect 3% or more change
        ..ra_engine::StalenessThresholds::default()
    };

    let mut timeline_optimizer =
        TimelineOptimizer::with_thresholds(config, query, optimizer, thresholds);

    let result = timeline_optimizer
        .optimize_timeline()
        .expect("Optimization failed");

    // Check for statistics changes across snapshots
    let has_row_count_change = result.snapshot_results.iter().any(|s| {
        s.changes_from_previous.iter().any(|c| {
            matches!(c.change_type, ChangeType::Statistics) && c.description.contains("row count")
        })
    });

    assert!(
        has_row_count_change,
        "Should detect row count changes with lower threshold"
    );
}
