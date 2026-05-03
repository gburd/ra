//! Demo of dynamic facts fuzzer functionality.
//!
//! This example shows how the enhanced fuzzer uses different database
//! scenarios to test optimizer robustness across varying facts and statistics.

// This is a demo binary; printing to stdout is intentional.
#![expect(clippy::print_stdout, reason = "demo example binary")]

use ra_core::algebra::RelExpr;
use ra_grammar_fuzzer::dynamic_facts::{DatabaseScenario, EnhancedPropertyValidator};
use ra_grammar_fuzzer::properties::OptimizerProperty;

fn main() {
    println!("🔧 Dynamic Facts Fuzzer Demo");
    println!("============================\n");

    // Generate a sample query
    let expr = ra_core::algebra::RelExpr::Project {
        columns: vec![ra_core::algebra::ProjectionColumn {
            expr: ra_core::expr::Expr::Column(ra_core::expr::ColumnRef {
                table: Some("users".to_string()),
                column: "*".to_string(),
            }),
            alias: None,
        }],
        input: Box::new(RelExpr::Filter {
            predicate: ra_core::expr::Expr::BinOp {
                op: ra_core::expr::BinOp::Eq,
                left: Box::new(ra_core::expr::Expr::Column(ra_core::expr::ColumnRef {
                    table: Some("users".to_string()),
                    column: "status".to_string(),
                })),
                right: Box::new(ra_core::expr::Expr::Const(
                    ra_core::expr::Const::String("active".to_string()),
                )),
            },
            input: Box::new(RelExpr::Scan {
                table: "users".to_string(),
                alias: None,
            }),
        }),
    };

    println!("📊 Testing Query:");
    println!("SELECT * FROM users WHERE status = 'active'\n");

    // Create enhanced property validator
    let validator = EnhancedPropertyValidator::new(vec![
        OptimizerProperty::RuleSafety,
        OptimizerProperty::PlanValidity,
        OptimizerProperty::Convergence,
        OptimizerProperty::TablePreservation,
    ]);

    println!("🧪 Testing across all database scenarios...\n");

    // Test the query across all scenarios
    let results = validator.validate_across_scenarios(&expr);

    for (scenario, property_results) in results {
        let cores = get_scenario_cores(scenario);
        let memory = get_scenario_memory_gb(scenario);
        let gpu = get_scenario_has_gpu(scenario);
        println!("🏗️  Scenario: {scenario:?}");
        println!("   Hardware: {cores} cores, {memory} GB RAM, GPU: {gpu}");

        let mut all_passed = true;
        for result in &property_results {
            if result.passed {
                println!("   ✅ {} - PASSED", result.property);
            } else {
                println!("   ❌ {} - FAILED: {}", result.property, result.details);
                all_passed = false;
            }
        }

        if all_passed {
            println!("   🎉 All properties passed!\n");
        } else {
            println!("   ⚠️  Some properties failed\n");
        }
    }

    println!("✨ Demo completed successfully!");
    println!("\nThe dynamic facts fuzzer systematically varies:");
    println!("• 🖥️  Hardware capabilities (CPU, memory, GPU)");
    println!("• 📊 Table sizes and statistics");
    println!("• 🔄 Data skew and staleness");
    println!("• ⚡ Feature availability and limits");
    println!("\nThis helps discover scenario-specific optimization bugs!");
}

fn get_scenario_cores(scenario: DatabaseScenario) -> u32 {
    match scenario {
        DatabaseScenario::SmallDev => 4,
        DatabaseScenario::MediumProd
        | DatabaseScenario::StaleStats
        | DatabaseScenario::SkewedData => 8,
        DatabaseScenario::LargeEnterprise => 32,
        DatabaseScenario::DataWarehouse => 64,
        DatabaseScenario::MemoryConstrained => 2,
        DatabaseScenario::HighPerformance => 128,
    }
}

fn get_scenario_memory_gb(scenario: DatabaseScenario) -> u64 {
    match scenario {
        DatabaseScenario::SmallDev => 8,
        DatabaseScenario::MediumProd
        | DatabaseScenario::StaleStats
        | DatabaseScenario::SkewedData => 32,
        DatabaseScenario::LargeEnterprise => 128,
        DatabaseScenario::DataWarehouse => 512,
        DatabaseScenario::MemoryConstrained => 2,
        DatabaseScenario::HighPerformance => 1024,
    }
}

fn get_scenario_has_gpu(scenario: DatabaseScenario) -> bool {
    matches!(
        scenario,
        DatabaseScenario::DataWarehouse | DatabaseScenario::HighPerformance
    )
}
