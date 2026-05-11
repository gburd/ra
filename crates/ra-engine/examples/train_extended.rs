//! Extended training pipeline for the `BitNet` cost model.
//!
//! Runs 10,000+ queries through the optimizer across 5 progressive training
//! rounds, measuring prediction accuracy after each round and saving
//! intermediate model checkpoints.
//!
//! Usage: `cargo run --example train_extended -p ra-engine --release`
//!
//! Structure:
//! - Round 1: 2000 simple queries (1-4 tables, equi-joins) → baseline
//! - Round 2: 2000 medium queries (3-8 tables, mixed joins) → broadens coverage
//! - Round 3: 2000 complex queries (5-12 tables, subqueries, CTEs, windows)
//! - Round 4: 2000 OLTP-style queries (point lookups, short scans)
//! - Round 5: 2000 mixed replay of all patterns → consolidation
//!
//! After each round:
//! - Flush coordinator, print training stats
//! - Run 200 evaluation queries and measure route prediction accuracy
//! - Save checkpoint to `models/cost_model.round{N}.json`
//!
//! Final model saved to `models/cost_model.bitnet.json`.

#![expect(clippy::unwrap_used, clippy::print_stdout, dead_code)]

use std::sync::Arc;
use std::time::Instant;

use ra_core::statistics::Statistics;
use ra_engine::speculative_router::{OptRoute, OptimizationFeatures, SpeculativeRouter};
use ra_engine::training_coordinator::{
    bootstrap_model, generate_bootstrap_samples, shared_coordinator_from_model,
    SharedTrainingCoordinator,
};
use ra_engine::{Optimizer, OptimizerConfig};
use ra_parser::sql_to_relexpr;

const FINAL_MODEL_PATH: &str = "models/cost_model.bitnet.json";
const QUERIES_PER_ROUND: usize = 2000;
const EVAL_QUERIES_PER_ROUND: usize = 200;
const RNG_SEED: u64 = 0xCAFE_BABE_DEAD_BEEF;

// ============================================================================
// Deterministic RNG (xorshift64)
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

    fn next_range(&mut self, min: usize, max: usize) -> usize {
        min + self.next_usize(max - min + 1)
    }

    fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next_usize(items.len())]
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() & 0xFFFF_FFFF) as f32 / u32::MAX as f32
    }
}

// ============================================================================
// Schema definitions (TPC-H + e-commerce)
// ============================================================================

const TPCH_TABLES: &[(&str, &[&str])] = &[
    (
        "lineitem",
        &[
            "l_orderkey", "l_partkey", "l_suppkey", "l_linenumber",
            "l_quantity", "l_extendedprice", "l_discount", "l_tax",
            "l_returnflag", "l_linestatus", "l_shipdate",
        ],
    ),
    (
        "orders",
        &[
            "o_orderkey", "o_custkey", "o_orderstatus", "o_totalprice",
            "o_orderdate", "o_orderpriority", "o_clerk", "o_shippriority",
        ],
    ),
    (
        "customer",
        &[
            "c_custkey", "c_name", "c_address", "c_nationkey",
            "c_phone", "c_acctbal", "c_mktsegment",
        ],
    ),
    (
        "supplier",
        &[
            "s_suppkey", "s_name", "s_address", "s_nationkey",
            "s_phone", "s_acctbal",
        ],
    ),
    (
        "part",
        &[
            "p_partkey", "p_name", "p_mfgr", "p_brand",
            "p_type", "p_size", "p_container", "p_retailprice",
        ],
    ),
    (
        "partsupp",
        &["ps_partkey", "ps_suppkey", "ps_availqty", "ps_supplycost"],
    ),
    ("nation", &["n_nationkey", "n_name", "n_regionkey"]),
    ("region", &["r_regionkey", "r_name"]),
];

const ECOM_TABLES: &[(&str, &[&str])] = &[
    (
        "users",
        &[
            "user_id", "username", "email", "created_at",
            "country_id", "status",
        ],
    ),
    (
        "products",
        &[
            "product_id", "name", "category_id", "price",
            "stock_qty", "created_at",
        ],
    ),
    (
        "categories",
        &["category_id", "category_name", "parent_id"],
    ),
    (
        "inventory",
        &[
            "inventory_id", "product_id", "warehouse_id",
            "quantity", "last_updated",
        ],
    ),
    (
        "payments",
        &[
            "payment_id", "order_id", "user_id", "amount",
            "payment_method", "payment_date",
        ],
    ),
    (
        "shipments",
        &[
            "shipment_id", "order_id", "carrier", "tracking_no",
            "shipped_date", "delivered_date",
        ],
    ),
];

const TPCH_JOIN_KEYS: &[(&str, &str, &str, &str)] = &[
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

const ECOM_JOIN_KEYS: &[(&str, &str, &str, &str)] = &[
    ("payments", "user_id", "users", "user_id"),
    ("payments", "order_id", "orders", "o_orderkey"),
    ("products", "category_id", "categories", "category_id"),
    ("inventory", "product_id", "products", "product_id"),
    ("shipments", "order_id", "orders", "o_orderkey"),
    ("users", "country_id", "nation", "n_nationkey"),
];

const COMP_OPS: &[&str] = &["=", ">", "<", ">=", "<=", "<>"];
const STRING_LITS: &[&str] = &[
    "'BUILDING'", "'AUTOMOBILE'", "'HOUSEHOLD'", "'MACHINERY'",
    "'FURNITURE'", "'R'", "'F'", "'O'", "'P'", "'URGENT'",
    "'HIGH'", "'MEDIUM'", "'LOW'",
];
const NUM_LITS: &[&str] = &[
    "0", "1", "5", "10", "20", "50", "100", "500",
    "1000", "5000", "10000", "50000",
];
const DATE_LITS: &[&str] = &[
    "'1993-01-01'", "'1994-06-15'", "'1995-03-15'", "'1996-01-01'",
    "'1997-07-01'", "'1998-12-31'",
];
const AGG_FUNCS: &[&str] = &["COUNT", "SUM", "AVG", "MIN", "MAX"];

// ============================================================================
// Helper: get all tables from both schemas
// ============================================================================

fn all_tables() -> Vec<(&'static str, &'static [&'static str])> {
    TPCH_TABLES
        .iter()
        .chain(ECOM_TABLES.iter())
        .copied()
        .collect()
}

fn all_join_keys() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    TPCH_JOIN_KEYS
        .iter()
        .chain(ECOM_JOIN_KEYS.iter())
        .copied()
        .collect()
}

// ============================================================================
// Query generators — Round 1: Simple queries (1-4 tables, equi-joins)
// ============================================================================

fn gen_simple_select(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let ncols = 1 + rng.next_usize(3.min(cols.len()));
    let select_cols: Vec<&str> = cols.iter().take(ncols).copied().collect();
    let op = rng.choose(COMP_OPS);

    let lit = match rng.next_usize(3) {
        0 => rng.choose(NUM_LITS).to_string(),
        1 => rng.choose(STRING_LITS).to_string(),
        _ => rng.choose(DATE_LITS).to_string(),
    };

    format!(
        "SELECT {} FROM {} WHERE {} {} {}",
        select_cols.join(", "),
        table, col, op, lit
    )
}

fn gen_simple_equi_join(rng: &mut Rng, max_tables: usize) -> String {
    let join_keys = all_join_keys();
    let num_joins = rng.next_range(1, max_tables.saturating_sub(1).max(1));

    let mut used_tables: Vec<&str> = Vec::new();
    let mut join_clauses: Vec<String> = Vec::new();
    let mut select_cols: Vec<String> = Vec::new();

    let first_join = &join_keys[rng.next_usize(join_keys.len())];
    used_tables.push(first_join.0);
    used_tables.push(first_join.2);
    join_clauses.push(format!(
        "JOIN {} ON {}.{} = {}.{}",
        first_join.2, first_join.0, first_join.1, first_join.2, first_join.3
    ));

    let tables = all_tables();
    let lt = tables.iter().find(|(n, _)| *n == first_join.0);
    let rt = tables.iter().find(|(n, _)| *n == first_join.2);
    if let Some((_, cols)) = lt {
        select_cols.push(format!("{}.{}", first_join.0, cols[0]));
    }
    if let Some((_, cols)) = rt {
        select_cols.push(format!("{}.{}", first_join.2, cols[0]));
    }

    for _ in 1..num_joins {
        let candidates: Vec<_> = join_keys
            .iter()
            .filter(|jk| {
                (used_tables.contains(&jk.0) && !used_tables.contains(&jk.2))
                    || (used_tables.contains(&jk.2) && !used_tables.contains(&jk.0))
            })
            .collect();

        if candidates.is_empty() {
            break;
        }

        let jk = candidates[rng.next_usize(candidates.len())];
        if used_tables.contains(&jk.0) {
            used_tables.push(jk.2);
            join_clauses.push(format!(
                "JOIN {} ON {}.{} = {}.{}", jk.2, jk.0, jk.1, jk.2, jk.3
            ));
            if let Some((_, cols)) = tables.iter().find(|(n, _)| *n == jk.2) {
                select_cols.push(format!("{}.{}", jk.2, cols[0]));
            }
        } else {
            used_tables.push(jk.0);
            join_clauses.push(format!(
                "JOIN {} ON {}.{} = {}.{}", jk.0, jk.0, jk.1, jk.2, jk.3
            ));
            if let Some((_, cols)) = tables.iter().find(|(n, _)| *n == jk.0) {
                select_cols.push(format!("{}.{}", jk.0, cols[0]));
            }
        }
    }

    if select_cols.is_empty() {
        select_cols.push("*".to_string());
    }

    format!(
        "SELECT {} FROM {} {}",
        select_cols.join(", "),
        used_tables[0],
        join_clauses.join(" ")
    )
}

fn gen_round1_query(rng: &mut Rng) -> String {
    match rng.next_usize(100) {
        0..30 => gen_simple_select(rng),
        30..60 => gen_simple_equi_join(rng, 2),
        60..80 => gen_simple_equi_join(rng, 3),
        80..95 => gen_simple_equi_join(rng, 4),
        _ => gen_distinct_simple(rng),
    }
}

fn gen_distinct_simple(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let ncols = 1 + rng.next_usize(3.min(cols.len()));
    let select_cols: Vec<&str> = cols.iter().take(ncols).copied().collect();
    format!("SELECT DISTINCT {} FROM {}", select_cols.join(", "), table)
}

// ============================================================================
// Query generators — Round 2: Medium queries (3-8 tables, mixed joins)
// ============================================================================

fn gen_aggregate_join(rng: &mut Rng) -> String {
    let join_keys = all_join_keys();
    let jk = join_keys[rng.next_usize(join_keys.len())];
    let tables = all_tables();
    let lt = tables.iter().find(|(n, _)| *n == jk.0).unwrap();
    let rt = tables.iter().find(|(n, _)| *n == jk.2).unwrap();

    let group_col = rng.choose(rt.1);
    let agg_col = rng.choose(lt.1);
    let agg_func = rng.choose(AGG_FUNCS);
    let having_val = rng.choose(NUM_LITS);

    if *agg_func == "COUNT" {
        format!(
            "SELECT {}.{}, COUNT(*) AS cnt FROM {} JOIN {} ON {}.{} = {}.{} \
             GROUP BY {}.{} HAVING COUNT(*) > {}",
            jk.2, group_col, jk.0, jk.2, jk.0, jk.1, jk.2, jk.3,
            jk.2, group_col, having_val
        )
    } else {
        format!(
            "SELECT {}.{}, {}({}.{}) AS agg_val FROM {} JOIN {} ON {}.{} = {}.{} \
             GROUP BY {}.{}",
            jk.2, group_col, agg_func, jk.0, agg_col,
            jk.0, jk.2, jk.0, jk.1, jk.2, jk.3,
            jk.2, group_col
        )
    }
}

fn gen_left_join(rng: &mut Rng) -> String {
    let join_keys = all_join_keys();
    let jk = join_keys[rng.next_usize(join_keys.len())];
    let tables = all_tables();
    let lt = tables.iter().find(|(n, _)| *n == jk.0).unwrap();
    let rt = tables.iter().find(|(n, _)| *n == jk.2).unwrap();

    format!(
        "SELECT {}.{}, {}.{} FROM {} LEFT JOIN {} ON {}.{} = {}.{} \
         WHERE {}.{} IS NULL",
        jk.0, lt.1[0], jk.2, rt.1[0],
        jk.0, jk.2, jk.0, jk.1, jk.2, jk.3,
        jk.2, rt.1[0]
    )
}

fn gen_between_query(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let low = rng.choose(NUM_LITS);
    let high = rng.choose(NUM_LITS);
    let ncols = 1 + rng.next_usize(3.min(cols.len()));
    let select_cols: Vec<&str> = cols.iter().take(ncols).copied().collect();

    format!(
        "SELECT {} FROM {} WHERE {} BETWEEN {} AND {}",
        select_cols.join(", "), table, col, low, high
    )
}

fn gen_in_list_query(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let count = rng.next_range(2, 5);
    let vals: Vec<&str> = (0..count)
        .map(|_| *rng.choose(STRING_LITS))
        .collect();

    format!(
        "SELECT * FROM {} WHERE {} IN ({})",
        table, col, vals.join(", ")
    )
}

fn gen_like_query(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let patterns = &["'%BUILDING%'", "'A%'", "'%ER'", "'M%CHINE%'", "'%HIGH%'"];
    let pattern = rng.choose(patterns);

    format!("SELECT * FROM {} WHERE {} LIKE {}", table, col, pattern)
}

fn gen_round2_query(rng: &mut Rng) -> String {
    match rng.next_usize(100) {
        0..20 => gen_simple_equi_join(rng, 5),
        20..35 => gen_simple_equi_join(rng, 8),
        35..50 => gen_aggregate_join(rng),
        50..60 => gen_left_join(rng),
        60..70 => gen_between_query(rng),
        70..80 => gen_in_list_query(rng),
        80..90 => gen_like_query(rng),
        _ => gen_multi_table_aggregate(rng),
    }
}

fn gen_multi_table_aggregate(rng: &mut Rng) -> String {
    let base = gen_simple_equi_join(rng, 5);
    let tables = all_tables();
    let (_, cols) = tables[rng.next_usize(tables.len())];
    let agg_col = rng.choose(cols);
    let group_col = rng.choose(cols);
    let agg_func = rng.choose(AGG_FUNCS);

    // Wrap the join in a subquery-free aggregate
    let from_clause = base.strip_prefix("SELECT ").unwrap_or(&base);
    let from_start = from_clause.find("FROM").unwrap_or(0);
    let from_part = &from_clause[from_start..];

    if *agg_func == "COUNT" {
        format!("SELECT {group_col}, COUNT(*) {from_part} GROUP BY {group_col}")
    } else {
        format!(
            "SELECT {group_col}, {agg_func}({agg_col}) {from_part} GROUP BY {group_col}"
        )
    }
}

// ============================================================================
// Query generators — Round 3: Complex queries (subqueries, CTEs, windows)
// ============================================================================

fn gen_correlated_subquery(rng: &mut Rng) -> String {
    let join_keys = all_join_keys();
    let jk = join_keys[rng.next_usize(join_keys.len())];

    if rng.next_usize(2) == 0 {
        format!(
            "SELECT * FROM {} WHERE EXISTS \
             (SELECT 1 FROM {} WHERE {}.{} = {}.{})",
            jk.0, jk.2, jk.0, jk.1, jk.2, jk.3
        )
    } else {
        format!(
            "SELECT * FROM {} WHERE {} IN (SELECT {} FROM {})",
            jk.0, jk.1, jk.3, jk.2
        )
    }
}

fn gen_cte_query(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let agg_func = rng.choose(AGG_FUNCS);
    let lit = rng.choose(NUM_LITS);

    if *agg_func == "COUNT" {
        format!(
            "WITH cte AS (SELECT {col}, COUNT(*) AS cnt FROM {table} \
             GROUP BY {col}) \
             SELECT * FROM cte WHERE cnt > {lit}"
        )
    } else {
        format!(
            "WITH cte AS (SELECT {col}, {agg_func}({col}) AS agg_val \
             FROM {table} GROUP BY {col}) \
             SELECT * FROM cte WHERE agg_val > {lit}"
        )
    }
}

fn gen_nested_cte(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (t1, cols1) = tables[rng.next_usize(tables.len())];
    let (t2, cols2) = tables[rng.next_usize(tables.len())];
    let col1 = rng.choose(cols1);
    let col2 = rng.choose(cols2);

    format!(
        "WITH cte1 AS (SELECT {col1} FROM {t1} WHERE {col1} > {}), \
         cte2 AS (SELECT {col2} FROM {t2} WHERE {col2} < {}) \
         SELECT * FROM cte1, cte2",
        rng.choose(NUM_LITS), rng.choose(NUM_LITS)
    )
}

fn gen_window_function(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let partition_col = rng.choose(cols);
    let order_col = rng.choose(cols);
    let value_col = rng.choose(cols);

    let window_funcs = &["ROW_NUMBER()", "RANK()", &format!("SUM({})", value_col)];
    let wf = rng.choose(window_funcs);

    format!(
        "SELECT {partition_col}, {order_col}, \
         {} OVER (PARTITION BY {partition_col} ORDER BY {order_col}) AS wf \
         FROM {table}",
        wf
    )
}

fn gen_window_with_join(rng: &mut Rng) -> String {
    let join_keys = all_join_keys();
    let jk = join_keys[rng.next_usize(join_keys.len())];
    let tables = all_tables();
    let lt = tables.iter().find(|(n, _)| *n == jk.0).unwrap();
    let rt = tables.iter().find(|(n, _)| *n == jk.2).unwrap();

    let part_col = rng.choose(rt.1);
    let ord_col = rng.choose(lt.1);

    format!(
        "SELECT {}.{}, {}.{}, \
         ROW_NUMBER() OVER (PARTITION BY {}.{} ORDER BY {}.{}) AS rn \
         FROM {} JOIN {} ON {}.{} = {}.{}",
        jk.0, lt.1[0], jk.2, rt.1[0],
        jk.2, part_col, jk.0, ord_col,
        jk.0, jk.2, jk.0, jk.1, jk.2, jk.3
    )
}

fn gen_set_operation(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let lit1 = rng.choose(NUM_LITS);
    let lit2 = rng.choose(NUM_LITS);

    let ops = &["UNION", "UNION ALL", "INTERSECT", "EXCEPT"];
    let op = rng.choose(ops);

    format!(
        "SELECT {col} FROM {table} WHERE {col} > {lit1} \
         {op} \
         SELECT {col} FROM {table} WHERE {col} < {lit2}"
    )
}

fn gen_complex_multi_join(rng: &mut Rng, min_tables: usize) -> String {
    let join_keys = all_join_keys();
    let target = rng.next_range(min_tables, 12.min(min_tables + 4));

    let mut used_tables: Vec<&str> = Vec::new();
    let mut join_clauses: Vec<String> = Vec::new();

    let first = &join_keys[rng.next_usize(join_keys.len())];
    used_tables.push(first.0);
    used_tables.push(first.2);
    join_clauses.push(format!(
        "JOIN {} ON {}.{} = {}.{}", first.2, first.0, first.1, first.2, first.3
    ));

    for _ in 2..target {
        let candidates: Vec<_> = join_keys
            .iter()
            .filter(|jk| {
                (used_tables.contains(&jk.0) && !used_tables.contains(&jk.2))
                    || (used_tables.contains(&jk.2) && !used_tables.contains(&jk.0))
            })
            .collect();

        if candidates.is_empty() {
            break;
        }

        let jk = candidates[rng.next_usize(candidates.len())];
        if used_tables.contains(&jk.0) {
            used_tables.push(jk.2);
            join_clauses.push(format!(
                "JOIN {} ON {}.{} = {}.{}", jk.2, jk.0, jk.1, jk.2, jk.3
            ));
        } else {
            used_tables.push(jk.0);
            join_clauses.push(format!(
                "JOIN {} ON {}.{} = {}.{}", jk.0, jk.0, jk.1, jk.2, jk.3
            ));
        }
    }

    format!(
        "SELECT * FROM {} {}",
        used_tables[0],
        join_clauses.join(" ")
    )
}

fn gen_round3_query(rng: &mut Rng) -> String {
    match rng.next_usize(100) {
        0..15 => gen_correlated_subquery(rng),
        15..30 => gen_cte_query(rng),
        30..40 => gen_nested_cte(rng),
        40..55 => gen_window_function(rng),
        55..65 => gen_window_with_join(rng),
        65..75 => gen_set_operation(rng),
        75..90 => gen_complex_multi_join(rng, 5),
        _ => gen_complex_multi_join(rng, 8),
    }
}

// ============================================================================
// Query generators — Round 4: OLTP-style (point lookups, short scans)
// ============================================================================

fn gen_point_lookup(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let pk_col = cols[0]; // Assume first column is PK-like
    let id_val = rng.next_range(1, 100_000);

    format!("SELECT * FROM {} WHERE {} = {}", table, pk_col, id_val)
}

fn gen_short_range_scan(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let limit = rng.next_range(1, 20);

    format!(
        "SELECT * FROM {} WHERE {} > {} ORDER BY {} LIMIT {}",
        table, col, rng.choose(NUM_LITS), col, limit
    )
}

fn gen_index_join_lookup(rng: &mut Rng) -> String {
    let join_keys = all_join_keys();
    let jk = join_keys[rng.next_usize(join_keys.len())];
    let id_val = rng.next_range(1, 10_000);

    format!(
        "SELECT * FROM {} JOIN {} ON {}.{} = {}.{} WHERE {}.{} = {}",
        jk.0, jk.2, jk.0, jk.1, jk.2, jk.3,
        jk.0, jk.1, id_val
    )
}

fn gen_offset_pagination(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let limit = rng.next_range(10, 50);
    let offset = rng.next_range(0, 500);
    let dir = if rng.next_usize(2) == 0 { "ASC" } else { "DESC" };

    format!(
        "SELECT * FROM {} ORDER BY {} {} LIMIT {} OFFSET {}",
        table, col, dir, limit, offset
    )
}

fn gen_count_star(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let op = rng.choose(COMP_OPS);
    let lit = rng.choose(NUM_LITS);

    format!("SELECT COUNT(*) FROM {} WHERE {} {} {}", table, col, op, lit)
}

fn gen_exists_check(rng: &mut Rng) -> String {
    let join_keys = all_join_keys();
    let jk = join_keys[rng.next_usize(join_keys.len())];
    let id_val = rng.next_range(1, 50_000);

    format!(
        "SELECT EXISTS (SELECT 1 FROM {} WHERE {} = {})",
        jk.0, jk.1, id_val
    )
}

fn gen_round4_query(rng: &mut Rng) -> String {
    match rng.next_usize(100) {
        0..25 => gen_point_lookup(rng),
        25..45 => gen_short_range_scan(rng),
        45..60 => gen_index_join_lookup(rng),
        60..75 => gen_offset_pagination(rng),
        75..88 => gen_count_star(rng),
        _ => gen_exists_check(rng),
    }
}

// ============================================================================
// Query generators — Round 5: Mixed replay of all patterns
// ============================================================================

fn gen_round5_query(rng: &mut Rng) -> String {
    match rng.next_usize(100) {
        0..15 => gen_round1_query(rng),
        15..30 => gen_round2_query(rng),
        30..50 => gen_round3_query(rng),
        50..65 => gen_round4_query(rng),
        65..75 => gen_star_schema_query(rng),
        75..85 => gen_snowflake_query(rng),
        _ => gen_self_join(rng),
    }
}

fn gen_star_schema_query(rng: &mut Rng) -> String {
    // Star schema: fact table (lineitem/orders) with multiple dimension joins
    let fact = if rng.next_usize(2) == 0 { "lineitem" } else { "orders" };
    let agg_func = rng.choose(AGG_FUNCS);

    if fact == "lineitem" {
        let dims = &[
            ("orders", "l_orderkey", "o_orderkey"),
            ("part", "l_partkey", "p_partkey"),
            ("supplier", "l_suppkey", "s_suppkey"),
        ];
        let ndims = rng.next_range(2, dims.len());
        let join_parts: Vec<String> = dims[..ndims]
            .iter()
            .map(|(d, fk, pk)| format!("JOIN {} ON lineitem.{} = {}.{}", d, fk, d, pk))
            .collect();

        if *agg_func == "COUNT" {
            format!(
                "SELECT supplier.s_name, COUNT(*) FROM lineitem {} GROUP BY supplier.s_name",
                join_parts.join(" ")
            )
        } else {
            format!(
                "SELECT supplier.s_name, {}(lineitem.l_extendedprice) \
                 FROM lineitem {} GROUP BY supplier.s_name",
                agg_func, join_parts.join(" ")
            )
        }
    } else {
        format!(
            "SELECT customer.c_name, {}(orders.o_totalprice) \
             FROM orders JOIN customer ON orders.o_custkey = customer.c_custkey \
             GROUP BY customer.c_name",
            agg_func
        )
    }
}

fn gen_snowflake_query(rng: &mut Rng) -> String {
    // Snowflake: extends star with dimension-to-dimension joins
    let agg_func = rng.choose(AGG_FUNCS);
    let lit = rng.choose(STRING_LITS);

    if *agg_func == "COUNT" {
        format!(
            "SELECT region.r_name, COUNT(*) \
             FROM lineitem \
             JOIN supplier ON lineitem.l_suppkey = supplier.s_suppkey \
             JOIN nation ON supplier.s_nationkey = nation.n_nationkey \
             JOIN region ON nation.n_regionkey = region.r_regionkey \
             WHERE region.r_name = {} \
             GROUP BY region.r_name",
            lit
        )
    } else {
        format!(
            "SELECT region.r_name, {}(lineitem.l_extendedprice) \
             FROM lineitem \
             JOIN supplier ON lineitem.l_suppkey = supplier.s_suppkey \
             JOIN nation ON supplier.s_nationkey = nation.n_nationkey \
             JOIN region ON nation.n_regionkey = region.r_regionkey \
             GROUP BY region.r_name",
            agg_func
        )
    }
}

fn gen_self_join(rng: &mut Rng) -> String {
    let tables = all_tables();
    let (table, cols) = tables[rng.next_usize(tables.len())];
    let col = rng.choose(cols);
    let op = rng.choose(COMP_OPS);

    format!(
        "SELECT a.{col}, b.{col} FROM {} a JOIN {} b ON a.{col} {} b.{col}",
        table, table, op
    )
}

// ============================================================================
// Evaluation: measure route prediction accuracy
// ============================================================================

struct EvalResult {
    total: usize,
    parsed: usize,
    optimized: usize,
    predictions_made: usize,
    correct_predictions: usize,
}

fn evaluate_model(
    coordinator: &SharedTrainingCoordinator,
    opt: &Optimizer,
    rng: &mut Rng,
    count: usize,
) -> EvalResult {
    let model = coordinator.lock().unwrap().current_model();
    let router = SpeculativeRouter::new(model);

    let mut result = EvalResult {
        total: count,
        parsed: 0,
        optimized: 0,
        predictions_made: 0,
        correct_predictions: 0,
    };

    for _ in 0..count {
        let query = match rng.next_usize(5) {
            0 => gen_round1_query(rng),
            1 => gen_round2_query(rng),
            2 => gen_round3_query(rng),
            3 => gen_round4_query(rng),
            _ => gen_round5_query(rng),
        };

        let rel_expr = match sql_to_relexpr(&query) {
            Ok(expr) => {
                result.parsed += 1;
                expr
            }
            Err(_) => continue,
        };

        // Get model prediction
        let features = OptimizationFeatures::from_expr(&rel_expr);
        let prediction = router.predict(&features);

        // Actually optimize and see what path was taken
        match opt.optimize(&rel_expr) {
            Ok(_) => {
                result.optimized += 1;
                result.predictions_made += 1;

                // Determine expected route based on query structure
                let expected = expected_route(&features);
                if prediction.route == expected {
                    result.correct_predictions += 1;
                }
            }
            Err(_) => {}
        }
    }

    result
}

fn expected_route(features: &OptimizationFeatures) -> OptRoute {
    // Heuristic "ground truth" for route classification
    if features.table_count <= 1.0 {
        return OptRoute::Skip;
    }

    if features.equi_join_fraction >= 0.9
        && features.cross_join_present < 0.5
        && features.table_count <= 7.0
        && features.subquery_count < 1.0
        && features.window_count < 1.0
    {
        return OptRoute::LeftDeep;
    }

    if features.table_count <= 3.0
        && features.subquery_count < 1.0
        && features.window_count < 1.0
    {
        return OptRoute::EGraphLow;
    }

    if features.table_count <= 6.0 {
        return OptRoute::EGraphMedium;
    }

    OptRoute::EGraphHigh
}

// ============================================================================
// Optimizer setup
// ============================================================================

fn create_training_optimizer(coordinator: SharedTrainingCoordinator) -> Optimizer {
    let config = OptimizerConfig {
        use_adaptive_limits: true,
        iter_limit: 8,
        node_limit: 8_000,
        time_limit_secs: 1,
        max_optimization_time_ms: 1000,
        ..OptimizerConfig::default()
    };
    let mut opt = Optimizer::with_config(config).with_training(coordinator);

    // TPC-H table stats
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

    // E-commerce table stats
    let ecom_stats: &[(&str, f64, u64, u64)] = &[
        ("users", 500_000.0, 120, 60_000_000),
        ("products", 100_000.0, 200, 20_000_000),
        ("categories", 500.0, 80, 40_000),
        ("inventory", 1_000_000.0, 100, 100_000_000),
        ("payments", 2_000_000.0, 100, 200_000_000),
        ("shipments", 1_500_000.0, 120, 180_000_000),
    ];

    for (table, rows, avg_row, total) in tpch_stats.iter().chain(ecom_stats.iter()) {
        let mut stats = Statistics::new(*rows);
        stats.avg_row_size = *avg_row;
        stats.total_size = *total;
        opt.add_table_stats(*table, stats);
    }

    opt
}

// ============================================================================
// Training round execution
// ============================================================================

struct RoundStats {
    round: usize,
    queries_generated: usize,
    parse_ok: usize,
    parse_fail: usize,
    opt_ok: usize,
    opt_fail: usize,
    duration_ms: u128,
    avg_loss: f32,
    total_traces: u64,
    total_train_steps: usize,
    model_samples: usize,
    eval_accuracy: f32,
}

fn run_training_round(
    round: usize,
    round_name: &str,
    coordinator: &SharedTrainingCoordinator,
    opt: &Optimizer,
    rng: &mut Rng,
    query_gen: fn(&mut Rng) -> String,
) -> RoundStats {
    println!();
    println!("  ╔══════════════════════════════════════════════════════════╗");
    println!("  ║  Round {}: {:48} ║", round, round_name);
    println!("  ╚══════════════════════════════════════════════════════════╝");

    let start = Instant::now();

    // Generate and process queries
    let mut parse_ok: usize = 0;
    let mut parse_fail: usize = 0;
    let mut opt_ok: usize = 0;
    let mut opt_fail: usize = 0;

    for _ in 0..QUERIES_PER_ROUND {
        let sql = query_gen(rng);
        match sql_to_relexpr(&sql) {
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

    // Flush coordinator to process remaining traces
    {
        let mut coord = coordinator.lock().unwrap();
        coord.flush();
    }

    let duration_ms = start.elapsed().as_millis();

    // Collect training stats
    let stats = coordinator.lock().unwrap().stats();

    // Run evaluation
    let eval = evaluate_model(coordinator, opt, rng, EVAL_QUERIES_PER_ROUND);
    let eval_accuracy = if eval.predictions_made > 0 {
        eval.correct_predictions as f32 / eval.predictions_made as f32
    } else {
        0.0
    };

    // Save checkpoint
    let checkpoint_path = format!("models/cost_model.round{}.json", round);
    coordinator.lock().unwrap().save_model(&checkpoint_path).unwrap();

    // Print round results
    println!("  Queries:     {} generated, {} parsed, {} optimized", QUERIES_PER_ROUND, parse_ok, opt_ok);
    println!("  Parse fail:  {} | Opt fail:  {}", parse_fail, opt_fail);
    println!("  Duration:    {}ms", duration_ms);
    println!("  Training:    {} traces, {} steps, loss={:.6}", stats.total_traces, stats.total_train_steps, stats.avg_loss);
    println!("  Samples:     {}", stats.model_samples_trained);
    println!("  Eval:        {}/{} correct ({:.1}% accuracy)",
        eval.correct_predictions, eval.predictions_made, eval_accuracy * 100.0);
    println!("  Checkpoint:  {}", checkpoint_path);

    RoundStats {
        round,
        queries_generated: QUERIES_PER_ROUND,
        parse_ok,
        parse_fail,
        opt_ok,
        opt_fail,
        duration_ms,
        avg_loss: stats.avg_loss,
        total_traces: stats.total_traces,
        total_train_steps: stats.total_train_steps,
        model_samples: stats.model_samples_trained,
        eval_accuracy,
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let total_start = Instant::now();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║       Extended BitNet Cost Model Training Pipeline          ║");
    println!("║       10,000+ queries across 5 progressive rounds           ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // 1. Bootstrap model
    println!("[1/4] Bootstrapping model from heuristic weights...");
    let initial_model = bootstrap_model();
    let coordinator = shared_coordinator_from_model(initial_model);

    // 2. Feed initial bootstrap samples
    println!("[2/4] Feeding bootstrap samples...");
    let bootstrap_samples = generate_bootstrap_samples();
    let bootstrap_count = bootstrap_samples.len();
    {
        let mut coord = coordinator.lock().unwrap();
        coord.train_on_samples(&bootstrap_samples);
    }
    println!("       Fed {} bootstrap samples", bootstrap_count);

    // 3. Create optimizer
    println!("[3/4] Creating optimizer with table stats...");
    let opt = create_training_optimizer(Arc::clone(&coordinator));

    // 4. Ensure models/ directory exists
    println!("[4/4] Ensuring models/ directory exists...");
    std::fs::create_dir_all("models").unwrap();

    // Run 5 training rounds
    let mut rng = Rng::new(RNG_SEED);
    let mut round_stats: Vec<RoundStats> = Vec::with_capacity(5);

    // Round 1: Simple queries
    round_stats.push(run_training_round(
        1,
        "Simple (1-4 tables, equi-joins)",
        &coordinator, &opt, &mut rng,
        gen_round1_query,
    ));

    // Round 2: Medium queries
    round_stats.push(run_training_round(
        2,
        "Medium (3-8 tables, mixed join types)",
        &coordinator, &opt, &mut rng,
        gen_round2_query,
    ));

    // Round 3: Complex queries
    round_stats.push(run_training_round(
        3,
        "Complex (5-12 tables, subqueries, CTEs, windows)",
        &coordinator, &opt, &mut rng,
        gen_round3_query,
    ));

    // Round 4: OLTP-style queries
    round_stats.push(run_training_round(
        4,
        "OLTP (point lookups, short transactions)",
        &coordinator, &opt, &mut rng,
        gen_round4_query,
    ));

    // Round 5: Mixed replay
    round_stats.push(run_training_round(
        5,
        "Mixed replay (all patterns consolidated)",
        &coordinator, &opt, &mut rng,
        gen_round5_query,
    ));

    // Save final model
    println!();
    println!("Saving final model to {}...", FINAL_MODEL_PATH);
    coordinator.lock().unwrap().save_model(FINAL_MODEL_PATH).unwrap();

    // Print comprehensive summary
    let total_duration = total_start.elapsed();
    let final_stats = coordinator.lock().unwrap().stats();

    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                   Training Complete                          ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Round │ Parse │  Opt  │ Loss     │ Accuracy │ Time(ms)     ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    for rs in &round_stats {
        println!(
            "║    {}   │ {:5} │ {:5} │ {:.6} │  {:5.1}%  │ {:8}     ║",
            rs.round, rs.parse_ok, rs.opt_ok, rs.avg_loss,
            rs.eval_accuracy * 100.0, rs.duration_ms
        );
    }
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Totals:                                                    ║");
    println!("║    Queries generated:  {:6}                                ║",
        round_stats.iter().map(|r| r.queries_generated).sum::<usize>());
    println!("║    Queries parsed:     {:6}                                ║",
        round_stats.iter().map(|r| r.parse_ok).sum::<usize>());
    println!("║    Queries optimized:  {:6}                                ║",
        round_stats.iter().map(|r| r.opt_ok).sum::<usize>());
    println!("║    Total traces:       {:6}                                ║", final_stats.total_traces);
    println!("║    Total train steps:  {:6}                                ║", final_stats.total_train_steps);
    println!("║    Final avg loss:     {:.6}                              ║", final_stats.avg_loss);
    println!("║    Samples trained:    {:6}                                ║", final_stats.model_samples_trained);
    println!("║    Bootstrap samples:  {:6}                                ║", bootstrap_count);
    println!("║    Total duration:     {:.2}s                              ║", total_duration.as_secs_f64());
    println!("║    Final model:        {}              ║", FINAL_MODEL_PATH);
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Accuracy progression:                                      ║");
    for rs in &round_stats {
        let bar_len = (rs.eval_accuracy * 30.0) as usize;
        let bar: String = "█".repeat(bar_len) + &"░".repeat(30 - bar_len);
        println!("║    Round {}: {} {:5.1}%      ║", rs.round, bar, rs.eval_accuracy * 100.0);
    }
    println!("╚══════════════════════════════════════════════════════════════╝");
}
