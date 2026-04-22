//! Profile all 113 JOB queries and output timing data as CSV.
//!
//! Run with:
//!   cargo run --release --example profile_all_job 2>/dev/null

#![allow(clippy::expect_used)]

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

fn count_tables(sql: &str) -> usize {
    // Count FROM-clause tables by counting commas + 1
    // JOB queries use comma-separated FROM clause
    let lower = sql.to_lowercase();
    let from_pos = lower.find("from ");
    let where_pos = lower.find("where ");
    if let (Some(f), Some(w)) = (from_pos, where_pos) {
        let from_clause = &sql[f + 5..w];
        from_clause.split(',').count()
    } else {
        0
    }
}

fn main() {
    let dir = queries_dir();
    let optimizer = make_optimizer();

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .expect("cannot read queries dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "sql"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    println!("query,tables,parse_us,optimize_us,status,tables_parsed");

    let mut total_parse = 0u128;
    let mut total_opt = 0u128;
    let mut success = 0u32;
    let mut failures = 0u32;
    let mut parse_failures = 0u32;

    for entry in &entries {
        let path = entry.path();
        let query_id = path
            .file_stem()
            .expect("stem")
            .to_str()
            .expect("utf8")
            .to_owned();

        let sql = fs::read_to_string(&path).expect("read sql");
        let table_count = count_tables(&sql);

        let parse_start = Instant::now();
        let relexpr = match sql_to_relexpr(&sql) {
            Ok(r) => r,
            Err(e) => {
                let parse_us = parse_start.elapsed().as_micros();
                println!(
                    "{query_id},{table_count},{parse_us},0,\
                     parse_error,0"
                );
                parse_failures += 1;
                eprintln!("PARSE FAIL {query_id}: {e}");
                continue;
            }
        };
        let parse_us = parse_start.elapsed().as_micros();

        let table_count_parsed = ra_engine::large_join::LargeJoinOptimizer::count_tables(&relexpr);

        let opt_start = Instant::now();
        let status = match optimizer.optimize(&relexpr) {
            Ok(_) => {
                success += 1;
                "ok"
            }
            Err(e) => {
                failures += 1;
                eprintln!("OPT FAIL {query_id}: {e}");
                "opt_error"
            }
        };
        let opt_us = opt_start.elapsed().as_micros();

        total_parse += parse_us;
        total_opt += opt_us;

        println!(
            "{query_id},{table_count},{parse_us},{opt_us},\
             {status},{table_count_parsed}"
        );
    }

    eprintln!("\n=== Summary ===");
    eprintln!("Queries:        {}", entries.len());
    eprintln!("Success:        {success}");
    eprintln!("Opt failures:   {failures}");
    eprintln!("Parse failures: {parse_failures}");
    eprintln!("Total parse:    {:.1}ms", total_parse as f64 / 1000.0);
    eprintln!("Total optimize: {:.1}ms", total_opt as f64 / 1000.0);
    eprintln!(
        "Avg optimize:   {:.1}ms",
        total_opt as f64 / 1000.0 / entries.len() as f64
    );
}
