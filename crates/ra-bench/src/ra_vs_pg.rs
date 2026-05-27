#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "benchmark binary uses stdout/stderr"
)]
//! Ra vs PostgreSQL head-to-head benchmark.
//!
//! Measures Ra optimizer planning time for a representative set of queries
//! and outputs JSON results that can be compared against PostgreSQL EXPLAIN
//! ANALYZE measurements.

use std::collections::HashMap;
use std::time::Instant;

use ra_core::statistics::Statistics;
use ra_engine::subquery_decorrelation::decorrelate;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use serde::Serialize;

const WARMUP: usize = 5;
const ITERATIONS: usize = 30;

#[derive(Serialize)]
struct QueryResult {
    id: String,
    category: String,
    plan_ms: Vec<f64>,
    success: bool,
    error: Option<String>,
}

fn main() {
    // `--with-stats` flag enables TPC-H SF=0.01 statistics so the Ra
    // optimizer pays the same cardinality-and-index lookup cost the PG
    // planner does on every query. Without it, Ra runs purely on the
    // rule machinery — useful for measuring rewrite throughput in
    // isolation but methodologically asymmetric to PG's catalog-driven
    // planner. See README §"Methodology disclosure".
    let with_stats = std::env::args().any(|a| a == "--with-stats");

    let queries = define_queries();
    let optimizer = if with_stats {
        Optimizer::new().with_table_stats(tpch_sf01_statistics())
    } else {
        Optimizer::new()
    };

    eprintln!(
        "Ra optimizer benchmark: {} queries x {} iterations (+ {} warmup){}",
        queries.len(),
        ITERATIONS,
        WARMUP,
        if with_stats {
            " [with TPC-H SF=0.01 stats]"
        } else {
            ""
        }
    );

    let mut results: Vec<QueryResult> = Vec::new();

    for (i, (id, category, sql)) in queries.iter().enumerate() {
        eprint!("\r  [{}/{}] {:<12}", i + 1, queries.len(), id);

        let mut times = Vec::with_capacity(ITERATIONS);
        let mut success = true;
        let mut error = None;

        for iteration in 0..(WARMUP + ITERATIONS) {
            let start = Instant::now();

            let parsed = match sql_to_relexpr::sql_to_relexpr(sql) {
                Ok(r) => r,
                Err(e) => {
                    success = false;
                    error = Some(format!("parse: {e}"));
                    break;
                }
            };

            let decorrelated = decorrelate(&parsed);
            let opt_result = optimizer.optimize(&decorrelated);

            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

            if let Err(e) = opt_result {
                success = false;
                error = Some(format!("optimize: {e}"));
                break;
            }

            if iteration >= WARMUP {
                times.push(elapsed_ms);
            }
        }

        results.push(QueryResult {
            id: id.to_string(),
            category: category.to_string(),
            plan_ms: times,
            success,
            error,
        });
    }
    eprintln!("\r  Done.{:30}", "");

    // Output JSON to stdout
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}

fn define_queries() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // Level 1: Simple scans
        ("scan_01", "1_simple",
         "SELECT COUNT(*) FROM lineitem WHERE l_shipdate >= '1994-01-01'"),
        ("scan_02", "1_simple",
         "SELECT l_returnflag, l_linestatus, COUNT(*) FROM lineitem \
          GROUP BY l_returnflag, l_linestatus"),
        ("scan_03", "1_simple",
         "SELECT COUNT(*) FROM orders \
          WHERE o_orderdate BETWEEN '1995-01-01' AND '1995-03-31'"),

        // Level 2: Two-table joins
        ("join2_01", "2_join",
         "SELECT COUNT(*) FROM customer c \
          JOIN orders o ON c.c_custkey = o.o_custkey \
          WHERE o.o_orderdate >= '1995-01-01'"),
        ("join2_02", "2_join",
         "SELECT COUNT(*) FROM orders o \
          JOIN lineitem l ON o.o_orderkey = l.l_orderkey \
          WHERE l.l_discount > 0.05"),
        ("join2_03", "2_join",
         "SELECT n.n_name, COUNT(*) FROM nation n \
          JOIN supplier s ON n.n_nationkey = s.s_nationkey GROUP BY n.n_name"),

        // Level 3: Multi-table joins (3-4 tables)
        ("join3_01", "3_multi_join",
         "SELECT COUNT(*) FROM customer c \
          JOIN orders o ON c.c_custkey = o.o_custkey \
          JOIN lineitem l ON o.o_orderkey = l.l_orderkey \
          WHERE l.l_shipdate >= '1994-01-01'"),
        ("join3_02", "3_multi_join",
         "SELECT n.n_name, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue \
          FROM nation n \
          JOIN supplier s ON n.n_nationkey = s.s_nationkey \
          JOIN lineitem l ON s.s_suppkey = l.l_suppkey \
          GROUP BY n.n_name ORDER BY revenue DESC"),
        ("join3_03", "3_multi_join",
         "SELECT r.r_name, COUNT(*) FROM region r \
          JOIN nation n ON r.r_regionkey = n.n_regionkey \
          JOIN customer c ON n.n_nationkey = c.c_nationkey \
          JOIN orders o ON c.c_custkey = o.o_custkey GROUP BY r.r_name"),

        // Level 4: Star schema (5+ tables)
        ("star_01", "4_star_join",
         "SELECT n.n_name, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue \
          FROM customer c \
          JOIN orders o ON c.c_custkey = o.o_custkey \
          JOIN lineitem l ON l.l_orderkey = o.o_orderkey \
          JOIN supplier s ON l.l_suppkey = s.s_suppkey \
          JOIN nation n ON s.s_nationkey = n.n_nationkey \
          WHERE o.o_orderdate >= '1994-01-01' AND o.o_orderdate < '1995-01-01' \
          GROUP BY n.n_name ORDER BY revenue DESC"),
        ("star_02", "4_star_join",
         "SELECT n.n_name, p.p_type, \
          SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue \
          FROM part p \
          JOIN lineitem l ON p.p_partkey = l.l_partkey \
          JOIN supplier s ON l.l_suppkey = s.s_suppkey \
          JOIN orders o ON l.l_orderkey = o.o_orderkey \
          JOIN customer c ON o.o_custkey = c.c_custkey \
          JOIN nation n ON c.c_nationkey = n.n_nationkey \
          WHERE o.o_orderdate BETWEEN '1995-01-01' AND '1996-12-31' \
          GROUP BY n.n_name, p.p_type ORDER BY revenue DESC LIMIT 20"),

        // Level 5: Aggregation + EXISTS
        ("agg_01", "5_aggregation",
         "SELECT c.c_name, COUNT(o.o_orderkey) as order_count, \
          SUM(o.o_totalprice) as total_spent \
          FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey \
          GROUP BY c.c_name HAVING SUM(o.o_totalprice) > 100000 \
          ORDER BY total_spent DESC LIMIT 20"),
        ("agg_02", "5_aggregation",
         "SELECT o_orderpriority, COUNT(*) as order_count FROM orders \
          WHERE o_orderdate >= '1993-07-01' AND o_orderdate < '1993-10-01' \
          AND EXISTS (SELECT 1 FROM lineitem l \
          WHERE l.l_orderkey = orders.o_orderkey \
          AND l.l_commitdate < l.l_receiptdate) \
          GROUP BY o_orderpriority ORDER BY o_orderpriority"),

        // Level 6: Correlated subqueries
        ("corr_01", "6_correlated",
         "SELECT c.c_name, c.c_acctbal FROM customer c \
          WHERE c.c_acctbal > (\
          SELECT AVG(c2.c_acctbal) FROM customer c2 \
          WHERE c2.c_nationkey = c.c_nationkey) \
          ORDER BY c.c_acctbal DESC LIMIT 20"),
        ("corr_02", "6_correlated",
         "SELECT s.s_name FROM supplier s \
          WHERE s.s_suppkey IN (\
          SELECT ps.ps_suppkey FROM partsupp ps \
          WHERE ps.ps_availqty > (\
          SELECT 0.5 * SUM(l.l_quantity) FROM lineitem l \
          WHERE l.l_partkey = ps.ps_partkey \
          AND l.l_suppkey = ps.ps_suppkey \
          AND l.l_shipdate >= '1994-01-01' AND l.l_shipdate < '1995-01-01')\
          ) ORDER BY s.s_name LIMIT 20"),

        // Level 7: TPC-H representative (Q1, Q3, Q5, Q10)
        ("tpch_q1", "7_tpch",
         "SELECT l_returnflag, l_linestatus, SUM(l_quantity) as sum_qty, \
          SUM(l_extendedprice) as sum_base_price, \
          SUM(l_extendedprice * (1 - l_discount)) as sum_disc_price, \
          SUM(l_extendedprice * (1 - l_discount) * (1 + l_tax)) as sum_charge, \
          AVG(l_quantity) as avg_qty, AVG(l_extendedprice) as avg_price, \
          AVG(l_discount) as avg_disc, COUNT(*) as count_order \
          FROM lineitem WHERE l_shipdate <= DATE '1998-12-01' - INTERVAL '90 days' \
          GROUP BY l_returnflag, l_linestatus ORDER BY l_returnflag, l_linestatus"),
        ("tpch_q3", "7_tpch",
         "SELECT l.l_orderkey, \
          SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue, \
          o.o_orderdate, o.o_shippriority \
          FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey \
          JOIN lineitem l ON l.l_orderkey = o.o_orderkey \
          WHERE c.c_mktsegment = 'BUILDING' \
          AND o.o_orderdate < DATE '1995-03-15' \
          AND l.l_shipdate > DATE '1995-03-15' \
          GROUP BY l.l_orderkey, o.o_orderdate, o.o_shippriority \
          ORDER BY revenue DESC, o.o_orderdate LIMIT 10"),
        ("tpch_q5", "7_tpch",
         "SELECT n.n_name, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue \
          FROM customer c \
          JOIN orders o ON c.c_custkey = o.o_custkey \
          JOIN lineitem l ON l.l_orderkey = o.o_orderkey \
          JOIN supplier s ON l.l_suppkey = s.s_suppkey \
          AND c.c_nationkey = s.s_nationkey \
          JOIN nation n ON s.s_nationkey = n.n_nationkey \
          JOIN region r ON n.n_regionkey = r.r_regionkey \
          WHERE r.r_name = 'REGION_0' \
          AND o.o_orderdate >= DATE '1994-01-01' \
          AND o.o_orderdate < DATE '1995-01-01' \
          GROUP BY n.n_name ORDER BY revenue DESC"),
        ("tpch_q10", "7_tpch",
         "SELECT c.c_custkey, c.c_name, \
          SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue, \
          c.c_acctbal, n.n_name, c.c_address, c.c_phone, c.c_comment \
          FROM customer c \
          JOIN orders o ON c.c_custkey = o.o_custkey \
          JOIN lineitem l ON l.l_orderkey = o.o_orderkey \
          JOIN nation n ON c.c_nationkey = n.n_nationkey \
          WHERE o.o_orderdate >= DATE '1993-10-01' \
          AND o.o_orderdate < DATE '1994-01-01' AND l.l_returnflag = 'R' \
          GROUP BY c.c_custkey, c.c_name, c.c_acctbal, c.c_phone, \
          n.n_name, c.c_address, c.c_comment ORDER BY revenue DESC LIMIT 20"),

        // Level 8: Window functions
        ("win_01", "8_window",
         "SELECT c_custkey, c_acctbal, c_nationkey, \
          RANK() OVER (PARTITION BY c_nationkey ORDER BY c_acctbal DESC) as rnk, \
          AVG(c_acctbal) OVER (PARTITION BY c_nationkey) as nation_avg \
          FROM customer ORDER BY c_nationkey, rnk LIMIT 50"),
        ("win_02", "8_window",
         "SELECT o_custkey, o_orderdate, o_totalprice, \
          SUM(o_totalprice) OVER (PARTITION BY o_custkey \
          ORDER BY o_orderdate ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) \
          as running_total FROM orders WHERE o_orderdate >= '1995-01-01' \
          ORDER BY o_custkey, o_orderdate LIMIT 50"),
    ]
}


/// TPC-H SF=0.01 table statistics matching the schema used by
/// `benchmarks/data/run_pg_bench.py`. Used by the `--with-stats`
/// variant so Ra's optimizer pays the same cardinality-and-index lookup
/// cost PG does on every query.
///
/// Numbers come from a fresh `ANALYZE` of a TPC-H 0.01 dataset:
/// lineitem 60175 rows, orders 15000 rows, customer 1500 rows,
/// supplier 100 rows, part 2000 rows, partsupp 8000 rows, nation 25,
/// region 5.
fn tpch_sf01_statistics() -> HashMap<String, Statistics> {
    fn t(rows: f64, avg_row_size: u64) -> Statistics {
        let mut s = Statistics::new(rows);
        s.avg_row_size = avg_row_size;
        s.total_size = (rows as u64).saturating_mul(avg_row_size);
        s
    }
    let mut m = HashMap::new();
    m.insert("lineitem".to_owned(), t(60175.0, 144));
    m.insert("orders".to_owned(), t(15000.0, 112));
    m.insert("customer".to_owned(), t(1500.0, 152));
    m.insert("supplier".to_owned(), t(100.0, 144));
    m.insert("part".to_owned(), t(2000.0, 152));
    m.insert("partsupp".to_owned(), t(8000.0, 144));
    m.insert("nation".to_owned(), t(25.0, 96));
    m.insert("region".to_owned(), t(5.0, 96));
    m
}
