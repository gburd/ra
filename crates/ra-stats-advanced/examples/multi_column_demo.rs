//! Multi-column statistics demo: handling correlated columns.
//!
//! Demonstrates the dramatic improvement in cardinality estimation accuracy
//! when using multi-column statistics for correlated columns vs the
//! independence assumption.
#![expect(clippy::print_stdout, reason = "example binary uses stdout")]

use ra_stats::multi_column::{MultiColumnConfig, MultiColumnEstimator};
use ra_stats::types::{MultiColumnStats, MultiDimHistogram};

fn main() {
    println!("Multi-Column Statistics Demo");
    println!("=============================\n");

    // Scenario: E-commerce database with correlated columns
    // - city and state are highly correlated (100 cities, 50 states, but only 100 unique pairs)
    // - product and category are moderately correlated (1000 products, 20 categories, 800 pairs)

    demo_exact_match();
    demo_prefix_match();
    demo_superset_match();
    demo_with_histogram();
    demo_configuration_presets();
    demo_q_error_comparison();
}

fn demo_exact_match() {
    println!("1. Exact Match: Query columns exactly match tracked statistic");
    println!("--------------------------------------------------------------");

    let mut estimator = MultiColumnEstimator::with_defaults();

    // Track city+state correlation
    let stats = MultiColumnStats {
        columns: vec!["city".to_string(), "state".to_string()],
        distinct_count: 100,
        total_rows: 1_000_000,
        correlation_matrix: vec![0.95],
        histogram: None,
    };
    estimator.add_stats(stats);

    let columns = vec!["city".to_string(), "state".to_string()];
    let individual_ndvs = vec![100, 50];

    let multi_estimate = estimator.estimate_cardinality(&columns, &individual_ndvs, 1_000_000);
    let independence_estimate = 100 * 50; // Wrong assumption

    println!(
        "  Individual NDVs: city={}, state={}",
        individual_ndvs[0], individual_ndvs[1]
    );
    println!("  Independence assumption: {independence_estimate} distinct pairs");
    println!("  Multi-column statistics: {multi_estimate} distinct pairs");
    println!("  Actual distinct pairs:   100");
    println!(
        "  Q-error (independence):  {:.1}x",
        f64::from(independence_estimate) / 100.0
    );
    println!(
        "  Q-error (multi-column):  {:.1}x\n",
        multi_estimate as f64 / 100.0
    );
}

fn demo_prefix_match() {
    println!("2. Prefix Match: Query is prefix of tracked statistic");
    println!("--------------------------------------------------------");

    let mut estimator = MultiColumnEstimator::with_defaults();

    // Track city+state+zip correlation
    let stats = MultiColumnStats {
        columns: vec!["city".to_string(), "state".to_string(), "zip".to_string()],
        distinct_count: 1000,
        total_rows: 10_000_000,
        correlation_matrix: vec![0.95, 0.98, 0.97],
        histogram: None,
    };
    estimator.add_stats(stats);

    // Query only city+state
    let columns = vec!["city".to_string(), "state".to_string()];
    let individual_ndvs = vec![500, 50];

    let multi_estimate = estimator.estimate_cardinality(&columns, &individual_ndvs, 10_000_000);
    let independence_estimate = 500 * 50;

    println!("  Query columns: city, state");
    println!("  Tracked stats: city, state, zip");
    println!("  Independence assumption: {independence_estimate} pairs");
    println!("  Multi-column statistics: {multi_estimate} pairs");
    println!(
        "  Improvement: {:.1}x better\n",
        f64::from(independence_estimate) / multi_estimate as f64
    );
}

fn demo_superset_match() {
    println!("3. Superset Match: Tracked stats are subset of query");
    println!("-------------------------------------------------------");

    let mut estimator = MultiColumnEstimator::with_defaults();

    // Track product+category correlation
    let stats = MultiColumnStats {
        columns: vec!["product".to_string(), "category".to_string()],
        distinct_count: 800,
        total_rows: 5_000_000,
        correlation_matrix: vec![0.6],
        histogram: None,
    };
    estimator.add_stats(stats);

    // Query includes additional column (brand)
    let columns = vec![
        "product".to_string(),
        "category".to_string(),
        "brand".to_string(),
    ];
    let individual_ndvs = vec![1000, 20, 100];

    let multi_estimate = estimator.estimate_cardinality(&columns, &individual_ndvs, 5_000_000);
    let independence_estimate = 1000 * 20 * 100;

    println!("  Query columns: product, category, brand");
    println!("  Tracked stats: product, category");
    println!("  Independence assumption: {independence_estimate} combinations");
    println!("  Blended estimate:        {multi_estimate} combinations");
    println!("  (Uses partial match + independence for brand)\n");
}

fn demo_with_histogram() {
    println!("4. Multi-Dimensional Histogram");
    println!("--------------------------------");

    let mut estimator = MultiColumnEstimator::with_defaults();

    // 2D histogram for age ranges vs income brackets
    let hist = MultiDimHistogram::new(
        vec!["age".to_string(), "income".to_string()],
        vec![
            vec![0.0, 20.0, 40.0, 60.0, 80.0],
            vec![0.0, 30_000.0, 60_000.0, 100_000.0],
        ],
        vec![
            500, 1200, 800, // age 0-20
            2000, 5000, 3000, // age 20-40
            1500, 4500, 4000, // age 40-60
            400, 1000, 600, // age 60-80
        ],
    );

    let total_rows = hist.total_rows;
    let total_cells = hist.total_cells();
    let stats = MultiColumnStats {
        columns: vec!["age".to_string(), "income".to_string()],
        distinct_count: 500,
        total_rows,
        correlation_matrix: vec![0.7],
        histogram: Some(hist),
    };
    let distinct = stats.distinct_count;
    estimator.add_stats(stats);

    println!("  Columns: age, income");
    println!("  Dimensions: 4x3 grid");
    println!("  Total rows: {total_rows}");
    println!("  Distinct combinations: {distinct}");
    println!("  Histogram cells: {total_cells}\n");
}

fn demo_configuration_presets() {
    println!("5. Configuration Presets");
    println!("-------------------------");

    let configs = vec![
        ("Default", MultiColumnConfig::default()),
        ("Aggressive", MultiColumnConfig::aggressive()),
        ("Minimal", MultiColumnConfig::minimal()),
        ("Disabled", MultiColumnConfig::disabled()),
    ];

    for (name, config) in configs {
        println!(
            "  {:<12} enabled={:<5} max_cols={} min_corr={:.2} min_improve={:.1}",
            name,
            config.enabled,
            config.max_column_combinations,
            config.min_correlation_threshold,
            config.min_improvement_factor
        );
    }
    println!();
}

fn demo_q_error_comparison() {
    println!("6. Q-Error Comparison: Real vs Estimated Cardinalities");
    println!("--------------------------------------------------------");

    let scenarios = vec![
        ("city+state (strong)", 100, 50, 100, 0.95),
        ("zip+city (very strong)", 1000, 500, 1000, 0.99),
        ("product+category (moderate)", 1000, 20, 800, 0.6),
        ("order_id+customer (weak)", 10000, 5000, 9000, 0.2),
    ];

    println!(
        "  {:<30} {:<10} {:<10} {:<10} {:<12} {:<12}",
        "Scenario", "Indep", "Multi", "Actual", "Q-Err Indep", "Q-Err Multi"
    );
    println!(
        "  {:-<30} {:-<10} {:-<10} {:-<10} {:-<12} {:-<12}",
        "", "", "", "", "", ""
    );

    for (name, ndv1, ndv2, actual, corr) in scenarios {
        let mut estimator = MultiColumnEstimator::with_defaults();

        let stats = MultiColumnStats {
            columns: vec!["col1".to_string(), "col2".to_string()],
            distinct_count: actual,
            total_rows: 1_000_000,
            correlation_matrix: vec![corr],
            histogram: None,
        };
        estimator.add_stats(stats);

        let independence = ndv1 * ndv2;
        let multi = estimator.estimate_cardinality(
            &["col1".to_string(), "col2".to_string()],
            &[ndv1, ndv2],
            1_000_000,
        );

        let q_err_indep =
            (independence as f64 / actual as f64).max(actual as f64 / independence as f64);
        let q_err_multi = (multi as f64 / actual as f64).max(actual as f64 / multi as f64);

        println!(
            "  {name:<30} {independence:<10} {multi:<10} {actual:<10} {q_err_indep:<12.1}x {q_err_multi:<12.1}x",
        );
    }

    println!("\n  Q-Error: max(estimate/actual, actual/estimate)");
    println!("  Lower is better. 1.0 = perfect estimate.\n");
}
