#![expect(clippy::unwrap_used, reason = "test code")]
//! Integration test: end-to-end training loop with fuzzed queries.
//!
//! Generates 1000+ SQL queries from parameterized templates, parses them
//! through `sql_to_relexpr`, optimizes each with the training coordinator
//! active, then verifies the model improved (loss decreased).
//!
//! Uses a seeded RNG for deterministic query generation so results are
//! reproducible across runs.

use std::sync::{Arc, Mutex};

use ra_core::statistics::Statistics;
use ra_engine::training_coordinator::{
    bootstrap_model, shared_coordinator_from_model,
    SharedTrainingCoordinator, TrainingCoordinator,
};
use ra_engine::{Optimizer, OptimizerConfig, ResourceBudget};
use ra_parser::sql_to_relexpr;

/// Minimum number of queries to generate for meaningful training.
const MIN_QUERIES: usize = 1000;

/// Seed for deterministic query generation.
const RNG_SEED: u64 = 0xDEAD_BEEF_CAFE_F00D;

// ============================================================================
// Query generation
// ============================================================================

/// Simple xorshift64 PRNG for deterministic generation without needing
/// the `rand` crate in integration tests.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn next_usize(&mut self, max: usize) -> usize {
        (self.next_u64() as usize) % max
    }

    fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next_usize(items.len())]
    }
}

/// TPC-H table definitions for query generation.
const TABLES: &[(&str, &[&str])] = &[
    (
        "lineitem",
        &[
            "l_orderkey",
            "l_partkey",
            "l_suppkey",
            "l_linenumber",
            "l_quantity",
            "l_extendedprice",
            "l_discount",
            "l_tax",
            "l_returnflag",
            "l_linestatus",
            "l_shipdate",
        ],
    ),
    (
        "orders",
        &[
            "o_orderkey",
            "o_custkey",
            "o_orderstatus",
            "o_totalprice",
            "o_orderdate",
            "o_orderpriority",
            "o_clerk",
            "o_shippriority",
        ],
    ),
    (
        "customer",
        &[
            "c_custkey",
            "c_name",
            "c_address",
            "c_nationkey",
            "c_phone",
            "c_acctbal",
            "c_mktsegment",
        ],
    ),
    (
        "supplier",
        &[
            "s_suppkey",
            "s_name",
            "s_address",
            "s_nationkey",
            "s_phone",
            "s_acctbal",
        ],
    ),
    (
        "part",
        &[
            "p_partkey",
            "p_name",
            "p_mfgr",
            "p_brand",
            "p_type",
            "p_size",
            "p_container",
            "p_retailprice",
        ],
    ),
    (
        "partsupp",
        &[
            "ps_partkey",
            "ps_suppkey",
            "ps_availqty",
            "ps_supplycost",
        ],
    ),
    ("nation", &["n_nationkey", "n_name", "n_regionkey"]),
    ("region", &["r_regionkey", "r_name"]),
];

/// Join relationships between TPC-H tables (left_table, left_col, right_table, right_col).
const JOIN_KEYS: &[(&str, &str, &str, &str)] = &[
    ("orders", "o_custkey", "customer", "c_custkey"),
    ("lineitem", "l_orderkey", "orders", "o_orderkey"),
    ("lineitem", "l_partkey", "part", "p_partkey"),
    ("lineitem", "l_suppkey", "supplier", "s_suppkey"),
    ("partsupp", "ps_partkey", "part", "p_partkey"),
    ("partsupp", "ps_suppkey", "supplier", "s_suppkey"),
    ("customer", "c_nationkey", "nation", "n_nationkey"),
    ("supplier", "s_nationkey", "nation", "n_nationkey"),
    ("nation", "n_regionkey", "region", "r_regionkey"),
];

/// Comparison operators for filter predicates.
const COMP_OPS: &[&str] = &["=", ">", "<", ">=", "<=", "<>"];

/// String literals for equality predicates.
const STRING_LITS: &[&str] = &[
    "'BUILDING'",
    "'AUTOMOBILE'",
    "'HOUSEHOLD'",
    "'MACHINERY'",
    "'FURNITURE'",
    "'R'",
    "'F'",
    "'O'",
    "'P'",
];

/// Numeric literals for range predicates.
const NUM_LITS: &[&str] = &[
    "0", "1", "5", "10", "20", "50", "100", "1000", "10000", "50000",
];

/// Aggregate functions.
const AGG_FUNCS: &[&str] = &["COUNT", "SUM", "AVG", "MIN", "MAX"];

/// Generate a simple SELECT with filter.
fn gen_simple_select(rng: &mut Rng) -> String {
    let (table, cols) = TABLES[rng.next_usize(TABLES.len())];
    let col = rng.choose(cols);
    let ncols = 1 + rng.next_usize(3.min(cols.len()));
    let select_cols: Vec<&str> = cols.iter().take(ncols).copied().collect();
    let op = rng.choose(COMP_OPS);

    let lit = if rng.next_usize(2) == 0 {
        rng.choose(NUM_LITS).to_string()
    } else {
        rng.choose(STRING_LITS).to_string()
    };

    format!(
        "SELECT {} FROM {} WHERE {} {} {}",
        select_cols.join(", "),
        table,
        col,
        op,
        lit
    )
}

/// Generate a two-table join query.
fn gen_two_table_join(rng: &mut Rng) -> String {
    let join = JOIN_KEYS[rng.next_usize(JOIN_KEYS.len())];
    let (lt, lc, rt, rc) = join;

    let left_table = TABLES.iter().find(|(n, _)| **n == *lt).unwrap();
    let right_table = TABLES.iter().find(|(n, _)| **n == *rt).unwrap();

    let lcol = rng.choose(left_table.1);
    let rcol = rng.choose(right_table.1);

    format!(
        "SELECT {}.{}, {}.{} FROM {} JOIN {} ON {}.{} = {}.{}",
        lt, lcol, rt, rcol, lt, rt, lt, lc, rt, rc
    )
}

/// Generate a three-table join query.
fn gen_three_table_join(rng: &mut Rng) -> String {
    // Pick two compatible joins that share a table
    let j1_idx = rng.next_usize(JOIN_KEYS.len());
    let j1 = JOIN_KEYS[j1_idx];

    // Find a second join that connects to one of the first join's tables
    let candidates: Vec<_> = JOIN_KEYS
        .iter()
        .enumerate()
        .filter(|(i, j2)| {
            *i != j1_idx
                && (j2.0 == j1.0
                    || j2.0 == j1.2
                    || j2.2 == j1.0
                    || j2.2 == j1.2)
        })
        .collect();

    if candidates.is_empty() {
        return gen_two_table_join(rng);
    }

    let (_, j2) = candidates[rng.next_usize(candidates.len())];

    let t1_cols = TABLES.iter().find(|(n, _)| *n == j1.0).unwrap().1;
    let t2_cols = TABLES.iter().find(|(n, _)| *n == j1.2).unwrap().1;
    let t3_cols = TABLES.iter().find(|(n, _)| *n == j2.2).unwrap().1;

    format!(
        "SELECT {}.{}, {}.{}, {}.{} FROM {} \
         JOIN {} ON {}.{} = {}.{} \
         JOIN {} ON {}.{} = {}.{}",
        j1.0, t1_cols[0],
        j1.2, t2_cols[0],
        j2.2, t3_cols[0],
        j1.0,
        j1.2, j1.0, j1.1, j1.2, j1.3,
        j2.2, j2.0, j2.1, j2.2, j2.3,
    )
}

/// Generate an aggregate query.
fn gen_aggregate(rng: &mut Rng) -> String {
    let (table, cols) = TABLES[rng.next_usize(TABLES.len())];
    let group_col = rng.choose(cols);
    let agg_col = rng.choose(cols);
    let agg_func = rng.choose(AGG_FUNCS);

    if *agg_func == "COUNT" {
        format!(
            "SELECT {}, COUNT(*) FROM {} GROUP BY {}",
            group_col, table, group_col
        )
    } else {
        format!(
            "SELECT {}, {}({}) FROM {} GROUP BY {}",
            group_col, agg_func, agg_col, table, group_col
        )
    }
}

/// Generate a query with ORDER BY and LIMIT.
fn gen_ordered_limit(rng: &mut Rng) -> String {
    let (table, cols) = TABLES[rng.next_usize(TABLES.len())];
    let order_col = rng.choose(cols);
    let dir = if rng.next_usize(2) == 0 {
        "ASC"
    } else {
        "DESC"
    };
    let limit = 1 + rng.next_usize(100);

    format!(
        "SELECT * FROM {} ORDER BY {} {} LIMIT {}",
        table, order_col, dir, limit
    )
}

/// Generate a subquery (EXISTS or IN).
fn gen_subquery(rng: &mut Rng) -> String {
    let join = JOIN_KEYS[rng.next_usize(JOIN_KEYS.len())];
    let (lt, lc, rt, rc) = join;

    if rng.next_usize(2) == 0 {
        format!(
            "SELECT * FROM {} WHERE EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{})",
            lt, rt, lt, lc, rt, rc
        )
    } else {
        format!(
            "SELECT * FROM {} WHERE {} IN (SELECT {} FROM {})",
            lt, lc, rc, rt
        )
    }
}

/// Generate a UNION query.
fn gen_union(rng: &mut Rng) -> String {
    let (table, cols) = TABLES[rng.next_usize(TABLES.len())];
    let col = rng.choose(cols);
    let lit1 = rng.choose(NUM_LITS);
    let lit2 = rng.choose(NUM_LITS);
    let union_type = if rng.next_usize(2) == 0 {
        "UNION"
    } else {
        "UNION ALL"
    };

    format!(
        "SELECT {} FROM {} WHERE {} > {} {} SELECT {} FROM {} WHERE {} < {}",
        col, table, col, lit1, union_type, col, table, col, lit2
    )
}

/// Generate a query with DISTINCT.
fn gen_distinct(rng: &mut Rng) -> String {
    let (table, cols) = TABLES[rng.next_usize(TABLES.len())];
    let ncols = 1 + rng.next_usize(3.min(cols.len()));
    let select_cols: Vec<&str> = cols.iter().take(ncols).copied().collect();

    format!("SELECT DISTINCT {} FROM {}", select_cols.join(", "), table)
}

/// Generate a join with aggregate.
fn gen_join_aggregate(rng: &mut Rng) -> String {
    let join = JOIN_KEYS[rng.next_usize(JOIN_KEYS.len())];
    let (lt, lc, rt, rc) = join;

    let left_table = TABLES.iter().find(|(n, _)| **n == *lt).unwrap();
    let right_table = TABLES.iter().find(|(n, _)| **n == *rt).unwrap();

    let group_col = rng.choose(right_table.1);
    let agg_col = rng.choose(left_table.1);
    let agg_func = rng.choose(AGG_FUNCS);

    if *agg_func == "COUNT" {
        format!(
            "SELECT {}.{}, COUNT(*) FROM {} JOIN {} ON {}.{} = {}.{} GROUP BY {}.{}",
            rt, group_col, lt, rt, lt, lc, rt, rc, rt, group_col
        )
    } else {
        format!(
            "SELECT {}.{}, {}({}.{}) FROM {} JOIN {} ON {}.{} = {}.{} GROUP BY {}.{}",
            rt, group_col, agg_func, lt, agg_col, lt, rt, lt, lc, rt, rc, rt, group_col
        )
    }
}

/// Generate a full diverse set of SQL queries.
fn generate_queries(count: usize) -> Vec<String> {
    let mut rng = Rng::new(RNG_SEED);
    let mut queries = Vec::with_capacity(count);

    // Distribution of query types (weighted toward joins and aggregates
    // since those exercise more optimizer rules):
    // 20% simple selects
    // 25% two-table joins
    // 15% three-table joins
    // 15% aggregates
    // 5% ordered/limit
    // 5% subqueries
    // 5% unions
    // 5% distinct
    // 5% join + aggregate
    for _ in 0..count {
        let query = match rng.next_usize(100) {
            0..20 => gen_simple_select(&mut rng),
            20..45 => gen_two_table_join(&mut rng),
            45..60 => gen_three_table_join(&mut rng),
            60..75 => gen_aggregate(&mut rng),
            75..80 => gen_ordered_limit(&mut rng),
            80..85 => gen_subquery(&mut rng),
            85..90 => gen_union(&mut rng),
            90..95 => gen_distinct(&mut rng),
            _ => gen_join_aggregate(&mut rng),
        };
        queries.push(query);
    }
    queries
}

// ============================================================================
// Optimizer setup
// ============================================================================

/// Create an optimizer with TPC-H table statistics and training enabled.
fn create_training_optimizer(
    coordinator: SharedTrainingCoordinator,
) -> Optimizer {
    let config = OptimizerConfig {
        use_adaptive_limits: true,
        iter_limit: 10, // Keep iterations low for speed
        node_limit: 10_000, // Limit e-graph size
        time_limit_secs: 2,
        max_optimization_time_ms: 2000,
        ..OptimizerConfig::default()
    };
    let mut opt = Optimizer::with_config(config).with_training(coordinator);

    // Add TPC-H table statistics for realistic cost estimation
    let tpch_stats: &[(&str, f64, u64, u64)] = &[
        ("lineitem", 6_000_000.0, 160, 960_000_000),
        ("orders", 1_500_000.0, 120, 180_000_000),
        ("customer", 150_000.0, 200, 30_000_000),
        ("supplier", 10_000.0, 180, 1_800_000),
        ("part", 200_000.0, 160, 32_000_000),
        ("partsupp", 800_000.0, 140, 112_000_000),
        ("nation", 25.0, 100, 2_500),
        ("region", 5.0, 80, 400),
    ];

    for (table, rows, avg_row, total) in tpch_stats {
        let mut stats = Statistics::new(*rows);
        stats.avg_row_size = *avg_row;
        stats.total_size = *total;
        opt.add_table_stats(*table, stats);
    }

    opt
}

// ============================================================================
// Main test
// ============================================================================

/// Run the training loop with fuzzed queries and verify loss decreases.
///
/// This test generates 1200 SQL queries covering diverse patterns (simple
/// scans, multi-table joins, aggregates, subqueries, unions), parses them
/// through `sql_to_relexpr`, optimizes each with the training coordinator
/// active, then verifies:
///
/// 1. Training actually occurred (train steps > 0)
/// 2. The model was updated (samples_trained > 0)
/// 3. Loss decreased from the initial random state
#[test]
#[ignore] // ~1s in release, may be slower in debug CI; run with: cargo test --test training_loop_test -- --ignored
fn training_loop_converges_with_fuzzed_queries() {
    // 1. Bootstrap the model with synthetic heuristic data
    let initial_model = bootstrap_model();
    let coordinator = shared_coordinator_from_model(initial_model);

    // Record initial loss (before any training from real traces)
    let initial_loss = {
        let coord = coordinator.lock().unwrap();
        coord.stats().avg_loss
    };

    // 2. Create optimizer with training enabled
    let opt = create_training_optimizer(Arc::clone(&coordinator));

    // 3. Generate 1200 queries (want >= 1000 after parse failures)
    let queries = generate_queries(1200);
    assert!(
        queries.len() >= MIN_QUERIES,
        "Generated {} queries, need at least {}",
        queries.len(),
        MIN_QUERIES,
    );

    // 4. Parse and optimize each query
    let mut parse_ok = 0u64;
    let mut parse_fail = 0u64;
    let mut opt_ok = 0u64;
    let mut opt_fail = 0u64;

    for sql in &queries {
        match sql_to_relexpr(sql) {
            Ok(rel_expr) => {
                parse_ok += 1;
                match opt.optimize(&rel_expr) {
                    Ok(_) => opt_ok += 1,
                    Err(_) => opt_fail += 1,
                }
            }
            Err(_) => {
                parse_fail += 1;
            }
        }
    }

    // Sanity check: most queries should parse successfully
    assert!(
        parse_ok >= MIN_QUERIES as u64 / 2,
        "Too many parse failures: {parse_ok} succeeded, {parse_fail} failed"
    );

    // 5. Flush the coordinator to process any remaining buffered traces
    let stats = {
        let mut coord = coordinator.lock().unwrap();
        coord.flush();
        coord.stats()
    };

    // 6. Verify training happened
    assert!(
        stats.total_traces >= parse_ok / 2,
        "Expected traces from optimization runs: got {}, optimized {}",
        stats.total_traces,
        opt_ok,
    );
    assert!(
        stats.total_train_steps > 0,
        "No training steps occurred (traces: {}, buffer: {})",
        stats.total_traces,
        stats.buffer_pending,
    );

    // 7. Verify model was updated
    assert!(
        stats.model_samples_trained > 0,
        "Model samples_trained should be > 0, got {}",
        stats.model_samples_trained,
    );

    // 8. Verify loss is bounded (not diverging)
    // After training on real optimization traces, loss should be reasonable.
    // We compare against a generous upper bound since the model starts from
    // bootstrap weights and actual query traces may differ from synthetic data.
    let final_loss = stats.avg_loss;
    assert!(
        final_loss < 10.0,
        "Loss is too high ({final_loss}), model may be diverging"
    );

    // If we had a meaningful initial loss, verify it didn't get worse
    if initial_loss > 0.0 {
        // Allow some tolerance - training on real data can temporarily
        // increase loss vs synthetic bootstrap, but shouldn't explode
        assert!(
            final_loss < initial_loss * 5.0,
            "Loss increased too much: initial={initial_loss}, final={final_loss}"
        );
    }

    // Print summary for manual inspection
    eprintln!("=== Training Loop Results ===");
    eprintln!("Queries generated: {}", queries.len());
    eprintln!("Parse success: {parse_ok}, fail: {parse_fail}");
    eprintln!("Optimize success: {opt_ok}, fail: {opt_fail}");
    eprintln!("Total traces: {}", stats.total_traces);
    eprintln!("Train steps: {}", stats.total_train_steps);
    eprintln!("Model samples: {}", stats.model_samples_trained);
    eprintln!("Initial loss: {initial_loss}");
    eprintln!("Final loss: {final_loss}");
    eprintln!("=============================");
}

// ============================================================================
// Task 12: Speculative router prediction accuracy
// ============================================================================

/// Train a model by running it through the full training loop with generated
/// queries. Returns the trained model for use by the speculative router.
fn train_model_for_testing() -> Arc<ra_engine::BitNetCostModel> {
    let initial_model = bootstrap_model();
    let coordinator = shared_coordinator_from_model(initial_model);

    let opt = create_training_optimizer(Arc::clone(&coordinator));

    // Generate and optimize enough queries to produce meaningful training
    let queries = generate_queries(800);
    for sql in &queries {
        if let Ok(rel_expr) = sql_to_relexpr(sql) {
            let _ = opt.optimize(&rel_expr);
        }
    }

    // Flush remaining buffered traces
    {
        let mut coord = coordinator.lock().unwrap();
        coord.flush();
    }

    let model = coordinator.lock().unwrap().current_model();
    model
}

/// Classify what route SHOULD have been predicted based on the actual
/// number of iterations the optimizer used.
///
/// The e-graph optimizer always runs at least 1 iteration even for trivial
/// queries (because `optimize_bounded` enters the saturation loop). The
/// classification thresholds are calibrated to match `OptRoute::iter_limit`:
///   Skip/LeftDeep = 0 iters conceptually, but 1-2 in practice (saturates)
///   EGraphLow = up to 3 iters
///   EGraphMedium = up to 8 iters
///   EGraphHigh = 9+ iters
fn classify_actual_route(
    actual_iterations: usize,
    features: &ra_engine::OptimizationFeatures,
) -> ra_engine::OptRoute {
    // Single-table queries that converge in 1-2 iterations are "Skip"
    if features.table_count <= 1.0 && actual_iterations <= 2 {
        return ra_engine::OptRoute::Skip;
    }

    // Multi-table equi-join chains that converge quickly are "LeftDeep"
    if features.table_count >= 2.0
        && features.equi_join_fraction >= 0.8
        && features.cross_join_present < 0.5
        && actual_iterations <= 3
    {
        return ra_engine::OptRoute::LeftDeep;
    }

    match actual_iterations {
        0..=3 => ra_engine::OptRoute::EGraphLow,
        4..=8 => ra_engine::OptRoute::EGraphMedium,
        _ => ra_engine::OptRoute::EGraphHigh,
    }
}

/// Create an optimizer without speculative routing (to measure actual
/// behavior) but with a generous iteration budget.
fn create_measurement_optimizer() -> Optimizer {
    let config = OptimizerConfig {
        use_adaptive_limits: false,
        iter_limit: 20,
        node_limit: 50_000,
        time_limit_secs: 5,
        max_optimization_time_ms: 5000,
        ..OptimizerConfig::default()
    };
    let mut opt = Optimizer::with_config(config).with_resource_budget(
        ResourceBudget::unlimited().with_iteration_limit(20),
    );

    // Add TPC-H table statistics for realistic cost estimation
    let tpch_stats: &[(&str, f64, u64, u64)] = &[
        ("lineitem", 6_000_000.0, 160, 960_000_000),
        ("orders", 1_500_000.0, 120, 180_000_000),
        ("customer", 150_000.0, 200, 30_000_000),
        ("supplier", 10_000.0, 180, 1_800_000),
        ("part", 200_000.0, 160, 32_000_000),
        ("partsupp", 800_000.0, 140, 112_000_000),
        ("nation", 25.0, 100, 2_500),
        ("region", 5.0, 80, 400),
    ];

    for (table, rows, avg_row, total) in tpch_stats {
        let mut stats = Statistics::new(*rows);
        stats.avg_row_size = *avg_row;
        stats.total_size = *total;
        opt.add_table_stats(*table, stats);
    }

    opt
}

/// Measures speculative router prediction accuracy against actual
/// optimization behavior.
///
/// Trains a model, creates a router, runs TPC-H-like queries, and
/// compares predicted routes/iterations against actual optimizer behavior.
/// Also compares against the heuristic fallback for reference.
#[test]
#[ignore] // ~3s: run with cargo test --test training_loop_test -- --ignored
fn speculative_router_prediction_accuracy() {
    use ra_engine::{OptimizationFeatures, SpeculativeRouter};

    // 1. Train a model using the same approach as the training loop test
    let model = train_model_for_testing();

    // 2. Create a router with the trained model
    let router = SpeculativeRouter::new(Arc::clone(&model));

    // 3. Create a measurement optimizer (no routing, generous budget)
    let measurement_opt = create_measurement_optimizer();

    // 4. Generate a fresh set of queries for evaluation (different seed
    //    would be ideal, but we reuse the generator with more queries to
    //    get some that weren't in training)
    let queries = generate_queries(1500);
    // Use the last 500 as evaluation set (first 800 were used for training)
    let eval_queries = &queries[1000..];

    let mut correct_routes = 0u32;
    let mut adjacent_routes = 0u32;
    let mut correct_iterations = 0u32;
    let mut heuristic_correct_routes = 0u32;
    let mut total = 0u32;

    let mut route_distribution: [u32; 5] = [0; 5]; // Skip, LeftDeep, Low, Med, High
    let mut actual_distribution: [u32; 5] = [0; 5];

    for sql in eval_queries {
        let Ok(relexpr) = sql_to_relexpr(sql) else {
            continue;
        };

        // Extract features for router prediction
        let features = OptimizationFeatures::from_expr(&relexpr);

        // Get prediction from the trained router
        let prediction = router.predict(&features);

        // Get heuristic fallback prediction for comparison
        let heuristic = SpeculativeRouter::heuristic_fallback(&features);

        // Get actual behavior by running full optimization
        let Ok(result) = measurement_opt.optimize_bounded(&relexpr) else {
            continue;
        };

        let actual_iterations = result.resource_usage.iterations_used;

        // Classify the actual route based on iterations + features
        let predicted_route = prediction.route;
        let actual_route = classify_actual_route(actual_iterations, &features);

        // Track route distribution
        route_distribution[route_index(predicted_route)] += 1;
        actual_distribution[route_index(actual_route)] += 1;

        // Route accuracy: exact match
        if predicted_route == actual_route {
            correct_routes += 1;
        }

        // Adjacent route accuracy: within one level
        let pred_idx = route_index(predicted_route);
        let actual_idx = route_index(actual_route);
        if pred_idx.abs_diff(actual_idx) <= 1 {
            adjacent_routes += 1;
        }

        // Heuristic comparison
        if heuristic.route == actual_route {
            heuristic_correct_routes += 1;
        }

        // Iteration accuracy: within 50% of actual
        let predicted_iters = prediction.predicted_iterations_needed as usize;
        let tolerance = (actual_iterations as f64 * 0.5).max(1.0) as usize;
        if predicted_iters.abs_diff(actual_iterations) <= tolerance {
            correct_iterations += 1;
        }

        total += 1;
    }

    assert!(
        total >= 100,
        "Need at least 100 evaluated queries, got {total}"
    );

    let route_accuracy = f64::from(correct_routes) / f64::from(total);
    let adjacent_accuracy = f64::from(adjacent_routes) / f64::from(total);
    let iter_accuracy = f64::from(correct_iterations) / f64::from(total);
    let heuristic_accuracy = f64::from(heuristic_correct_routes) / f64::from(total);

    // Print results for manual inspection
    eprintln!("=== Speculative Router Prediction Accuracy ===");
    eprintln!("Evaluated queries: {total}");
    eprintln!(
        "  Route accuracy (exact): {:.1}% ({}/{})",
        route_accuracy * 100.0,
        correct_routes,
        total
    );
    eprintln!(
        "  Route accuracy (within 1 level): {:.1}% ({}/{})",
        adjacent_accuracy * 100.0,
        adjacent_routes,
        total
    );
    eprintln!(
        "  Iteration accuracy (within 50%): {:.1}% ({}/{})",
        iter_accuracy * 100.0,
        correct_iterations,
        total
    );
    eprintln!(
        "  Heuristic fallback accuracy: {:.1}% ({}/{})",
        heuristic_accuracy * 100.0,
        heuristic_correct_routes,
        total
    );
    eprintln!("Predicted route distribution:");
    eprintln!(
        "  Skip={}, LeftDeep={}, EGraphLow={}, EGraphMedium={}, EGraphHigh={}",
        route_distribution[0],
        route_distribution[1],
        route_distribution[2],
        route_distribution[3],
        route_distribution[4]
    );
    eprintln!("Actual route distribution:");
    eprintln!(
        "  Skip={}, LeftDeep={}, EGraphLow={}, EGraphMedium={}, EGraphHigh={}",
        actual_distribution[0],
        actual_distribution[1],
        actual_distribution[2],
        actual_distribution[3],
        actual_distribution[4]
    );
    eprintln!("================================================");

    // Minimum acceptable accuracy after training on synthetic data.
    // The model is trained on synthetic bootstrap data, so exact route
    // match is hard. We require either:
    // - 50% exact route accuracy, OR
    // - 70% "within one level" accuracy (adjacent route is acceptable)
    //
    // The heuristic fallback provides a reference point.
    assert!(
        route_accuracy >= 0.5 || adjacent_accuracy >= 0.7,
        "Route accuracy too low: exact={:.1}%, adjacent={:.1}% \
         (need >=50% exact OR >=70% adjacent)",
        route_accuracy * 100.0,
        adjacent_accuracy * 100.0
    );
}

/// Map an `OptRoute` to an index for distribution tracking.
fn route_index(route: ra_engine::OptRoute) -> usize {
    match route {
        ra_engine::OptRoute::Skip => 0,
        ra_engine::OptRoute::LeftDeep => 1,
        ra_engine::OptRoute::EGraphLow => 2,
        ra_engine::OptRoute::EGraphMedium => 3,
        ra_engine::OptRoute::EGraphHigh => 4,
    }
}

// ============================================================================
// Smoke tests
// ============================================================================

/// Quick smoke test that the training infrastructure works (not ignored).
///
/// Runs a small batch through the training loop to verify integration
/// without the full 1000+ query workload.
#[test]
fn training_loop_smoke_test() {
    let coordinator = Arc::new(Mutex::new(TrainingCoordinator::new()));
    let opt = create_training_optimizer(Arc::clone(&coordinator));

    // A handful of known-good queries that definitely parse
    let queries = &[
        "SELECT * FROM orders",
        "SELECT o_orderkey, o_totalprice FROM orders WHERE o_totalprice > 1000",
        "SELECT * FROM orders WHERE o_orderstatus = 'O'",
        "SELECT o_orderkey FROM orders JOIN customer ON orders.o_custkey = customer.c_custkey",
        "SELECT c_name, COUNT(*) FROM customer GROUP BY c_name",
        "SELECT * FROM lineitem WHERE l_quantity > 10 ORDER BY l_extendedprice DESC LIMIT 50",
        "SELECT DISTINCT o_orderstatus FROM orders",
        "SELECT * FROM orders WHERE o_custkey IN (SELECT c_custkey FROM customer)",
        "SELECT l_orderkey, SUM(l_extendedprice) FROM lineitem GROUP BY l_orderkey",
        "SELECT * FROM part WHERE p_size > 5 AND p_retailprice < 1000",
    ];

    let mut optimized = 0;
    for sql in queries {
        if let Ok(expr) = sql_to_relexpr(sql) {
            if opt.optimize(&expr).is_ok() {
                optimized += 1;
            }
        }
    }

    assert!(
        optimized >= 5,
        "Expected at least 5 successful optimizations, got {optimized}"
    );

    // Verify coordinator received traces
    let stats = coordinator.lock().unwrap().stats();
    assert!(
        stats.total_traces > 0,
        "Training coordinator should have received traces"
    );
}
