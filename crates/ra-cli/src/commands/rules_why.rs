//! The `rules why` subcommand — show which rewrite rules fired.
//!
//! RA-STEERING §7.2 debugger command. Runs the optimizer with rule
//! tracking (`Optimizer::optimize_with_tracking`) and reports the rules
//! that fired, in order, with fire counts and — when verbose tracking
//! captured them — the per-step before/after plans. Thin wrapper: all
//! tracking is done by the engine.
#![expect(clippy::print_stdout, reason = "CLI output")]

use anyhow::Result;
use serde::Serialize;

use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;

use crate::display::format_plan_tree;
use crate::output::errors::format_sql_error;

#[derive(Serialize)]
struct RulesWhyReport {
    query: String,
    fired_rule_count: usize,
    fired_rules: Vec<FiredRule>,
    steps: Vec<StepJson>,
}

#[derive(Serialize)]
struct FiredRule {
    name: String,
    fired_count: usize,
    nodes_added: usize,
    cost_improvement: Option<f64>,
}

#[derive(Serialize)]
struct StepJson {
    step_number: usize,
    rule_name: String,
    reason: String,
    plan_before: String,
    plan_after: String,
    cost_improvement: Option<f64>,
}

pub fn cmd_rules_why(query: &str, format: &str, verbose: bool) -> Result<()> {
    let plan = sql_to_relexpr(query).map_err(|e| format_sql_error(&e, query))?;

    let optimizer = Optimizer::new();
    // Verbose tracking also captures per-step before/after plans.
    let result = optimizer
        .optimize_with_tracking_verbose(&plan, verbose)
        .map_err(|e| anyhow::anyhow!("optimization failed: {e}"))?;

    let tracking = result.rule_tracking.ok_or_else(|| {
        anyhow::anyhow!(
            "optimizer returned no rule-tracking data (route may have skipped the e-graph)"
        )
    })?;

    let fired: Vec<FiredRule> = tracking
        .applied
        .iter()
        .map(|a| FiredRule {
            name: a.name.clone(),
            fired_count: a.fired_count,
            nodes_added: a.nodes_added,
            cost_improvement: a.cost_improvement,
        })
        .collect();

    let steps: Vec<StepJson> = tracking
        .intermediate_steps
        .as_ref()
        .map(|steps| {
            steps
                .iter()
                .map(|s| StepJson {
                    step_number: s.step_number,
                    rule_name: s.rule_name.clone(),
                    reason: s.reason.clone(),
                    plan_before: format_plan_tree(&s.plan_before),
                    plan_after: format_plan_tree(&s.plan_after),
                    cost_improvement: s.cost_improvement,
                })
                .collect()
        })
        .unwrap_or_default();

    if format.eq_ignore_ascii_case("json") {
        let report = RulesWhyReport {
            query: query.to_string(),
            fired_rule_count: fired.len(),
            fired_rules: fired,
            steps,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Rules fired ({}):", fired.len());
    if fired.is_empty() {
        println!("  (no rules modified the e-graph for this query)");
    } else {
        for (i, r) in fired.iter().enumerate() {
            print!("  {:>3}. {}  (fired {})", i + 1, r.name, r.fired_count);
            if r.nodes_added > 0 {
                print!(", +{} nodes", r.nodes_added);
            }
            if let Some(delta) = r.cost_improvement {
                print!(", cost -{delta:.2}");
            }
            println!();
        }
    }

    if !steps.is_empty() {
        println!("\nPer-step transformations (verbose):");
        for s in &steps {
            println!("\n  Step {} — {}", s.step_number, s.rule_name);
            if !s.reason.is_empty() {
                println!("    reason: {}", s.reason);
            }
            if let Some(delta) = s.cost_improvement {
                println!("    cost improvement: {delta:.2}");
            }
            println!("    before:");
            for line in s.plan_before.lines() {
                println!("      {line}");
            }
            println!("    after:");
            for line in s.plan_after.lines() {
                println!("      {line}");
            }
        }
    } else if !verbose {
        println!("\n(use --verbose / -v for per-step before/after plans)");
    }

    Ok(())
}
