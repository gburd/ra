//! Diagnostic: which optimization path each JOB query takes.
//!
//! Run with: cargo run --example job_diagnostic --package ra-engine --release

use ra_core::statistics::Statistics;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn mk_stats(rows: f64, avg: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg;
    s.total_size = (rows as u64) * avg;
    s
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
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
        opt.add_table_stats(name, mk_stats(rows, sz));
    }
    opt
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

fn count_tables(expr: &ra_core::algebra::RelExpr) -> usize {
    use ra_core::algebra::RelExpr;
    match expr {
        RelExpr::Scan { .. } => 1,
        RelExpr::Join { left, right, .. } => {
            count_tables(left) + count_tables(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => count_tables(input),
        _ => 0,
    }
}

fn top_level_variant(
    expr: &ra_core::algebra::RelExpr,
) -> &'static str {
    use ra_core::algebra::RelExpr;
    match expr {
        RelExpr::Scan { .. } => "Scan",
        RelExpr::Filter { .. } => "Filter",
        RelExpr::Project { .. } => "Project",
        RelExpr::Join { .. } => "Join",
        RelExpr::Aggregate { .. } => "Aggregate",
        RelExpr::Sort { .. } => "Sort",
        RelExpr::Limit { .. } => "Limit",
        RelExpr::Window { .. } => "Window",
        RelExpr::Distinct { .. } => "Distinct",
        RelExpr::Union { .. } => "Union",
        RelExpr::CTE { .. } => "CTE",
        _ => "Other",
    }
}

fn main() {
    let _optimizer = make_optimizer();
    let dir = queries_dir();

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!("cannot read {}: {e}", dir.display())
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "sql")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    // First: measure all_rules() cost
    let rules_start = Instant::now();
    let rules = ra_engine::rewrite::all_rules();
    let rules_us = rules_start.elapsed().as_micros();
    println!(
        "all_rules() construction: {} rules in {} us ({:.1} ms)",
        rules.len(),
        rules_us,
        rules_us as f64 / 1000.0
    );
    drop(rules);

    // Second: measure to_rec_expr cost
    println!();
    println!(
        "{:<8} {:>6} {:>10} {:>10} {:>10} {:<12}",
        "Query", "Tables", "Parse(us)", "LeftDeep?", "TopNode", "Path"
    );
    println!("{}", "-".repeat(70));

    let mut left_deep_count = 0;
    let mut egraph_count = 0;
    let mut large_join_count = 0;

    for entry in &entries {
        let path = entry.path();
        let query_id = path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let sql = fs::read_to_string(&path).unwrap();

        let relexpr = match sql_to_relexpr(&sql) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let tables = count_tables(&relexpr);
        let top = top_level_variant(&relexpr);
        let eligible =
            ra_engine::left_deep::can_use_left_deep(&relexpr);

        let path_name = if eligible {
            left_deep_count += 1;
            "left-deep"
        } else if tables >= 10 {
            large_join_count += 1;
            "large-join"
        } else {
            egraph_count += 1;
            "e-graph"
        };

        println!(
            "{:<8} {:>6} {:>10} {:>10} {:>10} {:<12}",
            query_id, tables, "-", eligible, top, path_name
        );
    }

    println!();
    println!("=== Path Distribution ===");
    println!("Left-deep:  {left_deep_count}");
    println!("E-graph:    {egraph_count}");
    println!("Large-join: {large_join_count}");
}
