//! The `egraph` subcommand — inspect the equality-saturation e-graph.
//!
//! RA-STEERING §7.2 debugger command. Builds the e-graph for a query,
//! runs equality saturation with the full rewrite set, and reports
//! e-class / e-node counts and the saturation stop reason. `--extract-top
//! <n>` lists the n lowest-cost equivalent top-level plans with their
//! costs; `--dot` emits Graphviz. Thin wrapper over engine internals
//! (`to_rec_expr`, `rewrite::all_rules`, egg's Runner/Extractor,
//! `rec_expr_to_rel_expr`).
#![expect(clippy::print_stdout, reason = "CLI output")]

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use egg::{CostFunction, EGraph, Extractor, Language, RecExpr, Runner};
use serde::Serialize;

use ra_engine::analysis::RelAnalysis;
use ra_engine::cost::{IntegratedCostFn, LiveConditions};
use ra_engine::extract::rec_expr_to_rel_expr;
use ra_engine::rewrite::all_rules;
use ra_engine::{to_rec_expr, RelLang};
use ra_parser::sql_to_relexpr;

use crate::display::format_plan_tree;
use crate::helpers::load_hardware_profile;
use crate::output::errors::format_sql_error;

#[derive(Serialize)]
struct EgraphReport {
    query: String,
    initial_nodes: usize,
    eclasses: usize,
    enodes: usize,
    iterations: usize,
    stop_reason: String,
    top_plans: Vec<TopPlan>,
}

#[derive(Serialize)]
struct TopPlan {
    rank: usize,
    cost: f64,
    plan: String,
}

pub fn cmd_egraph(query: &str, extract_top: Option<usize>, dot: bool, format: &str) -> Result<()> {
    let plan = sql_to_relexpr(query).map_err(|e| format_sql_error(&e, query))?;
    let rec = to_rec_expr(&plan).map_err(|e| anyhow::anyhow!("e-graph conversion failed: {e}"))?;
    let initial_nodes = rec.as_ref().len();

    // Build + saturate with the full rewrite set. Node-limit keeps this
    // bounded for the CLI; the engine's own routing budgets are not applied
    // here since this is a debugging view of the raw saturated e-graph.
    let runner = Runner::<RelLang, RelAnalysis>::default()
        .with_node_limit(50_000)
        .with_iter_limit(30)
        .with_expr(&rec)
        .run(&all_rules());

    let root = runner.roots[0];
    let egraph = &runner.egraph;
    let stop_reason = format!("{:?}", runner.stop_reason);
    let iterations = runner.iterations.len();

    if dot {
        // Graphviz output goes to stdout regardless of --format.
        println!("{}", egraph.dot());
        return Ok(());
    }

    let top_plans = match extract_top {
        Some(n) if n > 0 => extract_top_plans(egraph, root, n)?,
        _ => Vec::new(),
    };

    if format.eq_ignore_ascii_case("json") {
        let report = EgraphReport {
            query: query.to_string(),
            initial_nodes,
            eclasses: egraph.number_of_classes(),
            enodes: egraph.total_number_of_nodes(),
            iterations,
            stop_reason,
            top_plans,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("E-graph for query:");
    println!("  initial nodes    {initial_nodes}");
    println!("  e-classes        {}", egraph.number_of_classes());
    println!("  e-nodes          {}", egraph.total_number_of_nodes());
    println!("  iterations       {iterations}");
    println!("  stop reason      {stop_reason}");

    if !top_plans.is_empty() {
        println!("\nTop {} lowest-cost equivalent plans:", top_plans.len());
        for p in &top_plans {
            println!("\n  #{} (cost {:.2}):", p.rank, p.cost);
            for line in p.plan.lines() {
                println!("    {line}");
            }
        }
    }

    Ok(())
}

/// Extract up to `n` distinct lowest-cost equivalent plans.
///
/// The optimum is the plan `find_best_node` selects at every e-class.
/// Additional distinct plans are single-substitution neighbors: walk the
/// e-classes on the optimal plan's path, and for each alternative e-node
/// in one of those classes, build the plan that forces that alternative
/// and keeps the cheapest choice everywhere else. Restricting to the
/// optimal path (rather than all e-classes) keeps this bounded and
/// cycle-free on large saturated e-graphs — no new optimization logic,
/// just extraction.
fn extract_top_plans(
    egraph: &EGraph<RelLang, RelAnalysis>,
    root: egg::Id,
    n: usize,
) -> Result<Vec<TopPlan>> {
    let hardware = load_hardware_profile("auto")?;
    let cost_fn = IntegratedCostFn::new(hardware.clone(), HashMap::new(), HashMap::new())
        .with_live_conditions(LiveConditions::NEUTRAL);
    let extractor = Extractor::new(egraph, cost_fn);

    let root = egraph.find(root);

    // Collect the e-classes on the optimal plan's path (root + best-node
    // descendants). This is a small, acyclic set.
    let mut path: Vec<egg::Id> = Vec::new();
    let mut seen_class: HashSet<egg::Id> = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let id = egraph.find(id);
        if !seen_class.insert(id) {
            continue;
        }
        path.push(id);
        for child in extractor.find_best_node(id).children() {
            stack.push(*child);
        }
    }

    // Candidate = force one (e-class on the path, node index) alternative;
    // (root, best) reproduces the plain optimum.
    let mut candidates: Vec<(f64, String)> = Vec::new();
    let build_forced = |forced_class: egg::Id, forced_idx: usize| -> Result<(f64, String)> {
        let root_node = if forced_class == root {
            egraph[root].nodes[forced_idx].clone()
        } else {
            extractor.find_best_node(root).clone()
        };
        // e-graphs can contain cycles; forcing a non-best node can expose
        // one, and build_recexpr would then loop re-expanding the cycle.
        // Bound the walk by total get_node calls: past the cap, hand back a
        // childless placeholder so the walk terminates. Such a plan won't
        // round-trip to a valid RelExpr and is dropped below — never emitted.
        // ponytail: hard call cap instead of true cycle detection; upgrade
        // to a proper k-best DP if the top-N view needs deep alternatives.
        let cap = 8 * egraph.total_number_of_nodes().max(256);
        let calls = std::cell::Cell::new(0usize);
        let expr: RecExpr<RelLang> = root_node.build_recexpr(|id| {
            calls.set(calls.get() + 1);
            if calls.get() > cap {
                return RelLang::Symbol("__cycle_cutoff".into());
            }
            let id = egraph.find(id);
            if id == forced_class {
                egraph[id].nodes[forced_idx].clone()
            } else {
                extractor.find_best_node(id).clone()
            }
        });
        let cost = cost_recexpr(hardware.clone(), &expr);
        let rel = rec_expr_to_rel_expr(&expr)
            .map_err(|e| anyhow::anyhow!("plan reconstruction failed: {e}"))?;
        Ok((cost, format_plan_tree(&rel)))
    };

    for &class in &path {
        for idx in 0..egraph[class].nodes.len() {
            // Skip candidates that fail to reconstruct (e.g. a cycle-cutoff
            // placeholder); the best plan is always among the successful ones.
            if let Ok(c) = build_forced(class, idx) {
                candidates.push(c);
            }
        }
    }

    // Dedup by rendered plan (keeping the lowest cost), sort, truncate.
    candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (cost, plan) in candidates {
        if seen.insert(plan.clone()) {
            out.push(TopPlan {
                rank: out.len() + 1,
                cost,
                plan,
            });
            if out.len() == n {
                break;
            }
        }
    }
    Ok(out)
}

/// Cost a finished `RecExpr` by folding `IntegratedCostFn` bottom-up.
/// egg guarantees children have lower indices than their parent, so a
/// single forward pass suffices.
fn cost_recexpr(hardware: ra_hardware::HardwareProfile, expr: &RecExpr<RelLang>) -> f64 {
    use ra_engine::cost::PlanCost;
    let mut cost_fn = IntegratedCostFn::new(hardware, HashMap::new(), HashMap::new())
        .with_live_conditions(LiveConditions::NEUTRAL);
    let nodes = expr.as_ref();
    let mut costs: Vec<PlanCost> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let c = cost_fn.cost(node, |id| costs[usize::from(id)]);
        costs.push(c);
    }
    costs.last().map_or(0.0, |c| c.total_cost)
}
