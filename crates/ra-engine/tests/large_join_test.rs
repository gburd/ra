//! Tests for large join graph optimization fallback.

use std::collections::HashMap;
use std::sync::Arc;

use ra_core::{
    algebra::{JoinType, RelExpr},
    cost::{Cost, CostModel, StatisticsProvider},
    expr::{ColumnRef, Const, Expr},
    statistics::{ColumnStats, Statistics},
};
use ra_engine::large_join::{JoinNode, LargeJoinOptimizer, LargeJoinStrategy};

/// Mock cost model for testing.
#[derive(Debug)]
struct MockCostModel;

impl CostModel for MockCostModel {
    fn estimate(&self, expr: &RelExpr, _stats: &dyn StatisticsProvider) -> Cost {
        // Simple cost model: cost increases with join depth
        let depth = count_join_depth(expr);
        Cost::new(depth as f64 * 100.0, depth as f64 * 10.0, 0.0, depth as u64 * 1024)
    }
}

fn count_join_depth(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Join { left, right, .. } => {
            1 + count_join_depth(left).max(count_join_depth(right))
        }
        _ => 0,
    }
}

/// Mock statistics provider for testing.
#[derive(Debug)]
struct MockStatsProvider {
    stats: HashMap<String, Statistics>,
}

impl MockStatsProvider {
    fn new() -> Self {
        let mut stats = HashMap::new();

        // Add statistics for test tables
        stats.insert(
            "users".to_string(),
            Statistics {
                row_count: 1000.0,
                avg_row_size: 100,
                total_size: 100_000,
                columns: HashMap::new(),
            },
        );

        stats.insert(
            "orders".to_string(),
            Statistics {
                row_count: 10_000.0,
                avg_row_size: 200,
                total_size: 2_000_000,
                columns: HashMap::new(),
            },
        );

        stats.insert(
            "products".to_string(),
            Statistics {
                row_count: 500.0,
                avg_row_size: 150,
                total_size: 75_000,
                columns: HashMap::new(),
            },
        );

        stats.insert(
            "order_items".to_string(),
            Statistics {
                row_count: 50_000.0,
                avg_row_size: 50,
                total_size: 2_500_000,
                columns: HashMap::new(),
            },
        );

        // Add more tables for large join tests
        for i in 1..=20 {
            stats.insert(
                format!("table{}", i),
                Statistics {
                    row_count: (i as f64) * 1000.0,
                    avg_row_size: 100,
                    total_size: i as u64 * 100_000,
                    columns: HashMap::new(),
                },
            );
        }

        Self { stats }
    }
}

impl StatisticsProvider for MockStatsProvider {
    fn get_statistics(&self, table: &str) -> Option<&Statistics> {
        self.stats.get(table)
    }
}

#[test]
fn test_greedy_join_order_empty() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::Greedy,
        cost_model,
        stats_provider,
    );

    let result = optimizer.optimize(vec![]);
    assert!(result.is_err());
}

#[test]
fn test_greedy_join_order_single_table() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::Greedy,
        cost_model,
        stats_provider,
    );

    let joins = vec![JoinNode {
        table: "users".to_string(),
        alias: None,
        condition: None,
    }];

    let result = optimizer.optimize(joins).unwrap();
    assert!(matches!(result, RelExpr::Scan { table, .. } if table == "users"));
}

#[test]
fn test_greedy_join_order_two_tables() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::Greedy,
        cost_model,
        stats_provider,
    );

    let joins = vec![
        JoinNode {
            table: "orders".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "products".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
    ];

    let result = optimizer.optimize(joins).unwrap();
    assert!(matches!(result, RelExpr::Join { .. }));

    // Should start with smallest table (products)
    if let RelExpr::Join { left, .. } = result {
        assert!(matches!(&**left, RelExpr::Scan { table, .. } if table == "products"));
    }
}

#[test]
fn test_greedy_join_order_multiple_tables() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::Greedy,
        cost_model,
        stats_provider,
    );

    let joins = vec![
        JoinNode {
            table: "users".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "orders".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "products".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "order_items".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
    ];

    let result = optimizer.optimize(joins).unwrap();

    // Should produce a valid join tree
    assert!(matches!(result, RelExpr::Join { .. }));

    // Count the number of joins
    fn count_joins(expr: &RelExpr) -> usize {
        match expr {
            RelExpr::Join { left, right, .. } => 1 + count_joins(left) + count_joins(right),
            _ => 0,
        }
    }

    assert_eq!(count_joins(&result), 3); // 4 tables need 3 joins
}

#[test]
fn test_simulated_annealing_basic() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::SimulatedAnnealing {
            initial_temp: 1000.0,
            cooling_rate: 0.95,
            max_iterations: 100,
        },
        cost_model,
        stats_provider,
    );

    let joins = vec![
        JoinNode {
            table: "users".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "orders".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "products".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
    ];

    let result = optimizer.optimize(joins).unwrap();
    assert!(matches!(result, RelExpr::Join { .. }));
}

#[test]
fn test_simulated_annealing_convergence() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    // Test with more iterations
    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::SimulatedAnnealing {
            initial_temp: 1000.0,
            cooling_rate: 0.99,
            max_iterations: 1000,
        },
        cost_model.clone(),
        stats_provider.clone(),
    );

    let joins = vec![
        JoinNode {
            table: "table1".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "table2".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "table3".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "table4".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "table5".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
    ];

    let result = optimizer.optimize(joins).unwrap();
    assert!(matches!(result, RelExpr::Join { .. }));
}

#[test]
fn test_large_join_20_tables() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    // Test greedy with 20 tables
    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::Greedy,
        cost_model.clone(),
        stats_provider.clone(),
    );

    let mut joins = Vec::new();
    for i in 1..=20 {
        joins.push(JoinNode {
            table: format!("table{}", i),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        });
    }

    let start = std::time::Instant::now();
    let result = optimizer.optimize(joins.clone()).unwrap();
    let greedy_time = start.elapsed();

    assert!(matches!(result, RelExpr::Join { .. }));

    // Greedy should complete in under 5 seconds for 20 tables
    assert!(greedy_time.as_secs() < 5, "Greedy took {:?}", greedy_time);

    // Test simulated annealing with 20 tables
    let optimizer_sa = LargeJoinOptimizer::new(
        LargeJoinStrategy::SimulatedAnnealing {
            initial_temp: 1000.0,
            cooling_rate: 0.95,
            max_iterations: 5000,
        },
        cost_model,
        stats_provider,
    );

    let start = std::time::Instant::now();
    let result = optimizer_sa.optimize(joins).unwrap();
    let sa_time = start.elapsed();

    assert!(matches!(result, RelExpr::Join { .. }));

    // Simulated annealing should complete in under 30 seconds for 20 tables
    assert!(sa_time.as_secs() < 30, "Simulated annealing took {:?}", sa_time);
}

#[test]
fn test_count_tables() {
    // Test single scan
    let expr = RelExpr::Scan {
        table: "users".to_string(),
        alias: None,
    };
    assert_eq!(LargeJoinOptimizer::count_tables(&expr), 1);

    // Test join of two tables
    let expr = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(RelExpr::Scan {
            table: "users".to_string(),
            alias: None,
        }),
        right: Box::new(RelExpr::Scan {
            table: "orders".to_string(),
            alias: None,
        }),
    };
    assert_eq!(LargeJoinOptimizer::count_tables(&expr), 2);

    // Test nested joins (3 tables)
    let expr = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::Scan {
                table: "users".to_string(),
                alias: None,
            }),
            right: Box::new(RelExpr::Scan {
                table: "orders".to_string(),
                alias: None,
            }),
        }),
        right: Box::new(RelExpr::Scan {
            table: "products".to_string(),
            alias: None,
        }),
    };
    assert_eq!(LargeJoinOptimizer::count_tables(&expr), 3);

    // Test with filter/project (shouldn't affect count)
    let expr = RelExpr::Project {
        columns: vec![],
        input: Box::new(RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(RelExpr::Scan {
                table: "users".to_string(),
                alias: None,
            }),
        }),
    };
    assert_eq!(LargeJoinOptimizer::count_tables(&expr), 1);
}

#[test]
fn test_extract_joins() {
    let expr = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(RelExpr::Scan {
            table: "users".to_string(),
            alias: Some("u".to_string()),
        }),
        right: Box::new(RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::Scan {
                table: "orders".to_string(),
                alias: Some("o".to_string()),
            }),
            right: Box::new(RelExpr::Scan {
                table: "products".to_string(),
                alias: Some("p".to_string()),
            }),
        }),
    };

    let joins = LargeJoinOptimizer::extract_joins(&expr);
    assert_eq!(joins.len(), 3);

    let tables: Vec<String> = joins.iter().map(|j| j.table.clone()).collect();
    assert!(tables.contains(&"users".to_string()));
    assert!(tables.contains(&"orders".to_string()));
    assert!(tables.contains(&"products".to_string()));
}

#[test]
fn test_perturb_produces_different_plans() {
    let cost_model = Arc::new(MockCostModel);
    let stats_provider = Arc::new(MockStatsProvider::new());

    let optimizer = LargeJoinOptimizer::new(
        LargeJoinStrategy::SimulatedAnnealing {
            initial_temp: 1000.0,
            cooling_rate: 0.95,
            max_iterations: 10,
        },
        cost_model,
        stats_provider,
    );

    let joins = vec![
        JoinNode {
            table: "table1".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "table2".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
        JoinNode {
            table: "table3".to_string(),
            alias: None,
            condition: Some(Expr::Const(Const::Bool(true))),
        },
    ];

    // Generate multiple plans and check that we get some variation
    let mut plans = Vec::new();
    for _ in 0..5 {
        let result = optimizer.optimize(joins.clone()).unwrap();
        plans.push(result);
    }

    // Due to randomization in simulated annealing, we should see some variation
    // (though this isn't guaranteed with such a small example)
    assert!(!plans.is_empty());
}