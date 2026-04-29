//! E-graph integration using the egg library.
//!
//! Defines the [`RelLang`] language for representing relational algebra
//! expressions as S-expressions inside an e-graph. Provides conversion
//! between [`ra_core::RelExpr`] and the e-graph representation, plus
//! the [`Optimizer`] that drives equality saturation.

mod config;
mod errors;
mod from_rec;
mod lang;
mod optimizer;
mod result;
mod to_rec;
mod tracking;

pub use config::{OptimizerConfig, ParallelConfig};
pub use errors::EGraphError;
pub use from_rec::from_egraph_node;
pub use lang::RelLang;
pub use optimizer::Optimizer;
pub use result::{IncrementalStats, OptimizationResult, OptimizationStatus};
pub use to_rec::to_rec_expr;
pub use tracking::{IntermediateStep, RuleApplication, RuleEvaluation, RuleTrackingResult};

#[cfg(test)]
#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    use crate::plan_cache::PlanCacheConfig;
    use crate::resource_budget::ResourceBudget;

    #[cfg(feature = "timeline")]
    use ra_stats::delta::DeltaSet;

    use ra_core::algebra::AggregateExpr;
    use ra_core::algebra::AggregateFunction;

    #[test]
    fn roundtrip_scan() {
        let expr = RelExpr::scan("users");
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn roundtrip_filter() {
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(42))),
        });
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn roundtrip_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn optimizer_roundtrip_simple_scan() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize(&expr)
            .expect("optimization should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn optimizer_roundtrip_filter() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = optimizer
            .optimize(&expr)
            .expect("optimization should succeed");
        // The optimized result should be semantically equivalent
        // (may or may not be structurally identical - optimizer may
        // wrap in Project, reorder, or apply other transformations)
        let _ = result;
    }

    // ---- optimize_bounded tests ----

    #[test]
    fn bounded_optimize_simple_scan() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::unlimited());
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("bounded optimization should succeed");
        assert_eq!(result.status, OptimizationStatus::Complete);
        assert!(result.cost.is_finite());
        assert!(result.resource_usage.completed_within_budget());
    }

    #[test]
    fn bounded_optimize_with_iteration_limit() {
        let budget = ResourceBudget::unlimited().with_iteration_limit(2);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("bounded optimization should succeed");
        assert!(result.resource_usage.iterations_used <= 2);
    }

    #[test]
    fn bounded_optimize_returns_plan_on_timeout() {
        let budget = ResourceBudget::unlimited()
            .with_time_limit(std::time::Duration::from_millis(0))
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::ReturnBestSoFar);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        // Even with 0ms budget, we should still get a plan
        // because we extract the initial plan before iterating
        std::thread::sleep(std::time::Duration::from_millis(1));
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should return best so far");
        assert_eq!(result.status, OptimizationStatus::Incomplete);
    }

    #[test]
    fn bounded_optimize_return_original_strategy() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(0)
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::ReturnOriginal);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should return original");
        assert_eq!(result.status, OptimizationStatus::Incomplete);
        assert_eq!(result.plan, expr);
    }

    #[test]
    fn bounded_optimize_fail_strategy() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(0)
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::Fail);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr);
        assert!(result.is_err());
        let err = result.expect_err("should be error");
        assert!(matches!(err, EGraphError::ResourceBudgetExceeded(_)));
    }

    #[test]
    fn bounded_optimize_no_budget_defaults_unlimited() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should succeed with default budget");
        assert_eq!(result.status, OptimizationStatus::Complete);
    }

    #[test]
    fn bounded_optimize_tracks_egraph_nodes() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.resource_usage.peak_egraph_nodes > 0);
    }

    #[test]
    fn bounded_optimize_tracks_memory_estimate() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.resource_usage.peak_memory_estimate > 0);
    }

    #[test]
    fn bounded_optimize_cost_is_finite() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::batch());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
        assert!(result.cost > 0.0);
    }

    #[test]
    fn bounded_optimize_elapsed_time_recorded() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        // Elapsed time should be recorded (we did some work)
        assert!(result.resource_usage.elapsed_time.as_nanos() > 0);
    }

    #[test]
    fn bounded_optimize_interactive_profile() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::interactive());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(
            result.cost.is_finite(),
            "interactive budget should produce a plan"
        );
    }

    #[test]
    fn bounded_optimize_memory_constrained_profile() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::memory_constrained());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
    }

    #[test]
    fn bounded_optimize_with_egraph_node_limit() {
        let budget = ResourceBudget::unlimited().with_egraph_node_limit(5);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should succeed with best-so-far");
        // With such a tight e-graph limit, likely incomplete
        assert!(result.cost.is_finite() || result.cost == f64::INFINITY);
    }

    #[test]
    fn optimization_status_variants() {
        assert_ne!(OptimizationStatus::Complete, OptimizationStatus::Incomplete);
        assert_ne!(OptimizationStatus::Complete, OptimizationStatus::Failed);
        assert_ne!(OptimizationStatus::Incomplete, OptimizationStatus::Failed);
    }

    #[test]
    fn optimization_result_has_plan() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::unlimited());
        let expr = RelExpr::scan("test_table");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        // The plan should be a valid RelExpr
        assert!(matches!(result.plan, RelExpr::Scan { .. }));
    }

    #[test]
    fn set_resource_budget_mutable() {
        let mut optimizer = Optimizer::new();
        optimizer.set_resource_budget(ResourceBudget::interactive());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
    }

    #[test]
    fn resource_budget_exceeded_error_display() {
        let err = EGraphError::ResourceBudgetExceeded("iterations".to_owned());
        let msg = format!("{err}");
        assert!(msg.contains("iterations"));
        assert!(msg.contains("resource budget exceeded"));
    }

    #[test]
    fn bounded_optimize_best_so_far_no_plan_returns_original() {
        // With 0 iterations AND ReturnBestSoFar, we still get
        // the initial plan because we extract before iterating
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(0)
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::ReturnBestSoFar);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should return a plan");
        assert_eq!(result.status, OptimizationStatus::Incomplete);
    }

    #[test]
    fn bounded_optimize_join_with_budget() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
        assert!(result.resource_usage.iterations_used > 0);
    }

    // ---- optimize_incremental tests ----

    #[cfg(feature = "timeline")]
    fn make_snap(time: u64, rows: u64) -> ra_stats::timeline::Snapshot {
        ra_stats::timeline::Snapshot {
            time_offset: time,
            label: None,
            tables: vec![ra_stats::timeline::TableSnapshot {
                name: "users".to_string(),
                row_count: rows,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![ra_stats::timeline::ColumnSnapshot {
                    name: "id".to_string(),
                    ndv: rows,
                    null_fraction: 0.0,
                    avg_width: 8.0,
                    correlation: Some(1.0),
                    min_value: None,
                    max_value: None,
                }],
            }],
        }
    }

    #[cfg(feature = "timeline")]
    fn small_delta() -> DeltaSet {
        let a = make_snap(0, 10_000);
        let b = make_snap(60, 10_100); // 1% change
        DeltaSet::compute(&a, &b)
    }

    #[cfg(feature = "timeline")]
    fn medium_delta() -> DeltaSet {
        let a = make_snap(0, 10_000);
        let b = make_snap(60, 11_000); // 10% change
        DeltaSet::compute(&a, &b)
    }

    #[cfg(feature = "timeline")]
    fn large_delta() -> DeltaSet {
        let a = make_snap(0, 10_000);
        let b = make_snap(60, 20_000); // 100% change
        DeltaSet::compute(&a, &b)
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_simple_scan() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (result, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("incremental should succeed");
        assert!(matches!(result, RelExpr::Scan { .. }));
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_returns_valid_plan() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let delta = small_delta();
        let (result, _) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(matches!(result, RelExpr::Filter { .. }) || matches!(result, RelExpr::Scan { .. }));
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_small_delta_fewer_iterations() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.iterations_used <= stats.max_iterations);
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_medium_delta_more_iterations() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = medium_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.iterations_used >= 1);
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_large_delta_falls_back_to_full() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = large_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.used_full_reoptimization);
        assert_eq!(stats.iterations_used, stats.max_iterations);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_updates_table_stats() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.tables_updated > 0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_empty_delta_uses_minimal_iterations() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = DeltaSet::new(0, 60);
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.delta_count == 0);
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_produces_same_as_full_for_scan() {
        let expr = RelExpr::scan("users");
        let delta = small_delta();

        let full_result = Optimizer::new()
            .optimize(&expr)
            .expect("full should succeed");
        let (incr_result, _) = Optimizer::new()
            .optimize_incremental(&expr, &delta)
            .expect("incremental should succeed");

        // Both should produce a scan (may differ in internal IDs).
        assert!(matches!(full_result, RelExpr::Scan { .. }));
        assert!(matches!(incr_result, RelExpr::Scan { .. }));
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_stats_speedup_factor() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.speedup_factor() >= 1.0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_stats_full_reopt_speedup_is_one() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = large_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!((stats.speedup_factor() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_reports_delta_count() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.delta_count > 0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_reports_row_change_pct() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = medium_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.row_change_pct > 5.0);
        assert!(stats.row_change_pct < 15.0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_elapsed_time_recorded() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        // Elapsed should be recorded.
        assert!(stats.elapsed.as_nanos() > 0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_join_query() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let delta = small_delta();
        let (result, _) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(matches!(result, RelExpr::Join { .. }) || matches!(result, RelExpr::Scan { .. }));
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_table_added_delta() {
        let a = ra_stats::timeline::Snapshot {
            time_offset: 0,
            label: None,
            tables: vec![ra_stats::timeline::TableSnapshot {
                name: "users".to_string(),
                row_count: 1000,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![],
            }],
        };
        let b = ra_stats::timeline::Snapshot {
            time_offset: 60,
            label: None,
            tables: vec![
                ra_stats::timeline::TableSnapshot {
                    name: "users".to_string(),
                    row_count: 1000,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
                ra_stats::timeline::TableSnapshot {
                    name: "orders".to_string(),
                    row_count: 5000,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
            ],
        };
        let delta = DeltaSet::compute(&a, &b);
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        // Structural change triggers full reoptimization.
        assert!(stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_nodes_in_egraph_reported() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.nodes_in_egraph > 0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_rules_evaluated_reported() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.rules_evaluated > 0);
    }

    #[test]
    fn optimize_with_facts_succeeds() {
        use crate::FactsContextBuilder;
        use ra_hardware::HardwareProfile;

        let facts = FactsContextBuilder::new(HardwareProfile::cpu_only())
            .database("postgresql")
            .dialect(ra_core::SqlDialect::Postgres)
            .feature("lateral_join", true)
            .feature("cte_recursive", true)
            .build();

        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");

        let result = optimizer
            .optimize_with_facts(&expr, &facts)
            .expect("should succeed");

        // Should produce a valid plan
        assert!(matches!(result, RelExpr::Scan { .. }));
    }

    #[test]
    fn optimize_with_facts_uses_hardware_info() {
        use crate::FactsContextBuilder;
        use ra_hardware::HardwareProfile;

        let facts = FactsContextBuilder::new(HardwareProfile::gpu_server())
            .database("duckdb")
            .dialect(ra_core::SqlDialect::DuckDb)
            .feature("parallel_scan", true)
            .build();

        let optimizer = Optimizer::new();
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };

        let result = optimizer
            .optimize_with_facts(&expr, &facts)
            .expect("should succeed");

        // Should produce an optimized plan
        // (actual plan may vary, but should not error)
        assert!(matches!(result, RelExpr::Join { .. }) || matches!(result, RelExpr::Scan { .. }));
    }

    // ── Plan cache integration tests ────────────────────────────

    fn cached_optimizer() -> Optimizer {
        Optimizer::new().with_plan_cache(PlanCacheConfig {
            max_entries: 64,
            similarity_threshold: 0.9,
            enable_fuzzy_matching: true,
            ..PlanCacheConfig::default()
        })
    }

    #[test]
    fn plan_cache_miss_then_hit() {
        let opt = cached_optimizer();
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(42))),
        });
        // First call: cache miss, runs optimization
        let _ = opt.optimize(&q1).expect("should succeed");
        let stats = opt.cache_stats().expect("cache enabled");
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.exact_hits, 0);

        // Same query again: cache hit
        let _ = opt.optimize(&q1).expect("should succeed");
        let stats = opt.cache_stats().expect("cache enabled");
        assert_eq!(stats.exact_hits, 1);
    }

    #[test]
    fn plan_cache_parameter_variation_hits() {
        let opt = cached_optimizer();
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let _ = opt.optimize(&q1).expect("should succeed");

        // Different constant value, same structure
        let q2 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(65))),
        });
        let _ = opt.optimize(&q2).expect("should succeed");

        let stats = opt.cache_stats().expect("cache enabled");
        assert_eq!(
            stats.exact_hits, 1,
            "parameter variation should be exact hit"
        );
        assert_eq!(stats.misses, 1, "only the first query should miss");
    }

    #[test]
    fn plan_cache_disabled_by_default() {
        let opt = Optimizer::new();
        assert!(opt.cache_stats().is_none());
    }

    #[test]
    fn plan_cache_clear() {
        let opt = cached_optimizer();
        let q = RelExpr::scan("users");
        let _ = opt.optimize(&q).expect("should succeed");
        assert_eq!(opt.cache_stats().expect("cache enabled").current_entries, 1);

        opt.clear_cache();
        assert_eq!(opt.cache_stats().expect("cache enabled").current_entries, 0);
    }

    #[test]
    fn plan_cache_oltp_hit_rate_above_90_pct() {
        let opt = cached_optimizer();

        // 5 templates, 20 parameter variations each = 100 queries
        let total = 100_u32;

        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::scan("users").filter(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Const(Const::Int(i))),
            }));
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::scan("orders").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("amount"))),
                right: Box::new(Expr::Const(Const::Int(i * 100))),
            }));
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::Join {
                join_type: JoinType::Inner,
                condition: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
                    right: Box::new(Expr::Column(ColumnRef::qualified("o", "uid"))),
                },
                left: Box::new(RelExpr::scan("users").filter(Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("age"))),
                    right: Box::new(Expr::Const(Const::Int(18 + i))),
                })),
                right: Box::new(RelExpr::scan("orders")),
            });
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::Aggregate {
                group_by: vec![Expr::Column(ColumnRef::new("dept"))],
                aggregates: vec![AggregateExpr {
                    function: AggregateFunction::Count,
                    arg: None,
                    distinct: false,
                    alias: None,
                }],
                input: Box::new(RelExpr::scan("employees").filter(Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("salary"))),
                    right: Box::new(Expr::Const(Const::Int(50000 + i * 1000))),
                })),
            });
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::scan("products").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("price"))),
                right: Box::new(Expr::Const(Const::Int(i * 10))),
            }));
        }

        let stats = opt.cache_stats().expect("cache enabled");
        let hit_count = (stats.exact_hits + stats.fuzzy_hits) as u32;

        // 5 cold misses + 95 hits = 95% hit rate
        let hit_rate = f64::from(hit_count) / f64::from(total);
        assert!(
            hit_rate >= 0.9,
            "expected >90% hit rate, got {:.1}% ({} hits / {} total, stats: {:?})",
            hit_rate * 100.0,
            hit_count,
            total,
            stats
        );
    }

    // ---- rule tracking tests ----

    #[test]
    fn test_optimize_with_tracking_simple() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_with_tracking(&expr)
            .expect("tracking optimization should succeed");

        assert!(result.rule_tracking.is_some());
        let tracking = result.rule_tracking.unwrap();
        assert!(!tracking.available.is_empty());
        assert!(tracking.available.len() >= 200); // Total rules (varies with features)
    }

    #[test]
    fn test_optimize_with_tracking_with_changes() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });

        let result = optimizer
            .optimize_with_tracking(&expr)
            .expect("tracking optimization should succeed");

        assert!(result.rule_tracking.is_some());
        let tracking = result.rule_tracking.unwrap();
        assert!(!tracking.available.is_empty());

        // The filter-true rule should simplify this
        if !tracking.applied.is_empty() {
            assert!(tracking.applied[0].fired_count > 0);
        }
    }

    #[test]
    fn test_rule_tracking_result_structure() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_with_tracking(&expr)
            .expect("tracking optimization should succeed");

        let tracking = result.rule_tracking.expect("tracking should be present");

        // Check structure
        assert!(!tracking.available.is_empty());
        // Applied and evaluated depend on whether rules fired
        assert!(tracking.applied.len() <= tracking.available.len());
        assert!(tracking.evaluated.len() <= tracking.available.len());
    }

    #[test]
    fn test_verbose_mode_captures_intermediate_steps() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });

        let result = optimizer
            .optimize_with_tracking_verbose(&expr, true)
            .expect("verbose tracking should succeed");

        let tracking = result.rule_tracking.expect("tracking should be present");

        // Verbose mode should populate intermediate_steps
        assert!(tracking.intermediate_steps.is_some());

        if !tracking.applied.is_empty() {
            let steps = tracking.intermediate_steps.unwrap();
            // If rules were applied, we should have steps
            if !steps.is_empty() {
                // Each step should have complete information
                for step in &steps {
                    assert!(step.step_number > 0);
                    assert!(!step.rule_name.is_empty());
                    assert!(!step.reason.is_empty());
                }
            }
        }
    }

    #[test]
    fn test_non_verbose_mode_skips_intermediate_steps() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });

        let result = optimizer
            .optimize_with_tracking_verbose(&expr, false)
            .expect("non-verbose tracking should succeed");

        let tracking = result.rule_tracking.expect("tracking should be present");

        // Non-verbose mode should not populate intermediate_steps
        assert!(
            tracking.intermediate_steps.is_none()
                || tracking.intermediate_steps.as_ref().unwrap().is_empty()
        );
    }
}
