//! Integration tests for ra-stats with ra-engine cost models.
//!
//! Validates that statistics staleness, confidence, and profiles
//! correctly influence cost-based plan extraction.

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_engine::cost::{from_core_statistics, IntegratedCostFn, IntegratedCostModel};
use ra_engine::Optimizer;
use ra_hardware::HardwareProfile;
use ra_stats::accuracy::{Staleness, StatisticsSource, StatisticsState};
use ra_stats::integration::ManagedTableStats;
use ra_stats::profiles::{ProfileSelector, StatisticsProfile};
use ra_stats::types::TableStats;

// ── Helpers ──────────────────────────────────────────────────────

fn managed(row_count: u64, avg_row_size: f64, source: StatisticsSource) -> ManagedTableStats {
    ManagedTableStats {
        table: TableStats {
            row_count,
            page_count: row_count / 100 + 1,
            average_row_size: avg_row_size,
            table_size_bytes: row_count * (avg_row_size as u64),
            live_tuples: Some(row_count),
            dead_tuples: Some(0),
            last_analyzed: None,
        },
        columns: HashMap::new(),
        state: StatisticsState::new(source, row_count),
    }
}

fn stale_managed(row_count: u64, modifications: u64) -> ManagedTableStats {
    let mut m = managed(row_count, 100.0, StatisticsSource::ExactCount);
    m.state.record_modifications(modifications);
    m
}

fn cpu_hw() -> HardwareProfile {
    HardwareProfile::cpu_only()
}

fn scan(table: &str) -> RelExpr {
    RelExpr::scan(table)
}

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn join(left: &str, right: &str) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("fk_id")),
        left: Box::new(scan(left)),
        right: Box::new(scan(right)),
    }
}

// ── IntegratedCostModel: Creation & Profiles ─────────────────────

#[test]
fn model_with_standard_profile() {
    let m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    assert_eq!(m.profile().name, "Standard");
    assert_eq!(m.table_count(), 0);
}

#[test]
fn model_with_realtime_profile() {
    let m = IntegratedCostModel::new(StatisticsProfile::real_time(), cpu_hw());
    assert_eq!(m.profile().name, "RealTime");
    assert!(m.profile().min_confidence > 0.9);
}

#[test]
fn model_with_lazy_profile() {
    let m = IntegratedCostModel::new(StatisticsProfile::lazy(), cpu_hw());
    assert_eq!(m.profile().name, "Lazy");
    assert!(!m.profile().multi_column_stats);
}

#[test]
fn model_with_stale_profile() {
    let m = IntegratedCostModel::new(StatisticsProfile::stale(), cpu_hw());
    assert_eq!(m.profile().name, "Stale");
    assert!(m.profile().use_sketches);
}

#[test]
fn model_with_analytical_profile() {
    let m = IntegratedCostModel::new(StatisticsProfile::analytical(), cpu_hw());
    assert_eq!(m.profile().name, "Analytical");
    assert!(m.profile().correlation_stats);
}

#[test]
fn model_with_streaming_profile() {
    let m = IntegratedCostModel::new(StatisticsProfile::streaming(), cpu_hw());
    assert_eq!(m.profile().name, "Streaming");
}

// ── IntegratedCostModel: Table Registration ──────────────────────

#[test]
fn register_single_table() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "users".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    assert_eq!(m.table_count(), 1);
}

#[test]
fn register_multiple_tables() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "users".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    m.add_table(
        "orders".into(),
        managed(100_000, 200.0, StatisticsSource::ExactCount),
    );
    m.add_table(
        "products".into(),
        managed(5_000, 150.0, StatisticsSource::ExactCount),
    );
    assert_eq!(m.table_count(), 3);
}

#[test]
fn register_table_overwrites() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(100, 100.0, StatisticsSource::ExactCount),
    );
    m.add_table(
        "t".into(),
        managed(200, 100.0, StatisticsSource::ExactCount),
    );
    assert_eq!(m.table_count(), 1);
    let stats = m.effective_statistics("t");
    assert!((stats.row_count - 200.0).abs() < f64::EPSILON);
}

// ── Staleness Classification ─────────────────────────────────────

#[test]
fn staleness_fresh_zero_modifications() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(100_000, 100.0, StatisticsSource::ExactCount),
    );
    assert_eq!(m.staleness("t"), Staleness::Fresh);
}

#[test]
fn staleness_slightly_stale() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(100_000, 2_000));
    assert_eq!(m.staleness("t"), Staleness::SlightlyStale);
}

#[test]
fn staleness_moderately_stale() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(100_000, 10_000));
    assert_eq!(m.staleness("t"), Staleness::ModeratelyStale);
}

#[test]
fn staleness_very_stale() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(100_000, 30_000));
    assert_eq!(m.staleness("t"), Staleness::VeryStale);
}

#[test]
fn staleness_unknown_for_missing() {
    let m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    assert_eq!(m.staleness("missing"), Staleness::Unknown);
}

#[test]
fn staleness_boundary_1_percent() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(100_000, 999));
    assert_eq!(m.staleness("t"), Staleness::Fresh);
}

#[test]
fn staleness_boundary_5_percent() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(100_000, 4_999));
    assert_eq!(m.staleness("t"), Staleness::SlightlyStale);
}

#[test]
fn staleness_boundary_20_percent() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(100_000, 19_999));
    assert_eq!(m.staleness("t"), Staleness::ModeratelyStale);
}

// ── Effective Statistics ─────────────────────────────────────────

#[test]
fn effective_stats_returns_row_count() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(50_000, 100.0, StatisticsSource::ExactCount),
    );
    let s = m.effective_statistics("t");
    assert!((s.row_count - 50_000.0).abs() < f64::EPSILON);
}

#[test]
fn effective_stats_default_for_unknown() {
    let m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    let s = m.effective_statistics("missing");
    assert!((s.row_count - 1000.0).abs() < f64::EPSILON);
}

#[test]
fn effective_stats_inflated_when_stale() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(10_000, 5_000));
    let s = m.effective_statistics("t");
    assert!(s.row_count > 10_000.0);
}

// ── Quality Metrics ──────────────────────────────────────────────

#[test]
fn quality_metrics_perfect_for_fresh_exact() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    let qm = m.quality_metrics("t").expect("should exist");
    assert_eq!(qm.quality_score, 1.0);
    assert_eq!(qm.freshness, 1.0);
    assert_eq!(qm.confidence, 1.0);
    assert_eq!(qm.coverage, 1.0);
}

#[test]
fn quality_metrics_lower_for_sampled() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::Sampled { sample_rate: 10 }),
    );
    let qm = m.quality_metrics("t").expect("should exist");
    assert!(qm.quality_score < 1.0);
    assert!(qm.confidence < 1.0);
}

#[test]
fn quality_metrics_lower_for_default() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::Default),
    );
    let qm = m.quality_metrics("t").expect("should exist");
    assert!(qm.quality_score < 0.5);
}

#[test]
fn quality_metrics_none_for_missing_table() {
    let m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    assert!(m.quality_metrics("nonexistent").is_none());
}

#[test]
fn quality_metrics_freshness_degrades_with_modifications() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table("t".into(), stale_managed(10_000, 5_000));
    let qm = m.quality_metrics("t").expect("should exist");
    assert!(qm.freshness < 1.0);
}

// ── Should Refresh ───────────────────────────────────────────────

#[test]
fn should_refresh_false_for_fresh_standard() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(!m.should_refresh("t"));
}

#[test]
fn should_refresh_true_for_missing() {
    let m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    assert!(m.should_refresh("missing"));
}

#[test]
fn should_refresh_realtime_low_threshold() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::real_time(), cpu_hw());
    m.add_table("t".into(), stale_managed(10_000, 2_000));
    assert!(m.should_refresh("t"));
}

#[test]
fn should_refresh_lazy_high_threshold() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::lazy(), cpu_hw());
    m.add_table("t".into(), stale_managed(10_000, 2_000));
    assert!(!m.should_refresh("t"));
}

#[test]
fn should_refresh_stale_profile_tolerant() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::stale(), cpu_hw());
    m.add_table("t".into(), stale_managed(10_000, 5_000));
    assert!(!m.should_refresh("t"));
}

// ── Cost Estimation: Scan ────────────────────────────────────────

#[test]
fn scan_cost_positive_for_known_table() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    let cost = m.scan_cost("t");
    assert!(cost > 0.0);
    assert!(cost.is_finite());
}

#[test]
fn scan_cost_positive_for_unknown_table() {
    let m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    assert!(m.scan_cost("missing") > 0.0);
}

#[test]
fn scan_cost_scales_with_row_count() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "small".into(),
        managed(1_000, 100.0, StatisticsSource::ExactCount),
    );
    m.add_table(
        "large".into(),
        managed(1_000_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.scan_cost("large") > m.scan_cost("small"));
}

#[test]
fn scan_cost_stale_higher_than_fresh() {
    let mut m1 = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m1.add_table(
        "t".into(),
        managed(100_000, 100.0, StatisticsSource::ExactCount),
    );

    let mut m2 = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m2.add_table("t".into(), stale_managed(100_000, 50_000));

    assert!(m2.scan_cost("t") > m1.scan_cost("t"));
}

#[test]
fn scan_cost_low_confidence_higher() {
    let mut m1 = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m1.add_table(
        "t".into(),
        managed(100_000, 100.0, StatisticsSource::ExactCount),
    );

    let mut m2 = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m2.add_table(
        "t".into(),
        managed(100_000, 100.0, StatisticsSource::Default),
    );

    assert!(m2.scan_cost("t") > m1.scan_cost("t"));
}

// ── Cost Estimation: Filter ──────────────────────────────────────

#[test]
fn filter_cost_positive() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.filter_cost("t") > 0.0);
}

#[test]
fn filter_cost_scales_with_rows() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "small".into(),
        managed(100, 100.0, StatisticsSource::ExactCount),
    );
    m.add_table(
        "large".into(),
        managed(1_000_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.filter_cost("large") > m.filter_cost("small"));
}

// ── Cost Estimation: Join ────────────────────────────────────────

#[test]
fn join_cost_positive() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "a".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    m.add_table(
        "b".into(),
        managed(1_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.join_cost("a", "b") > 0.0);
}

#[test]
fn join_cost_stale_higher() {
    let mut m1 = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m1.add_table(
        "a".into(),
        managed(100_000, 100.0, StatisticsSource::ExactCount),
    );
    m1.add_table(
        "b".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );

    let mut m2 = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m2.add_table("a".into(), stale_managed(100_000, 50_000));
    m2.add_table("b".into(), stale_managed(10_000, 5_000));

    assert!(m2.join_cost("a", "b") > m1.join_cost("a", "b"));
}

// ── Cost Estimation: Sort ────────────────────────────────────────

#[test]
fn sort_cost_positive() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.sort_cost("t") > 0.0);
}

#[test]
fn sort_cost_superlinear_scaling() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "small".into(),
        managed(1_000, 100.0, StatisticsSource::ExactCount),
    );
    m.add_table(
        "large".into(),
        managed(1_000_000, 100.0, StatisticsSource::ExactCount),
    );
    let ratio = m.sort_cost("large") / m.sort_cost("small");
    assert!(ratio > 1000.0);
}

// ── Cost Estimation: Aggregate ───────────────────────────────────

#[test]
fn aggregate_cost_positive() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.aggregate_cost("t", 100.0) > 0.0);
}

#[test]
fn aggregate_cost_more_groups_higher() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(100_000, 100.0, StatisticsSource::ExactCount),
    );
    let low = m.aggregate_cost("t", 10.0);
    let high = m.aggregate_cost("t", 10_000.0);
    assert!(high > low);
}

// ── from_core_statistics ─────────────────────────────────────────

#[test]
fn from_core_creates_model() {
    let mut stats = HashMap::new();
    stats.insert("users".into(), Statistics::new(50_000.0));
    stats.insert("orders".into(), Statistics::new(500_000.0));

    let model = from_core_statistics(&stats, &cpu_hw(), StatisticsProfile::standard());
    assert_eq!(model.table_count(), 2);
}

#[test]
fn from_core_preserves_row_counts() {
    let mut stats = HashMap::new();
    let mut s = Statistics::new(12_345.0);
    s.avg_row_size = 200;
    s.total_size = 12_345 * 200;
    stats.insert("t".into(), s);

    let model = from_core_statistics(&stats, &cpu_hw(), StatisticsProfile::standard());
    let es = model.effective_statistics("t");
    assert!((es.row_count - 12_345.0).abs() < f64::EPSILON);
}

#[test]
fn from_core_empty_stats() {
    let stats = HashMap::new();
    let model = from_core_statistics(&stats, &cpu_hw(), StatisticsProfile::standard());
    assert_eq!(model.table_count(), 0);
}

// ── IntegratedCostFn ─────────────────────────────────────────────

#[test]
fn cost_fn_from_model() {
    let mut model = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    model.add_table(
        "t".into(),
        managed(5_000, 100.0, StatisticsSource::ExactCount),
    );

    let _cfn = IntegratedCostFn::from_model(&model, &["t".to_string()]);
}

#[test]
fn cost_fn_with_staleness() {
    let mut stats = HashMap::new();
    stats.insert("t".into(), Statistics::new(10_000.0));
    let mut smap = HashMap::new();
    smap.insert("t".into(), Staleness::VeryStale);

    let _cfn = IntegratedCostFn::new(cpu_hw(), stats, smap);
}

// ── ProfileSelector Integration ──────────────────────────────────

#[test]
fn profile_selector_oltp() {
    let sel = ProfileSelector {
        writes_per_second: 1000.0,
        reads_per_second: 500.0,
        table_size: 1_000_000,
        latency_sensitivity: 0.9,
    };
    let profile = sel.recommend();
    assert_eq!(profile.name, "RealTime");

    let m = IntegratedCostModel::new(profile, cpu_hw());
    assert_eq!(m.profile().name, "RealTime");
}

#[test]
fn profile_selector_olap() {
    let sel = ProfileSelector {
        writes_per_second: 10.0,
        reads_per_second: 100.0,
        table_size: 200_000_000,
        latency_sensitivity: 0.3,
    };
    let profile = sel.recommend();
    assert_eq!(profile.name, "Analytical");

    let m = IntegratedCostModel::new(profile, cpu_hw());
    assert!(m.profile().multi_column_stats);
}

#[test]
fn profile_selector_read_mostly() {
    let sel = ProfileSelector {
        writes_per_second: 1.0,
        reads_per_second: 1000.0,
        table_size: 1_000_000,
        latency_sensitivity: 0.5,
    };
    let profile = sel.recommend();
    assert_eq!(profile.name, "Lazy");
}

// ── Optimizer Integration ────────────────────────────────────────

#[test]
fn optimizer_with_statistics() {
    let mut opt = Optimizer::new();
    opt.add_table_stats("users", Statistics::new(50_000.0));
    opt.add_table_stats("orders", Statistics::new(500_000.0));
    opt.set_hardware_profile(cpu_hw());

    let plan = scan("users");
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

#[test]
fn optimizer_join_with_statistics() {
    let mut opt = Optimizer::new();
    opt.add_table_stats("users", Statistics::new(10_000.0));
    opt.add_table_stats("orders", Statistics::new(100_000.0));
    opt.set_hardware_profile(cpu_hw());

    let plan = join("orders", "users");
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Join { .. }));
}

#[test]
fn optimizer_filter_with_statistics() {
    let mut opt = Optimizer::new();
    opt.add_table_stats("users", Statistics::new(50_000.0));
    opt.set_hardware_profile(cpu_hw());

    let plan = RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(col("age")),
            right: Box::new(Expr::Const(Const::Int(18))),
        },
        input: Box::new(scan("users")),
    };
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Filter { .. }) || matches!(result, RelExpr::Scan { .. }));
}

// ── Statistics Sources ───────────────────────────────────────────

#[test]
fn exact_count_source_full_confidence() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    let qm = m.quality_metrics("t").expect("exists");
    assert_eq!(qm.confidence, 1.0);
}

#[test]
fn sampled_source_partial_confidence() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::Sampled { sample_rate: 50 }),
    );
    let qm = m.quality_metrics("t").expect("exists");
    assert!((qm.confidence - 0.5).abs() < f64::EPSILON);
}

#[test]
fn histogram_source_confidence() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::Histogram),
    );
    let qm = m.quality_metrics("t").expect("exists");
    assert!((qm.confidence - 0.8).abs() < f64::EPSILON);
}

#[test]
fn ml_model_source_confidence() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(
            10_000,
            100.0,
            StatisticsSource::MlModel {
                model_name: "nn_v1".into(),
            },
        ),
    );
    let qm = m.quality_metrics("t").expect("exists");
    assert!((qm.confidence - 0.7).abs() < f64::EPSILON);
}

#[test]
fn derived_source_confidence() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::Derived),
    );
    let qm = m.quality_metrics("t").expect("exists");
    assert!((qm.confidence - 0.6).abs() < f64::EPSILON);
}

#[test]
fn default_source_low_confidence() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::Default),
    );
    let qm = m.quality_metrics("t").expect("exists");
    assert!((qm.confidence - 0.3).abs() < f64::EPSILON);
}

// ── Edge Cases ───────────────────────────────────────────────────

#[test]
fn zero_row_table() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "empty".into(),
        managed(0, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.scan_cost("empty") >= 0.0);
    assert!(m.scan_cost("empty").is_finite());
}

#[test]
fn billion_row_table() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "huge".into(),
        managed(1_000_000_000, 100.0, StatisticsSource::ExactCount),
    );
    assert!(m.scan_cost("huge") > 0.0);
    assert!(m.scan_cost("huge").is_finite());
}

#[test]
fn single_row_sort() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "one".into(),
        managed(1, 100.0, StatisticsSource::ExactCount),
    );
    let cost = m.sort_cost("one");
    assert!(cost >= 0.0);
    assert!(cost.is_finite());
}

#[test]
fn scalar_aggregate_zero_groups() {
    let mut m = IntegratedCostModel::new(StatisticsProfile::standard(), cpu_hw());
    m.add_table(
        "t".into(),
        managed(10_000, 100.0, StatisticsSource::ExactCount),
    );
    let cost = m.aggregate_cost("t", 0.0);
    assert!(cost >= 0.0);
}

// ── Cross-Profile Cost Comparison ────────────────────────────────

#[test]
fn cost_consistent_across_profiles() {
    for profile_fn in &[
        StatisticsProfile::real_time,
        StatisticsProfile::standard,
        StatisticsProfile::lazy,
        StatisticsProfile::analytical,
        StatisticsProfile::streaming,
    ] {
        let mut m = IntegratedCostModel::new(profile_fn(), cpu_hw());
        m.add_table(
            "t".into(),
            managed(100_000, 100.0, StatisticsSource::ExactCount),
        );
        let cost = m.scan_cost("t");
        assert!(cost > 0.0, "cost should be positive for all profiles");
        assert!(cost.is_finite(), "cost should be finite for all profiles");
    }
}
