//! Train the `BitNet` cost model and save to `models/cost_model.bitnet.json`.
//!
//! Usage: `cargo run --example train_model -p ra-engine`
//!
//! This generates 1200+ SQL queries from parameterized TPC-H-style templates,
//! parses them through `sql_to_relexpr`, optimizes each with the training
//! coordinator active, feeds bootstrap samples, then saves the trained model.

#![expect(clippy::unwrap_used, clippy::print_stdout)]

use std::sync::Arc;

use ra_core::statistics::Statistics;
use ra_engine::training_coordinator::{
    bootstrap_model, generate_bootstrap_samples, shared_coordinator_from_model,
};
use ra_engine::{Optimizer, OptimizerConfig};
use ra_parser::sql_to_relexpr;

const MODEL_PATH: &str = "models/cost_model.bitnet.json";
const QUERY_COUNT: usize = 1200;
const RNG_SEED: u64 = 0xDEAD_BEEF_CAFE_F00D;

// ============================================================================
// Deterministic RNG (same as training_loop_test.rs)
// ============================================================================

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

// ============================================================================
// TPC-H schema definitions
// ============================================================================

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

const COMP_OPS: &[&str] = &["=", ">", "<", ">=", "<=", "<>"];

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

const NUM_LITS: &[&str] = &[
    "0", "1", "5", "10", "20", "50", "100", "1000", "10000", "50000",
];

const AGG_FUNCS: &[&str] = &["COUNT", "SUM", "AVG", "MIN", "MAX"];

// ============================================================================
// Query generators
// ============================================================================

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

fn gen_two_table_join(rng: &mut Rng) -> String {
    let join = JOIN_KEYS[rng.next_usize(JOIN_KEYS.len())];
    let (lt, lc, rt, rc) = join;

    let left_table = TABLES.iter().find(|(n, _)| **n == *lt).unwrap();
    let right_table = TABLES.iter().find(|(n, _)| **n == *rt).unwrap();

    let lcol = rng.choose(left_table.1);
    let rcol = rng.choose(right_table.1);

    format!(
        "SELECT {lt}.{lcol}, {rt}.{rcol} FROM {lt} JOIN {rt} ON {lt}.{lc} = {rt}.{rc}"
    )
}

fn gen_three_table_join(rng: &mut Rng) -> String {
    let j1_idx = rng.next_usize(JOIN_KEYS.len());
    let j1 = JOIN_KEYS[j1_idx];

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

fn gen_aggregate(rng: &mut Rng) -> String {
    let (table, cols) = TABLES[rng.next_usize(TABLES.len())];
    let group_col = rng.choose(cols);
    let agg_col = rng.choose(cols);
    let agg_func = rng.choose(AGG_FUNCS);

    if *agg_func == "COUNT" {
        format!(
            "SELECT {group_col}, COUNT(*) FROM {table} GROUP BY {group_col}"
        )
    } else {
        format!(
            "SELECT {group_col}, {agg_func}({agg_col}) FROM {table} GROUP BY {group_col}"
        )
    }
}

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
        "SELECT * FROM {table} ORDER BY {order_col} {dir} LIMIT {limit}"
    )
}

fn gen_subquery(rng: &mut Rng) -> String {
    let join = JOIN_KEYS[rng.next_usize(JOIN_KEYS.len())];
    let (lt, lc, rt, rc) = join;

    if rng.next_usize(2) == 0 {
        format!(
            "SELECT * FROM {lt} WHERE EXISTS \
             (SELECT 1 FROM {rt} WHERE {lt}.{lc} = {rt}.{rc})"
        )
    } else {
        format!(
            "SELECT * FROM {lt} WHERE {lc} IN (SELECT {rc} FROM {rt})"
        )
    }
}

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
        "SELECT {col} FROM {table} WHERE {col} > {lit1} {union_type} SELECT {col} FROM {table} WHERE {col} < {lit2}"
    )
}

fn gen_distinct(rng: &mut Rng) -> String {
    let (table, cols) = TABLES[rng.next_usize(TABLES.len())];
    let ncols = 1 + rng.next_usize(3.min(cols.len()));
    let select_cols: Vec<&str> = cols.iter().take(ncols).copied().collect();

    format!("SELECT DISTINCT {} FROM {}", select_cols.join(", "), table)
}

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
            "SELECT {rt}.{group_col}, COUNT(*) FROM {lt} JOIN {rt} ON {lt}.{lc} = {rt}.{rc} \
             GROUP BY {rt}.{group_col}"
        )
    } else {
        format!(
            "SELECT {rt}.{group_col}, {agg_func}({lt}.{agg_col}) FROM {lt} JOIN {rt} ON {lt}.{lc} = {rt}.{rc} \
             GROUP BY {rt}.{group_col}"
        )
    }
}

fn generate_queries(count: usize) -> Vec<String> {
    let mut rng = Rng::new(RNG_SEED);
    let mut queries = Vec::with_capacity(count);

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

fn create_training_optimizer(
    coordinator: ra_engine::training_coordinator::SharedTrainingCoordinator,
) -> Optimizer {
    let config = OptimizerConfig {
        use_adaptive_limits: true,
        iter_limit: 10,
        node_limit: 10_000,
        time_limit_secs: 2,
        max_optimization_time_ms: 2000,
        ..OptimizerConfig::default()
    };
    let mut opt = Optimizer::with_config(config).with_training(coordinator);

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
// Main
// ============================================================================

fn main() {
    println!("=== BitNet Cost Model Training ===");

    // 1. Bootstrap from synthetic heuristic data
    println!("[1/9] Bootstrapping model from heuristic weights...");
    let initial_model = bootstrap_model();
    let coordinator = shared_coordinator_from_model(initial_model);

    // 2. Create optimizer with training enabled + TPC-H stats
    println!("[2/9] Creating optimizer with TPC-H table stats...");
    let opt = create_training_optimizer(Arc::clone(&coordinator));

    // 3. Generate 1200+ SQL queries
    println!("[3/9] Generating {QUERY_COUNT} SQL queries...");
    let queries = generate_queries(QUERY_COUNT);

    // 4. Parse and optimize each query
    println!("[4/9] Parsing and optimizing queries...");
    let mut parse_ok: u64 = 0;
    let mut parse_fail: u64 = 0;
    let mut opt_ok: u64 = 0;
    let mut opt_fail: u64 = 0;

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

    println!(
        "      Parse: {parse_ok} ok, {parse_fail} failed | Optimize: {opt_ok} ok, {opt_fail} failed"
    );

    // 5. Feed bootstrap samples directly to the trainer
    println!("[5/9] Feeding bootstrap samples to trainer...");
    let bootstrap_samples = generate_bootstrap_samples();
    let bootstrap_count = bootstrap_samples.len();
    {
        let mut coord = coordinator.lock().unwrap();
        coord.train_on_samples(&bootstrap_samples);
    }
    println!("      Fed {bootstrap_count} bootstrap samples");

    // 6. Flush the coordinator
    println!("[6/9] Flushing coordinator...");
    {
        let mut coord = coordinator.lock().unwrap();
        coord.flush();
    }

    // 7. Collect stats
    println!("[7/9] Collecting training stats...");
    let stats = coordinator.lock().unwrap().stats();

    // 8. Ensure models/ directory exists
    println!("[8/9] Ensuring models/ directory exists...");
    std::fs::create_dir_all("models").unwrap();

    // 9. Save the model
    println!("[9/9] Saving model to {MODEL_PATH}...");
    coordinator.lock().unwrap().save_model(MODEL_PATH).unwrap();

    // Print summary
    println!();
    println!("=== Training Complete ===");
    println!("  Model path:      {MODEL_PATH}");
    println!("  Total traces:    {}", stats.total_traces);
    println!("  Train steps:     {}", stats.total_train_steps);
    println!("  Avg loss:        {:.6}", stats.avg_loss);
    println!("  Samples trained: {}", stats.model_samples_trained);
    println!(
        "  Queries:         {QUERY_COUNT} generated, {parse_ok} parsed, {opt_ok} optimized"
    );
    println!(
        "  Bootstrap:       {bootstrap_count} samples"
    );
    println!("=========================");
}
