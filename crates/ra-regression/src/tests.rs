//! Integration tests for regression detection.

#[cfg(test)]
mod integration_tests {
    use crate::*;
    use tempfile::tempdir;

    #[test]
    fn test_end_to_end_regression_detection() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("test.db");
        let storage = SqliteStorage::new(&storage_path);

        // Add baseline entry
        let baseline = QueryEntry::new(
            "test_query".to_string(),
            "SELECT * FROM users WHERE id = 1".to_string(),
            "hash123".to_string(),
            100.0,
        );
        storage.add_entry(baseline).unwrap();

        // Load history and detect regression with higher cost
        let history = storage.load().unwrap();
        let detector = RegressionDetector::new();

        // Use a simple fingerprint for testing (avoiding async DataFusion)
        let fingerprint = PlanFingerprint::from_plan(&datafusion::logical_expr::LogicalPlan::EmptyRelation(
            datafusion::logical_expr::EmptyRelation {
                produce_one_row: false,
                schema: std::sync::Arc::new(datafusion::common::DFSchema::empty()),
            },
        ));

        // Test error-level regression (2.5x cost increase)
        let report = detector.detect("test_query", 250.0, &fingerprint, &history);
        assert_eq!(report.severity, RegressionSeverity::Error);
        assert!(report.is_regression());
        assert_eq!(report.cost_ratio, Some(2.5));

        // Test warning-level regression (1.3x cost increase)
        let report = detector.detect("test_query", 130.0, &fingerprint, &history);
        assert_eq!(report.severity, RegressionSeverity::Warning);
        assert!(report.is_regression());
        assert_eq!(report.cost_ratio, Some(1.3));

        // Test improvement (0.5x cost)
        let report = detector.detect("test_query", 50.0, &fingerprint, &history);
        assert_eq!(report.severity, RegressionSeverity::Info);
        assert!(!report.is_regression());
        assert!(report.is_improvement());
        assert_eq!(report.cost_ratio, Some(0.5));
    }

    #[test]
    fn test_plan_structure_change_detection() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("test.db");
        let storage = SqliteStorage::new(&storage_path);

        // Add baseline with one plan hash
        let baseline = QueryEntry::new(
            "test_query".to_string(),
            "SELECT * FROM users".to_string(),
            "hash_old".to_string(),
            100.0,
        );
        storage.add_entry(baseline).unwrap();

        let history = storage.load().unwrap();
        let config = RegressionConfig {
            detect_plan_changes: true,
            ..Default::default()
        };
        let detector = RegressionDetector::with_config(config);

        // Create a different plan fingerprint (using a Sort node instead of EmptyRelation)
        let new_fingerprint = PlanFingerprint::from_plan(&datafusion::logical_expr::LogicalPlan::Sort(
            datafusion::logical_expr::Sort {
                expr: vec![],
                input: std::sync::Arc::new(datafusion::logical_expr::LogicalPlan::EmptyRelation(
                    datafusion::logical_expr::EmptyRelation {
                        produce_one_row: false,
                        schema: std::sync::Arc::new(datafusion::common::DFSchema::empty()),
                    },
                )),
                fetch: None,
            },
        ));

        // Detect with same cost but different plan
        let report = detector.detect("test_query", 100.0, &new_fingerprint, &history);
        assert!(report.plan_changed);
        assert_eq!(report.severity, RegressionSeverity::Info);
        assert!(report.description.contains("Plan changed"));
    }

    #[test]
    fn test_historical_average_detection() {
        let mut history = CostHistory::new();

        // Add multiple historical entries
        for i in 0..5 {
            let entry = QueryEntry::new(
                "test_query".to_string(),
                "SELECT * FROM users".to_string(),
                "hash123".to_string(),
                100.0 + i as f64 * 10.0, // 100, 110, 120, 130, 140
            );
            history.add_entry(entry);
        }

        let detector = RegressionDetector::new();
        let fingerprint = PlanFingerprint::from_plan(&datafusion::logical_expr::LogicalPlan::EmptyRelation(
            datafusion::logical_expr::EmptyRelation {
                produce_one_row: false,
                schema: std::sync::Arc::new(datafusion::common::DFSchema::empty()),
            },
        ));

        // Current cost significantly above average
        let report = detector.detect("test_query", 300.0, &fingerprint, &history);
        assert!(report.is_regression());

        // Check historical average is calculated correctly
        let avg = history.get_average_cost("test_query", 10).unwrap();
        assert_eq!(avg, 120.0); // (100+110+120+130+140)/5
    }

    #[test]
    fn test_toml_storage_roundtrip() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("test.toml");
        let storage = TomlStorage::new(&storage_path);

        // Create entries
        let entry1 = QueryEntry::new(
            "q1".to_string(),
            "SELECT * FROM t1".to_string(),
            "hash1".to_string(),
            100.0,
        );
        let entry2 = QueryEntry::new(
            "q2".to_string(),
            "SELECT * FROM t2".to_string(),
            "hash2".to_string(),
            200.0,
        );

        // Store entries
        storage.add_entry(entry1.clone()).unwrap();
        storage.add_entry(entry2.clone()).unwrap();

        // Load and verify
        let history = storage.load().unwrap();
        assert_eq!(history.get_entries("q1").unwrap().len(), 1);
        assert_eq!(history.get_entries("q2").unwrap().len(), 1);
        assert_eq!(history.get_latest("q1").unwrap().cost, 100.0);
        assert_eq!(history.get_latest("q2").unwrap().cost, 200.0);
    }

    #[test]
    fn test_cost_history_pruning() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("test.db");
        let storage = SqliteStorage::new(&storage_path);

        // Add many entries
        for i in 0..10 {
            let entry = QueryEntry::new(
                "q1".to_string(),
                "SELECT * FROM t".to_string(),
                "hash1".to_string(),
                100.0 + i as f64,
            );
            storage.add_entry(entry).unwrap();
        }

        // Load, prune, and save
        let mut history = storage.load().unwrap();
        assert_eq!(history.get_entries("q1").unwrap().len(), 10);

        history.prune(5);
        assert_eq!(history.get_entries("q1").unwrap().len(), 5);

        // Verify the most recent entries are kept
        let entries = history.get_entries("q1").unwrap();
        assert_eq!(entries[0].cost, 105.0); // Should keep entries 5-9
        assert_eq!(entries[4].cost, 109.0);
    }
}