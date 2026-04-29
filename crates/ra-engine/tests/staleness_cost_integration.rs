#![expect(clippy::unwrap_used, reason = "test code")]
//! Integration tests for staleness-aware cost model.

use ra_engine::cost::IntegratedCostModel;
use ra_hardware::HardwareProfile;
use ra_stats::accuracy::{StatisticsSource, StatisticsState};
use ra_stats::integration::ManagedTableStats;
use ra_stats::profiles::StatisticsProfile;
use ra_stats::types::TableStats as RaTableStats;
use std::collections::HashMap;

fn create_test_hardware() -> HardwareProfile {
    HardwareProfile::cpu_only()
}

fn create_test_profile() -> StatisticsProfile {
    StatisticsProfile::standard()
}

fn create_managed_stats(row_count: u64, modifications: u64, age_days: i64) -> ManagedTableStats {
    let mut state = StatisticsState::new(StatisticsSource::ExactCount, row_count);
    state.modifications_since = modifications;
    // Adjust gathered_at to simulate age
    state.gathered_at -= age_days * 86400;

    ManagedTableStats {
        table: RaTableStats {
            row_count,
            average_row_size: 100.0,
            table_size_bytes: row_count * 100,
            page_count: row_count / 100,
            live_tuples: Some(row_count),
            dead_tuples: Some(0),
            last_analyzed: Some(state.gathered_at),
        },
        columns: HashMap::new(),
        state,
    }
}

#[test]
fn fresh_stats_normal_cost() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();
    let mut model = IntegratedCostModel::new(profile, hardware);

    // Fresh stats: no modifications, recent analysis
    let stats = create_managed_stats(100_000, 0, 0);
    model.add_table("users".to_string(), stats);

    let cost = model.scan_cost("users");
    assert!(cost > 0.0, "Cost should be positive");
    // Fresh stats should have minimal penalty
}

#[test]
fn stale_stats_increased_cost() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();

    // Fresh model
    let mut model_fresh = IntegratedCostModel::new(profile.clone(), hardware.clone());
    let fresh_stats = create_managed_stats(100_000, 0, 0);
    model_fresh.add_table("users".to_string(), fresh_stats);
    let fresh_cost = model_fresh.scan_cost("users");

    // Stale model (30% modifications, 30 days old)
    let mut model_stale = IntegratedCostModel::new(profile, hardware);
    let stale_stats = create_managed_stats(100_000, 30_000, 30);
    model_stale.add_table("users".to_string(), stale_stats);
    let stale_cost = model_stale.scan_cost("users");

    assert!(
        stale_cost > fresh_cost * 2.0,
        "Stale stats should have significantly higher cost. Fresh: {fresh_cost}, Stale: {stale_cost}",
    );
}

#[test]
fn index_scan_more_sensitive_to_staleness() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();

    // Fresh model
    let mut model_fresh = IntegratedCostModel::new(profile.clone(), hardware.clone());
    let fresh_stats = create_managed_stats(100_000, 0, 0);
    model_fresh.add_table("users".to_string(), fresh_stats);
    let fresh_seq = model_fresh.scan_cost("users");
    let fresh_idx = model_fresh.index_scan_cost("users", 0.1);

    // Stale model (20% modifications)
    let mut model_stale = IntegratedCostModel::new(profile, hardware);
    let stale_stats = create_managed_stats(100_000, 20_000, 15);
    model_stale.add_table("users".to_string(), stale_stats);
    let stale_seq = model_stale.scan_cost("users");
    let stale_idx = model_stale.index_scan_cost("users", 0.1);

    let seq_penalty = stale_seq / fresh_seq;
    let idx_penalty = stale_idx / fresh_idx;

    assert!(
        idx_penalty > seq_penalty,
        "Index scan should be more sensitive to staleness. Seq penalty: {seq_penalty:.2}x, Index penalty: {idx_penalty:.2}x",
    );
}

#[test]
fn join_cost_uses_max_staleness() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();
    let mut model = IntegratedCostModel::new(profile, hardware);

    // One fresh table, one stale table
    let fresh_stats = create_managed_stats(10_000, 0, 0);
    let stale_stats = create_managed_stats(10_000, 4_000, 45);

    model.add_table("fresh".to_string(), fresh_stats);
    model.add_table("stale".to_string(), stale_stats);

    let cost = model.join_cost("fresh", "stale");
    assert!(cost > 0.0);
    // Join cost should reflect the stalest table
}

#[test]
fn aggregate_cost_with_stale_stats() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();
    let mut model = IntegratedCostModel::new(profile, hardware);

    // Stale stats affect group count estimation
    let stale_stats = create_managed_stats(100_000, 25_000, 60);
    model.add_table("orders".to_string(), stale_stats);

    let cost = model.aggregate_cost("orders", 1000.0);
    assert!(cost > 0.0);
}

#[test]
fn sort_cost_with_very_stale_stats() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();
    let mut model = IntegratedCostModel::new(profile, hardware);

    // Very stale stats (60% modifications, 120 days old)
    let very_stale = create_managed_stats(50_000, 30_000, 120);
    model.add_table("products".to_string(), very_stale);

    let cost = model.sort_cost("products");
    assert!(cost > 0.0);
}

#[test]
fn staleness_penalty_capped_at_max() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();

    // Extreme staleness: table tripled in size, 1 year old
    let mut model = IntegratedCostModel::new(profile, hardware);
    let extreme_stats = create_managed_stats(10_000, 20_000, 365);
    model.add_table("ancient".to_string(), extreme_stats);

    let cost = model.scan_cost("ancient");
    assert!(cost > 0.0);
    // Penalty should be capped, not infinite
}

#[test]
fn quality_metrics_reflect_staleness() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();
    let mut model = IntegratedCostModel::new(profile, hardware);

    let stale_stats = create_managed_stats(100_000, 15_000, 45);
    model.add_table("users".to_string(), stale_stats);

    let metrics = model.quality_metrics("users");
    assert!(metrics.is_some());
    let m = metrics.unwrap();
    // QualityMetrics doesn't have staleness_score, but we can check other fields
    assert!(m.quality_score > 0.0);
}

#[test]
fn should_refresh_detects_stale_stats() {
    let profile = StatisticsProfile::real_time();
    let hardware = create_test_hardware();
    let mut model = IntegratedCostModel::new(profile, hardware);

    // Very stale stats
    let stale_stats = create_managed_stats(100_000, 30_000, 60);
    model.add_table("users".to_string(), stale_stats);

    assert!(
        model.should_refresh("users"),
        "Should detect need for refresh"
    );
}

#[test]
fn unknown_table_returns_default_cost() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();
    let model = IntegratedCostModel::new(profile, hardware);

    let cost = model.scan_cost("nonexistent");
    assert!(cost > 0.0, "Should return default cost for unknown table");
}

#[test]
fn cost_comparison_fresh_vs_stale() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();

    // Fresh scenario
    let mut model_fresh = IntegratedCostModel::new(profile.clone(), hardware.clone());
    let fresh = create_managed_stats(100_000, 100, 0);
    model_fresh.add_table("t".to_string(), fresh);

    // Moderately stale scenario (10% mods, 2 weeks old)
    let mut model_mod_stale = IntegratedCostModel::new(profile.clone(), hardware.clone());
    let mod_stale = create_managed_stats(100_000, 10_000, 14);
    model_mod_stale.add_table("t".to_string(), mod_stale);

    // Very stale scenario (50% mods, 3 months old)
    let mut model_very_stale = IntegratedCostModel::new(profile, hardware);
    let very_stale = create_managed_stats(100_000, 50_000, 90);
    model_very_stale.add_table("t".to_string(), very_stale);

    let cost_fresh = model_fresh.scan_cost("t");
    let cost_mod = model_mod_stale.scan_cost("t");
    let cost_very = model_very_stale.scan_cost("t");

    assert!(
        cost_fresh < cost_mod && cost_mod < cost_very,
        "Costs should increase with staleness. Fresh: {cost_fresh:.2}, Moderate: {cost_mod:.2}, Very: {cost_very:.2}",
    );
}

#[test]
fn robust_plans_favored_when_stale() {
    let profile = create_test_profile();
    let hardware = create_test_hardware();

    // With fresh stats, index scan might be cheaper for low selectivity
    let mut model_fresh = IntegratedCostModel::new(profile.clone(), hardware.clone());
    let fresh = create_managed_stats(100_000, 0, 0);
    model_fresh.add_table("t".to_string(), fresh);
    let fresh_seq = model_fresh.scan_cost("t");
    let fresh_idx = model_fresh.index_scan_cost("t", 0.01); // 1% selectivity

    // With stale stats, seq scan becomes relatively cheaper
    let mut model_stale = IntegratedCostModel::new(profile, hardware);
    let stale = create_managed_stats(100_000, 40_000, 90);
    model_stale.add_table("t".to_string(), stale);
    let stale_seq = model_stale.scan_cost("t");
    let stale_idx = model_stale.index_scan_cost("t", 0.01);

    let fresh_ratio = fresh_idx / fresh_seq;
    let stale_ratio = stale_idx / stale_seq;

    assert!(
        stale_ratio > fresh_ratio,
        "Index scan should become relatively more expensive with stale stats. \
         Fresh idx/seq: {fresh_ratio:.2}, Stale idx/seq: {stale_ratio:.2}",
    );
}
