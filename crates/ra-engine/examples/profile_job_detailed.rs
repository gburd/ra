#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "example binary uses stdout/stderr"
)]
//! Detailed profiling of JOB queries showing optimizer phases.
//!
//! Run with:
//!   `RUST_LOG=ra_engine=info cargo run --release --example profile_job_detailed 2>&1`

#![expect(clippy::expect_used)]

use egg::{EGraph, Runner};
use ra_core::statistics::Statistics;
use ra_engine::analysis::RelAnalysis;
use ra_engine::egraph::{to_rec_expr, OptimizerConfig, RelLang};
use ra_engine::extract::extract_best;
use ra_engine::rewrite::all_rules;
use ra_engine::stats_cache::StatsCache;
use ra_parser::sql_to_relexpr;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn mk_stats(rows: f64, avg: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg;
    s.total_size = (rows as u64) * avg;
    s
}

fn table_stats() -> HashMap<String, Statistics> {
    let mut m = HashMap::new();
    for (name, rows, sz) in [
        ("aka_name", 901_343.0, 100_u64),
        ("aka_title", 361_472.0, 150),
        ("cast_info", 36_244_344.0, 80),
        ("char_name", 3_140_339.0, 90),
        ("comp_cast_type", 4.0, 50),
        ("company_name", 234_997.0, 120),
        ("company_type", 4.0, 50),
        ("complete_cast", 135_086.0, 60),
        ("info_type", 113.0, 50),
        ("keyword", 134_170.0, 100),
        ("kind_type", 7.0, 50),
        ("link_type", 18.0, 50),
        ("movie_companies", 2_609_129.0, 100),
        ("movie_info", 14_835_720.0, 150),
        ("movie_info_idx", 1_380_035.0, 100),
        ("movie_keyword", 4_523_930.0, 60),
        ("movie_link", 29_997.0, 80),
        ("name", 4_167_491.0, 110),
        ("person_info", 2_963_664.0, 120),
        ("role_type", 12.0, 50),
        ("title", 2_528_312.0, 180),
    ] {
        m.insert(name.to_string(), mk_stats(rows, sz));
    }
    m
}

fn queries_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir.pop();
    dir.push("benchmarks");
    dir.push("job");
    dir.push("queries");
    dir
}

fn main() {
    let dir = queries_dir();
    let stats = table_stats();
    let stats_cache = StatsCache::from_map(stats.clone());
    let hardware = ra_hardware::detect_hardware();
    let rules = all_rules();
    let config = OptimizerConfig::default();

    eprintln!("Rules loaded: {}", rules.len());
    eprintln!(
        "Config: node_limit={}, adaptive={}",
        config.node_limit, config.use_adaptive_limits
    );

    // Select representative queries: 3b (4 tab), 5a (5 tab),
    // 10c (7 tab), 13b (9 tab), 17a (7 tab)
    let targets = ["3b", "5a", "10c", "13b", "17a", "11c"];

    println!(
        "query,tables,parse_us,to_rec_us,egraph_us,\
         extract_us,total_us,iterations,nodes,classes,\
         termination"
    );

    for target in &targets {
        let path = dir.join(format!("{target}.sql"));
        let sql = fs::read_to_string(&path).expect("read");
        let table_count = ra_engine::large_join::LargeJoinOptimizer::count_tables;

        let parse_start = Instant::now();
        let relexpr = sql_to_relexpr(&sql).expect("parse");
        let parse_us = parse_start.elapsed().as_micros();

        let tables = table_count(&relexpr);

        let rec_start = Instant::now();
        let rec_expr = to_rec_expr(&relexpr).expect("to_rec");
        let rec_us = rec_start.elapsed().as_micros();

        // Determine adaptive limits based on table count
        let iter_limit = match tables {
            0..=1 => 3,
            2..=4 => 5,
            5..=7 => 10,
            8..=9 => 15,
            _ => 20,
        };
        let timeout_ms: u64 = match tables {
            0..=1 => 50,
            2..=4 => 200,
            5..=7 => 500,
            8..=9 => 2000,
            _ => 5000,
        };

        eprintln!(
            "\n--- {target}: {tables} tables, \
             iter_limit={iter_limit}, timeout={timeout_ms}ms ---"
        );

        let egraph_start = Instant::now();
        let mut egraph: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let root = egraph.add_expr(&rec_expr);

        let timeout = std::time::Duration::from_millis(timeout_ms);
        let mut actual_iters = 0_usize;
        let mut termination = "iter_limit";

        for i in 0..iter_limit {
            if egraph_start.elapsed() >= timeout {
                termination = "timeout";
                break;
            }

            let prev_nodes = egraph.total_size();
            let runner: Runner<RelLang, RelAnalysis> = Runner::default()
                .with_egraph(egraph)
                .with_node_limit(config.node_limit)
                .with_iter_limit(1)
                .with_time_limit(timeout.saturating_sub(egraph_start.elapsed()))
                .run(&rules);

            egraph = runner.egraph;
            actual_iters = i + 1;

            let new_nodes = egraph.total_size();
            eprintln!(
                "  iter {i}: nodes {prev_nodes} -> {new_nodes} \
                 (+{}), classes={}, elapsed={:?}",
                new_nodes - prev_nodes,
                egraph.number_of_classes(),
                egraph_start.elapsed()
            );

            if let Some(ref stop) = runner.stop_reason {
                if matches!(stop, egg::StopReason::Saturated) {
                    termination = "saturated";
                    break;
                }
            }
        }

        let egraph_us = egraph_start.elapsed().as_micros();
        let nodes = egraph.total_size();
        let classes = egraph.number_of_classes();

        let extract_start = Instant::now();
        let _result = extract_best(&egraph, root, stats_cache.as_map(), &hardware, ra_engine::LiveConditions::NEUTRAL);
        let extract_us = extract_start.elapsed().as_micros();

        let total_us = parse_us + rec_us + egraph_us + extract_us;

        println!(
            "{target},{tables},{parse_us},{rec_us},\
             {egraph_us},{extract_us},{total_us},\
             {actual_iters},{nodes},{classes},{termination}"
        );
    }
}
