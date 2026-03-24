//! Quick timing measurement for JOB queries.
//!
//! Run with: cargo run --example job_timing --package ra-engine

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

fn main() {
    let optimizer = make_optimizer();
    let dir = queries_dir();

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "sql")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut results: Vec<(String, f64, bool, String)> = Vec::new();
    let mut parse_failures = 0;
    let mut optimize_failures = 0;

    println!(
        "{:<8} {:>10} {:>10} {:<10} {}",
        "Query", "Parse(us)", "Opt(us)", "Status", "Notes"
    );
    println!("{}", "-".repeat(70));

    for entry in &entries {
        let path = entry.path();
        let query_id = path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let sql = fs::read_to_string(&path).unwrap();

        let parse_start = Instant::now();
        let relexpr = match sql_to_relexpr(&sql) {
            Ok(r) => r,
            Err(e) => {
                println!(
                    "{:<8} {:>10} {:>10} {:<10} {}",
                    query_id, "-", "-", "PARSE_ERR",
                    format!("{e}")
                        .chars()
                        .take(40)
                        .collect::<String>()
                );
                parse_failures += 1;
                continue;
            }
        };
        let parse_us = parse_start.elapsed().as_micros();

        let opt_start = Instant::now();
        match optimizer.optimize(&relexpr) {
            Ok(_optimized) => {
                let opt_us = opt_start.elapsed().as_micros();
                println!(
                    "{:<8} {:>10} {:>10} {:<10}",
                    query_id, parse_us, opt_us, "OK"
                );
                results.push((
                    query_id,
                    opt_us as f64,
                    true,
                    String::new(),
                ));
            }
            Err(e) => {
                let opt_us = opt_start.elapsed().as_micros();
                println!(
                    "{:<8} {:>10} {:>10} {:<10} {}",
                    query_id,
                    parse_us,
                    opt_us,
                    "OPT_ERR",
                    format!("{e}")
                        .chars()
                        .take(40)
                        .collect::<String>()
                );
                optimize_failures += 1;
                results.push((
                    query_id,
                    opt_us as f64,
                    false,
                    format!("{e}"),
                ));
            }
        }
    }

    println!();
    println!("=== Summary ===");
    println!("Total queries:    {}", entries.len());
    println!("Parse failures:   {parse_failures}");
    println!("Optimize failures:{optimize_failures}");

    let successful: Vec<_> =
        results.iter().filter(|r| r.2).collect();
    if !successful.is_empty() {
        let total_us: f64 =
            successful.iter().map(|r| r.1).sum();
        let avg_us = total_us / successful.len() as f64;
        let max = successful
            .iter()
            .max_by(|a, b| {
                a.1.partial_cmp(&b.1).unwrap()
            })
            .unwrap();
        let min = successful
            .iter()
            .min_by(|a, b| {
                a.1.partial_cmp(&b.1).unwrap()
            })
            .unwrap();

        println!(
            "Successful:       {} / {}",
            successful.len(),
            results.len()
        );
        println!("Total opt time:   {:.0} us ({:.1} ms)", total_us, total_us / 1000.0);
        println!("Avg opt time:     {:.0} us ({:.1} ms)", avg_us, avg_us / 1000.0);
        println!(
            "Min opt time:     {:.0} us ({})",
            min.1, min.0
        );
        println!(
            "Max opt time:     {:.0} us ({})",
            max.1, max.0
        );

        // Breakdown by query complexity
        let mut by_template: std::collections::BTreeMap<u32, Vec<f64>> =
            std::collections::BTreeMap::new();
        for r in &successful {
            let num: u32 = r.0
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            by_template
                .entry(num)
                .or_default()
                .push(r.1);
        }

        println!();
        println!(
            "{:<12} {:>8} {:>12} {:>12}",
            "Template", "Queries", "Avg(us)", "Max(us)"
        );
        println!("{}", "-".repeat(50));
        for (template, times) in &by_template {
            let avg =
                times.iter().sum::<f64>() / times.len() as f64;
            let max = times
                .iter()
                .copied()
                .fold(0.0_f64, f64::max);
            println!(
                "{:<12} {:>8} {:>12.0} {:>12.0}",
                template,
                times.len(),
                avg,
                max
            );
        }

        // Queries over 1ms, 10ms, 100ms, 1s
        let over_1ms =
            successful.iter().filter(|r| r.1 > 1000.0).count();
        let over_10ms =
            successful.iter().filter(|r| r.1 > 10_000.0).count();
        let over_100ms =
            successful.iter().filter(|r| r.1 > 100_000.0).count();
        let over_1s =
            successful.iter().filter(|r| r.1 > 1_000_000.0).count();
        let over_5s =
            successful.iter().filter(|r| r.1 > 5_000_000.0).count();

        println!();
        println!("Queries >1ms:   {over_1ms}");
        println!("Queries >10ms:  {over_10ms}");
        println!("Queries >100ms: {over_100ms}");
        println!("Queries >1s:    {over_1s}");
        println!("Queries >5s:    {over_5s}");
    }
}
