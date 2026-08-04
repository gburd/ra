//! The `route` subcommand — inspect the speculative router's decision.
//!
//! RA-STEERING §7.2 debugger command. Extracts the 16-D
//! `OptimizationFeatures` from a parsed query, runs the heuristic
//! router (no trained model required), and prints the predicted route
//! plus that route's budgets. Thin wrapper over engine internals — no
//! new optimization logic here.
#![expect(clippy::print_stdout, reason = "CLI output")]

use anyhow::Result;
use serde::Serialize;

use ra_engine::speculative_router::{OptRoute, OptimizationFeatures, SpeculativeRouter};
use ra_engine::to_rec_expr;
use ra_parser::sql_to_relexpr;

use crate::output::errors::format_sql_error;

#[derive(Serialize)]
struct RouteReport {
    features: Vec<FeatureValue>,
    prediction: PredictionJson,
    budgets: BudgetsJson,
}

#[derive(Serialize)]
struct FeatureValue {
    name: &'static str,
    value: f32,
}

#[derive(Serialize)]
struct PredictionJson {
    route: String,
    confidence: f32,
    predicted_iterations_needed: u8,
    predicted_cost_improvement_pct: f32,
}

#[derive(Serialize)]
struct BudgetsJson {
    iter_limit: usize,
    timeout_ms: u64,
    rule_application_budget: usize,
    node_growth_budget: usize,
}

const FEATURE_NAMES: [&str; OptimizationFeatures::DIM] = [
    "table_count",
    "join_count",
    "filter_count",
    "aggregate_count",
    "subquery_count",
    "window_count",
    "join_graph_density",
    "max_join_fan_out",
    "equi_join_fraction",
    "cross_join_present",
    "avg_predicate_selectivity",
    "has_limit",
    "has_distinct_or_group",
    "log_estimated_rows",
    "total_table_pages",
    "index_coverage",
];

fn route_name(route: OptRoute) -> &'static str {
    match route {
        OptRoute::Skip => "Skip",
        OptRoute::LeftDeep => "LeftDeep",
        OptRoute::EGraphLow => "EGraphLow",
        OptRoute::EGraphMedium => "EGraphMedium",
        OptRoute::EGraphHigh => "EGraphHigh",
    }
}

pub fn cmd_route(query: &str, format: &str) -> Result<()> {
    let plan = sql_to_relexpr(query).map_err(|e| format_sql_error(&e, query))?;

    let features = OptimizationFeatures::from_expr(&plan);
    let values = features.as_array();
    // No trained model needed: the heuristic fallback routes from the
    // extracted features alone.
    let prediction = SpeculativeRouter::heuristic_fallback(&features);
    let route = prediction.route;

    // node_growth_budget is expressed as a multiple of initial node count;
    // report it against the parsed plan's e-graph size so the number is
    // meaningful. to_rec_expr gives the exact node count.
    let initial_nodes = to_rec_expr(&plan).map_or(1, |r| r.as_ref().len());

    if format.eq_ignore_ascii_case("json") {
        let report = RouteReport {
            features: FEATURE_NAMES
                .iter()
                .zip(values.iter())
                .map(|(&name, &value)| FeatureValue { name, value })
                .collect(),
            prediction: PredictionJson {
                route: route_name(route).to_string(),
                confidence: prediction.confidence,
                predicted_iterations_needed: prediction.predicted_iterations_needed,
                predicted_cost_improvement_pct: prediction.predicted_cost_improvement_pct,
            },
            budgets: BudgetsJson {
                iter_limit: route.iter_limit(),
                timeout_ms: route.timeout_ms(),
                rule_application_budget: route.rule_application_budget(),
                node_growth_budget: route.node_growth_budget(initial_nodes),
            },
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Optimization features (16-D):");
    for (name, value) in FEATURE_NAMES.iter().zip(values.iter()) {
        println!("  {name:<26} {value}");
    }
    println!();
    println!("Route prediction:");
    println!("  route                       {}", route_name(route));
    println!("  confidence                  {:.3}", prediction.confidence);
    println!(
        "  predicted_iterations_needed {}",
        prediction.predicted_iterations_needed
    );
    println!(
        "  predicted_cost_improvement  {:.1}%",
        prediction.predicted_cost_improvement_pct
    );
    println!();
    println!("Route budgets:");
    println!("  iter_limit                  {}", route.iter_limit());
    println!("  timeout_ms                  {}", route.timeout_ms());
    println!(
        "  rule_application_budget     {}",
        route.rule_application_budget()
    );
    println!(
        "  node_growth_budget          {} (initial_nodes={initial_nodes})",
        route.node_growth_budget(initial_nodes)
    );

    Ok(())
}
