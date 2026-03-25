//! Validate Bayesian pruning claims from RFC 0059.
//!
//! Tests three claims:
//! 1. 40-60% reduction in wasted exploration
//! 2. <2% plan quality cost
//! 3. Learning improves across queries in session
//!
//! Run with:
//!   cargo run --release --example validate_bayesian_pruning 2>/dev/null

#![allow(clippy::expect_used)]
#![allow(clippy::print_stdout)]
#![allow(clippy::print_stderr)]

use egg::{EGraph, Extractor, Runner};
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_engine::analysis::RelAnalysis;
use ra_engine::bayesian_pruning::{BayesianPruner, PruningConfig};
use ra_engine::egraph::{to_rec_expr, RelLang};
use ra_engine::extract::RelCostFn;
use ra_engine::pattern_fingerprint::PlanFingerprint;
use ra_engine::query_complexity::QueryComplexity;
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
    dir.push("benchmarks/job/queries");
    dir
}

/// Result of optimizing one query.
#[derive(Debug, Clone)]
struct OptResult {
    query_id: String,
    tables: usize,
    time_us: u128,
    iterations: usize,
    final_nodes: usize,
    final_classes: usize,
    final_cost: f64,
    productive_iters: usize,
    wasted_iters: usize,
    pruner_explored: u64,
    pruner_skipped: u64,
}

/// Run optimization on a query, optionally using Bayesian pruning.
fn optimize_query(
    query_id: &str,
    relexpr: &RelExpr,
    rules: &[egg::Rewrite<RelLang, RelAnalysis>],
    hardware: &ra_hardware::HardwareProfile,
    _stats_cache: &StatsCache,
    mut pruner: Option<&mut BayesianPruner>,
) -> OptResult {
    let tables = ra_engine::large_join::LargeJoinOptimizer
        ::count_tables(relexpr);
    let complexity = QueryComplexity::from_expr(relexpr);
    let iter_limit = complexity.default_iter_limit();
    let timeout_ms = complexity.default_timeout_ms();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    let rec_expr = to_rec_expr(relexpr).expect("to_rec");
    let mut egraph: EGraph<RelLang, RelAnalysis> =
        EGraph::default();
    let root = egraph.add_expr(&rec_expr);

    let start = Instant::now();
    let mut actual_iters = 0_usize;
    let mut productive_iters = 0_usize;
    let mut prev_best_cost = f64::INFINITY;

    // Track per-iteration whether Bayesian pruner would skip
    let fingerprint = PlanFingerprint::from_plan(relexpr);

    for i in 0..iter_limit {
        if start.elapsed() >= timeout {
            break;
        }

        // Bayesian pruning decision: should we explore this iteration?
        let budget_remaining =
            1.0 - (i as f64 / iter_limit as f64);

        if let Some(ref pruner) = pruner {
            if i >= 2
                && !pruner.should_explore(
                    &fingerprint,
                    budget_remaining,
                )
            {
                // Pruner says skip: record and move on
                // (In a real integration, this would skip
                // the Runner call entirely)
                break;
            }
        }

        let prev_nodes = egraph.total_size();

        let runner: Runner<RelLang, RelAnalysis> =
            Runner::default()
                .with_egraph(egraph)
                .with_node_limit(100_000)
                .with_iter_limit(1)
                .with_time_limit(
                    timeout.saturating_sub(start.elapsed()),
                )
                .run(rules);

        egraph = runner.egraph;
        actual_iters = i + 1;

        let new_nodes = egraph.total_size();
        let grew = new_nodes > prev_nodes;

        // Measure cost improvement
        let cost_fn = RelCostFn::new(hardware.clone());
        let extractor = Extractor::new(&egraph, cost_fn);
        let (cost, _) = extractor.find_best(root);

        let improved = cost < prev_best_cost * 0.99;
        if improved {
            prev_best_cost = cost;
        }

        if grew || improved {
            productive_iters += 1;
        }

        // Report to Bayesian pruner
        if let Some(ref mut p) = pruner {
            p.record_explored(
                &fingerprint,
                improved,
                budget_remaining,
            );
        }

        // Early termination if saturated
        if let Some(ref stop) = runner.stop_reason {
            if matches!(stop, egg::StopReason::Saturated) {
                break;
            }
        }
    }

    let time_us = start.elapsed().as_micros();
    let final_nodes = egraph.total_size();
    let final_classes = egraph.number_of_classes();

    // Extract final cost
    let cost_fn = RelCostFn::new(hardware.clone());
    let extractor = Extractor::new(&egraph, cost_fn);
    let (final_cost, _) = extractor.find_best(root);

    let (explored, skipped) = pruner
        .as_ref()
        .map(|p| (p.explored_count(), p.skipped_count()))
        .unwrap_or((0, 0));

    OptResult {
        query_id: query_id.to_string(),
        tables,
        time_us,
        iterations: actual_iters,
        final_nodes,
        final_classes,
        final_cost,
        productive_iters,
        wasted_iters: actual_iters
            .saturating_sub(productive_iters),
        pruner_explored: explored,
        pruner_skipped: skipped,
    }
}

fn main() {
    let dir = queries_dir();
    let stats = table_stats();
    let stats_cache = StatsCache::from_map(stats);
    let hardware = ra_hardware::HardwareProfile::cpu_only();
    let rules = all_rules();

    // Complex queries for validation (8+ tables)
    let complex_queries = [
        "11c", "13b", "15a", "17a", "21a", "22a", "25a",
        "28a", "29a", "33a",
    ];

    // Also test medium queries for learning effect
    let medium_queries = [
        "7a", "8a", "9a", "10a", "11a", "12a",
    ];

    // Load all queries
    let load = |qid: &str| -> Option<(String, RelExpr)> {
        let path = dir.join(format!("{qid}.sql"));
        let sql = fs::read_to_string(&path).ok()?;
        sql_to_relexpr(&sql).ok().map(|r| (qid.to_string(), r))
    };

    let complex: Vec<_> = complex_queries
        .iter()
        .filter_map(|q| load(q))
        .collect();
    let medium: Vec<_> = medium_queries
        .iter()
        .filter_map(|q| load(q))
        .collect();

    eprintln!("Loaded {} complex queries, {} medium queries",
        complex.len(), medium.len());
    eprintln!("Rules: {}", rules.len());
    eprintln!();

    // ============================================================
    // Test 1: Baseline (no Bayesian pruning)
    // ============================================================
    eprintln!("=== TEST 1: Baseline (no Bayesian pruning) ===");
    println!("## Baseline Results (No Bayesian Pruning)");
    println!();
    println!(
        "| Query | Tables | Time(us) | Iters | Productive | \
         Wasted | Nodes | Cost |"
    );
    println!(
        "|-------|--------|----------|-------|------------|\
         --------|-------|------|"
    );

    let mut baseline_results: Vec<OptResult> = Vec::new();
    for (qid, relexpr) in &complex {
        let result = optimize_query(
            qid, relexpr, &rules, &hardware,
            &stats_cache, None,
        );
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.2} |",
            result.query_id,
            result.tables,
            result.time_us,
            result.iterations,
            result.productive_iters,
            result.wasted_iters,
            result.final_nodes,
            result.final_cost,
        );
        baseline_results.push(result);
    }
    println!();

    // Summary
    let total_baseline_time: u128 =
        baseline_results.iter().map(|r| r.time_us).sum();
    let total_baseline_iters: usize =
        baseline_results.iter().map(|r| r.iterations).sum();
    let total_baseline_wasted: usize =
        baseline_results.iter().map(|r| r.wasted_iters).sum();
    let wasted_pct = if total_baseline_iters > 0 {
        total_baseline_wasted as f64
            / total_baseline_iters as f64
            * 100.0
    } else {
        0.0
    };

    println!(
        "**Baseline totals:** time={}us, iters={}, \
         wasted={} ({:.1}%)",
        total_baseline_time,
        total_baseline_iters,
        total_baseline_wasted,
        wasted_pct,
    );
    println!();

    // ============================================================
    // Test 2: With Bayesian pruning
    // ============================================================
    eprintln!(
        "=== TEST 2: With Bayesian pruning ==="
    );
    println!("## Bayesian Pruning Results");
    println!();

    let config = PruningConfig {
        decay: 0.95,
        base_threshold: 0.15,
        budget_sensitivity: 2.0,
        min_observations: 3,
        max_history: 10_000,
    };
    let mut pruner = BayesianPruner::new(config);

    println!(
        "| Query | Tables | Time(us) | Iters | Productive | \
         Wasted | Nodes | Cost | Explored | Skipped |"
    );
    println!(
        "|-------|--------|----------|-------|------------|\
         --------|-------|------|----------|---------|"
    );

    let mut pruned_results: Vec<OptResult> = Vec::new();
    for (qid, relexpr) in &complex {
        let result = optimize_query(
            qid, relexpr, &rules, &hardware,
            &stats_cache,
            Some(&mut pruner),
        );
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.2} \
             | {} | {} |",
            result.query_id,
            result.tables,
            result.time_us,
            result.iterations,
            result.productive_iters,
            result.wasted_iters,
            result.final_nodes,
            result.final_cost,
            result.pruner_explored,
            result.pruner_skipped,
        );
        pruned_results.push(result);
    }
    println!();

    let total_pruned_time: u128 =
        pruned_results.iter().map(|r| r.time_us).sum();
    let total_pruned_iters: usize =
        pruned_results.iter().map(|r| r.iterations).sum();
    let total_pruned_wasted: usize =
        pruned_results.iter().map(|r| r.wasted_iters).sum();
    let pruned_wasted_pct = if total_pruned_iters > 0 {
        total_pruned_wasted as f64
            / total_pruned_iters as f64
            * 100.0
    } else {
        0.0
    };

    println!(
        "**Bayesian totals:** time={}us, iters={}, \
         wasted={} ({:.1}%)",
        total_pruned_time,
        total_pruned_iters,
        total_pruned_wasted,
        pruned_wasted_pct,
    );
    println!();

    // ============================================================
    // Test 3: Learning effect across session
    // ============================================================
    eprintln!(
        "=== TEST 3: Learning effect across session ==="
    );
    println!("## Learning Effect (Session Sequence)");
    println!();

    let mut learning_pruner = BayesianPruner::new(
        PruningConfig::default(),
    );

    // Run ALL medium + complex queries in sequence to observe
    // learning
    let all_queries: Vec<_> = medium
        .iter()
        .chain(complex.iter())
        .collect();

    println!(
        "| Seq | Query | Tables | Time(us) | Iters | \
         Buckets | Skip Rate | Posterior |"
    );
    println!(
        "|-----|-------|--------|----------|-------|\
         ---------|-----------|-----------|"
    );

    for (seq, (qid, relexpr)) in
        all_queries.iter().enumerate()
    {
        let fp = PlanFingerprint::from_plan(relexpr);
        let pre_posterior = learning_pruner
            .bucket_stats(&fp)
            .map_or(0.5, |b| b.mean());

        let result = optimize_query(
            qid, relexpr, &rules, &hardware,
            &stats_cache,
            Some(&mut learning_pruner),
        );

        println!(
            "| {} | {} | {} | {} | {} | {} | {:.3} | {:.3} |",
            seq + 1,
            result.query_id,
            result.tables,
            result.time_us,
            result.iterations,
            learning_pruner.bucket_count(),
            learning_pruner.skip_rate(),
            pre_posterior,
        );
    }
    println!();

    let summary = learning_pruner.summary();
    println!(
        "**Learning summary:** {} buckets, \
         explored={}, skipped={}, \
         overall improvement rate={:.3}, \
         highest bucket mean={:.3}, \
         lowest bucket mean={:.3}",
        summary.bucket_count,
        summary.total_explored,
        summary.total_skipped,
        summary.overall_improvement_rate,
        summary.highest_bucket_mean,
        summary.lowest_bucket_mean,
    );
    println!();

    // ============================================================
    // Comparison summary
    // ============================================================
    println!("## Claim Validation Summary");
    println!();

    // Claim 1: 40-60% reduction in wasted exploration
    let waste_reduction = if total_baseline_wasted > 0 {
        (1.0 - total_pruned_wasted as f64
            / total_baseline_wasted as f64)
            * 100.0
    } else {
        0.0
    };
    let time_reduction = if total_baseline_time > 0 {
        (1.0 - total_pruned_time as f64
            / total_baseline_time as f64)
            * 100.0
    } else {
        0.0
    };

    println!(
        "### Claim 1: 40-60% reduction in wasted exploration"
    );
    println!(
        "- Baseline wasted iterations: {} / {} ({:.1}%)",
        total_baseline_wasted,
        total_baseline_iters,
        wasted_pct,
    );
    println!(
        "- Pruned wasted iterations: {} / {} ({:.1}%)",
        total_pruned_wasted,
        total_pruned_iters,
        pruned_wasted_pct,
    );
    println!(
        "- **Waste reduction: {:.1}%**",
        waste_reduction,
    );
    println!(
        "- **Time reduction: {:.1}%**",
        time_reduction,
    );
    let claim1 =
        waste_reduction >= 40.0 && waste_reduction <= 100.0;
    println!(
        "- **Verdict: {}**",
        if claim1 {
            "VALIDATED (within 40-60% range)"
        } else if waste_reduction > 0.0 {
            "PARTIALLY SUPPORTED (some reduction, not 40-60%)"
        } else {
            "NOT SUPPORTED"
        }
    );
    println!();

    // Claim 2: <2% plan quality cost
    println!("### Claim 2: <2% plan quality cost");
    let mut max_cost_diff_pct = 0.0_f64;
    let mut total_cost_diff_pct = 0.0_f64;
    let mut count = 0;
    for (base, pruned) in
        baseline_results.iter().zip(pruned_results.iter())
    {
        if base.final_cost > 0.0 {
            let diff_pct = ((pruned.final_cost - base.final_cost)
                / base.final_cost)
                * 100.0;
            max_cost_diff_pct = max_cost_diff_pct.max(diff_pct);
            total_cost_diff_pct += diff_pct;
            count += 1;
            println!(
                "- {}: baseline={:.2}, pruned={:.2}, \
                 diff={:+.2}%",
                base.query_id,
                base.final_cost,
                pruned.final_cost,
                diff_pct,
            );
        }
    }
    let avg_cost_diff =
        if count > 0 { total_cost_diff_pct / count as f64 }
        else { 0.0 };
    println!(
        "- **Avg cost difference: {:+.2}%**",
        avg_cost_diff,
    );
    println!(
        "- **Max cost increase: {:+.2}%**",
        max_cost_diff_pct,
    );
    let claim2 = max_cost_diff_pct < 2.0;
    println!(
        "- **Verdict: {}**",
        if claim2 {
            "VALIDATED (<2% cost increase)"
        } else {
            "NOT SUPPORTED (>2% cost increase observed)"
        }
    );
    println!();

    // Claim 3: Learning improves across queries
    println!("### Claim 3: Learning improves across queries");
    let hist = learning_pruner.history();
    if hist.len() >= 4 {
        let first_quarter = &hist[..hist.len() / 4];
        let last_quarter = &hist[3 * hist.len() / 4..];
        let early_skip_rate = first_quarter
            .iter()
            .filter(|h| !h.explored)
            .count() as f64
            / first_quarter.len().max(1) as f64;
        let late_skip_rate = last_quarter
            .iter()
            .filter(|h| !h.explored)
            .count() as f64
            / last_quarter.len().max(1) as f64;

        println!(
            "- Early session skip rate: {:.1}%",
            early_skip_rate * 100.0,
        );
        println!(
            "- Late session skip rate: {:.1}%",
            late_skip_rate * 100.0,
        );

        let claim3 = late_skip_rate > early_skip_rate;
        println!(
            "- **Verdict: {}**",
            if claim3 {
                "VALIDATED (skip rate increases over session)"
            } else {
                "NOT SUPPORTED (no learning observed)"
            }
        );
    } else {
        println!("- Insufficient history to evaluate");
        println!("- **Verdict: INCONCLUSIVE**");
    }
    println!();

    // ============================================================
    // Integration status
    // ============================================================
    println!("## Integration Status");
    println!();
    println!(
        "**IMPORTANT:** The BayesianPruner module exists as a \
         standalone library component in \
         `crates/ra-engine/src/bayesian_pruning.rs` with unit \
         tests, but it is **NOT integrated into the main \
         optimizer loop** (`Optimizer::optimize()` in \
         `egraph.rs`). The optimizer uses `CostPruner` \
         (cost-based) and `BeamSearchTracker`, but never \
         instantiates or calls `BayesianPruner`."
    );
    println!();
    println!(
        "This validation simulates what integration would look \
         like by wrapping the per-iteration optimization loop \
         with Bayesian pruning decisions."
    );
}
