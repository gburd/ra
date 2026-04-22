//! Demonstration of FTS cost model functionality.
//!
//! Run with: cargo run --example fts_cost_demo

use ra_engine::fts_cost::{
    boolean_query_cost, gin_scan_cost, index_vs_seqscan_speedup, inverted_index_lookup_cost,
    rum_scan_cost as fts_rum_scan_cost, select_fts_index_type, skip_list_intersection_cost,
    top_k_ranking_cost, BooleanOperator as FtsBooleanOperator, FtsIndexType, RankingAlgorithm,
};
use ra_engine::fts_rules::{optimize_top_k_fts, OptimizationDecision};

fn main() {
    println!("=== FTS Cost Model Demo ===\n");

    println!("1. Inverted Index Lookup Costs:");
    let rare_cost = inverted_index_lookup_cost("rare", 1_000_000, 100);
    let common_cost = inverted_index_lookup_cost("common", 1_000_000, 50_000);
    println!("   Rare term (100 docs): {:.2}", rare_cost);
    println!("   Common term (50k docs): {:.2}", common_cost);
    println!("   Cost ratio: {:.2}x\n", common_cost / rare_cost);

    println!("2. Skip-List Intersection:");
    let list_a = 100_000;
    let list_b = 50_000;
    let skip_cost = skip_list_intersection_cost(list_a, list_b);
    let linear_estimate = (list_a + list_b) as f64 * 0.3;
    println!("   Skip-list cost: {:.2}", skip_cost);
    println!("   Linear estimate: {:.2}", linear_estimate);
    println!("   Speedup: {:.2}x\n", linear_estimate / skip_cost);

    println!("3. Boolean Query Costs:");
    let terms = vec!["rust", "language", "optimization"];
    let freqs = vec![10_000, 20_000, 5_000];
    let and_cost = boolean_query_cost(&terms, FtsBooleanOperator::And, 1_000_000, &freqs);
    let phrase_cost = boolean_query_cost(&terms, FtsBooleanOperator::Phrase, 1_000_000, &freqs);
    println!(
        "   AND query: CPU={:.2}, IO={:.2}",
        and_cost.cpu, and_cost.io
    );
    println!(
        "   PHRASE query: CPU={:.2}, IO={:.2}",
        phrase_cost.cpu, phrase_cost.io
    );
    println!(
        "   Phrase overhead: {:.2}x\n",
        phrase_cost.cpu / and_cost.cpu
    );

    println!("4. Top-K Ranking Optimization:");
    let matches = 100_000;
    let no_limit = top_k_ranking_cost(matches, RankingAlgorithm::Bm25, None);
    let with_limit = top_k_ranking_cost(matches, RankingAlgorithm::Bm25, Some(10));
    println!("   Without limit: {:.2}", no_limit);
    println!("   With LIMIT 10: {:.2}", with_limit);
    println!("   Speedup: {:.2}x\n", no_limit / with_limit);

    println!("5. Index Type Selection:");
    let small_table = select_fts_index_type(FtsBooleanOperator::And, false, 500);
    let large_ranked = select_fts_index_type(FtsBooleanOperator::And, true, 1_000_000);
    let phrase_ranked = select_fts_index_type(FtsBooleanOperator::Phrase, true, 100_000);
    println!("   Small table (500 rows): {:?}", small_table);
    println!("   Large table ranked: {:?}", large_ranked);
    println!("   Phrase query ranked: {:?}\n", phrase_ranked);

    println!("6. Index vs Sequential Scan Speedup:");
    let gin_speedup = index_vs_seqscan_speedup(1_000_000, 100, FtsIndexType::Gin);
    let rum_speedup = index_vs_seqscan_speedup(1_000_000, 100, FtsIndexType::Rum);
    println!("   GIN (100/1M docs): {:.1}x faster", gin_speedup);
    println!("   RUM (100/1M docs): {:.1}x faster\n", rum_speedup);

    println!("7. GIN vs RUM Comparison:");
    let terms_single = vec!["search"];
    let freqs_single = vec![10_000];
    let gin = gin_scan_cost(
        &terms_single,
        FtsBooleanOperator::And,
        1_000_000,
        &freqs_single,
        true,
        Some(10),
    );
    let rum = fts_rum_scan_cost(
        &terms_single,
        FtsBooleanOperator::And,
        1_000_000,
        &freqs_single,
        true,
        Some(10),
    );
    println!("   GIN ranked LIMIT 10: CPU={:.2}", gin.cpu);
    println!("   RUM ranked LIMIT 10: CPU={:.2}", rum.cpu);
    println!("   RUM advantage: {:.2}x\n", gin.cpu / rum.cpu);

    println!("8. Optimization Decision:");
    let decision = optimize_top_k_fts(
        true,
        false,
        Some(10),
        &terms_single,
        1_000_000,
        &freqs_single,
    );
    match decision {
        OptimizationDecision::UseRumRankedScan { cost, limit } => {
            println!("   Decision: Use RUM ranked scan");
            println!("   Estimated cost: {:.2}", cost);
            println!("   Limit: {}", limit);
        }
        OptimizationDecision::UseGinWithSort { cost } => {
            println!("   Decision: Use GIN with sort");
            println!("   Estimated cost: {:.2}", cost);
        }
        OptimizationDecision::NoOptimization => {
            println!("   Decision: No FTS optimization");
        }
    }

    println!("\n=== Demo Complete ===");
}
