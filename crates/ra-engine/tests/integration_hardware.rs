//! Integration tests for ra-hardware with ra-engine cost models.
//!
//! Validates that hardware profiles (CPU, GPU, FPGA) correctly
//! influence cost estimates and plan extraction.

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::cost::{CostCalibration, IntegratedCostFn, IntegratedCostModel};
use ra_engine::Optimizer;
use ra_hardware::detection::detect_hardware;
use ra_hardware::HardwareProfile;
use ra_stats::accuracy::{Staleness, StatisticsSource, StatisticsState};
use ra_stats::integration::ManagedTableStats;
use ra_stats::profiles::StatisticsProfile;
use ra_stats::types::TableStats;

// ── Helpers ──────────────────────────────────────────────────────

fn managed(row_count: u64) -> ManagedTableStats {
    ManagedTableStats {
        table: TableStats {
            row_count,
            page_count: row_count / 100 + 1,
            average_row_size: 100.0,
            table_size_bytes: row_count * 100,
            live_tuples: Some(row_count),
            dead_tuples: Some(0),
            last_analyzed: None,
        },
        columns: HashMap::new(),
        state: StatisticsState::new(
            StatisticsSource::ExactCount,
            row_count,
        ),
    }
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

fn model_with_hw(hw: HardwareProfile) -> IntegratedCostModel {
    IntegratedCostModel::new(StatisticsProfile::standard(), hw)
}

fn populated_model(hw: HardwareProfile) -> IntegratedCostModel {
    let mut m = model_with_hw(hw);
    m.add_table("users".into(), managed(50_000));
    m.add_table("orders".into(), managed(500_000));
    m.add_table("products".into(), managed(5_000));
    m.add_table("events".into(), managed(10_000_000));
    m
}

// ── Hardware Profile Construction ────────────────────────────────

#[test]
fn cpu_only_profile() {
    let hw = HardwareProfile::cpu_only();
    assert!(hw.cpu_available);
    assert!(!hw.gpu_available);
    assert!(!hw.fpga_available);
}

#[test]
fn gpu_server_profile() {
    let hw = HardwareProfile::gpu_server();
    assert!(hw.gpu_available);
    assert!(hw.gpu_memory_bytes > 0);
    assert!(hw.gpu_sm_count > 0);
}

#[test]
fn fpga_appliance_profile() {
    let hw = HardwareProfile::fpga_appliance();
    assert!(hw.fpga_available);
    assert!(hw.fpga_bram_bytes > 0);
}

// ── Hardware Detection ───────────────────────────────────────────

#[test]
fn detect_hardware_returns_valid_profile() {
    let hw = detect_hardware();
    assert!(hw.cpu_available);
    assert!(hw.cpu_cores > 0);
    assert!(hw.cpu_cores <= 256);
    assert!(hw.l2_cache_bytes >= 128 * 1024);
    assert!(hw.l3_cache_bytes >= 1024 * 1024);
    assert!(
        hw.simd_width_bits == 128
            || hw.simd_width_bits == 256
            || hw.simd_width_bits == 512
    );
}

#[test]
fn detect_hardware_consistent_on_repeat() {
    let hw1 = detect_hardware();
    let hw2 = detect_hardware();
    assert_eq!(hw1.cpu_cores, hw2.cpu_cores);
    assert_eq!(hw1.simd_width_bits, hw2.simd_width_bits);
    assert_eq!(hw1.l3_cache_bytes, hw2.l3_cache_bytes);
}

#[test]
fn detected_hardware_in_model() {
    let hw = detect_hardware();
    let m = model_with_hw(hw);
    assert!(m.hardware().cpu_cores > 0);
}

// ── Storage Bandwidth Impact on Scan Cost ────────────────────────

#[test]
fn faster_storage_reduces_scan_cost() {
    let mut hw_slow = HardwareProfile::cpu_only();
    hw_slow.storage_bandwidth_gbps = 0.5;
    let mut hw_fast = HardwareProfile::cpu_only();
    hw_fast.storage_bandwidth_gbps = 7.0;

    let m_slow = populated_model(hw_slow);
    let m_fast = populated_model(hw_fast);

    assert!(m_fast.scan_cost("orders") < m_slow.scan_cost("orders"));
}

#[test]
fn nvme_vs_hdd_scan_cost() {
    let mut hw_hdd = HardwareProfile::cpu_only();
    hw_hdd.storage_bandwidth_gbps = 0.15;
    let mut hw_nvme = HardwareProfile::cpu_only();
    hw_nvme.storage_bandwidth_gbps = 3.5;

    let m_hdd = populated_model(hw_hdd);
    let m_nvme = populated_model(hw_nvme);

    let ratio = m_hdd.scan_cost("events") / m_nvme.scan_cost("events");
    assert!(ratio > 10.0);
}

#[test]
fn scan_cost_proportional_to_inverse_bandwidth() {
    let mut hw1 = HardwareProfile::cpu_only();
    hw1.storage_bandwidth_gbps = 2.0;
    let mut hw2 = HardwareProfile::cpu_only();
    hw2.storage_bandwidth_gbps = 4.0;

    let m1 = populated_model(hw1);
    let m2 = populated_model(hw2);

    let ratio = m1.scan_cost("users") / m2.scan_cost("users");
    assert!((ratio - 2.0).abs() < 0.1);
}

// ── SIMD Width Impact on Filter Cost ─────────────────────────────

#[test]
fn wider_simd_reduces_filter_cost() {
    let mut hw_sse = HardwareProfile::cpu_only();
    hw_sse.simd_width_bits = 128;
    let mut hw_avx512 = HardwareProfile::cpu_only();
    hw_avx512.simd_width_bits = 512;

    let m_sse = populated_model(hw_sse);
    let m_avx = populated_model(hw_avx512);

    assert!(m_avx.filter_cost("events") < m_sse.filter_cost("events"));
}

#[test]
fn simd_128_vs_256_filter_ratio() {
    let mut hw128 = HardwareProfile::cpu_only();
    hw128.simd_width_bits = 128;
    let mut hw256 = HardwareProfile::cpu_only();
    hw256.simd_width_bits = 256;

    let m128 = populated_model(hw128);
    let m256 = populated_model(hw256);

    let ratio = m128.filter_cost("users") / m256.filter_cost("users");
    assert!((ratio - 2.0).abs() < 0.1);
}

// ── L3 Cache Impact on Join Cost ─────────────────────────────────

#[test]
fn bigger_cache_reduces_join_cost() {
    let mut hw_small = HardwareProfile::cpu_only();
    hw_small.l3_cache_bytes = 4 * 1024 * 1024;
    let mut hw_big = HardwareProfile::cpu_only();
    hw_big.l3_cache_bytes = 64 * 1024 * 1024;

    let m_small = populated_model(hw_small);
    let m_big = populated_model(hw_big);

    assert!(
        m_big.join_cost("users", "orders")
            < m_small.join_cost("users", "orders")
    );
}

#[test]
fn cache_size_proportional_effect() {
    let mut hw8 = HardwareProfile::cpu_only();
    hw8.l3_cache_bytes = 8 * 1024 * 1024;
    let mut hw32 = HardwareProfile::cpu_only();
    hw32.l3_cache_bytes = 32 * 1024 * 1024;

    let m8 = populated_model(hw8);
    let m32 = populated_model(hw32);

    assert!(
        m32.join_cost("users", "orders")
            < m8.join_cost("users", "orders")
    );
}

// ── CPU Cores Impact on Sort Cost ────────────────────────────────

#[test]
fn more_cores_reduces_sort_cost() {
    let mut hw4 = HardwareProfile::cpu_only();
    hw4.cpu_cores = 4;
    let mut hw64 = HardwareProfile::cpu_only();
    hw64.cpu_cores = 64;

    let m4 = populated_model(hw4);
    let m64 = populated_model(hw64);

    assert!(m64.sort_cost("events") < m4.sort_cost("events"));
}

#[test]
fn sort_cost_has_lower_bound() {
    let mut hw1024 = HardwareProfile::cpu_only();
    hw1024.cpu_cores = 255;

    let m = populated_model(hw1024);
    let cost = m.sort_cost("events");
    assert!(cost > 0.0);
}

// ── L3 Cache Impact on Aggregate Cost ────────────────────────────

#[test]
fn bigger_cache_reduces_aggregate_cost() {
    let mut hw_small = HardwareProfile::cpu_only();
    hw_small.l3_cache_bytes = 4 * 1024 * 1024;
    let mut hw_big = HardwareProfile::cpu_only();
    hw_big.l3_cache_bytes = 128 * 1024 * 1024;

    let m_small = populated_model(hw_small);
    let m_big = populated_model(hw_big);

    assert!(
        m_big.aggregate_cost("events", 1000.0)
            < m_small.aggregate_cost("events", 1000.0)
    );
}

// ── GPU Profile in Cost Model ────────────────────────────────────

#[test]
fn gpu_server_model_has_gpu() {
    let m = model_with_hw(HardwareProfile::gpu_server());
    assert!(m.hardware().gpu_available);
    assert!(m.hardware().gpu_memory_bytes > 0);
}

#[test]
fn gpu_server_scan_cost_positive() {
    let m = populated_model(HardwareProfile::gpu_server());
    assert!(m.scan_cost("events") > 0.0);
}

#[test]
fn gpu_server_join_cost_positive() {
    let m = populated_model(HardwareProfile::gpu_server());
    assert!(m.join_cost("users", "orders") > 0.0);
}

// ── FPGA Profile in Cost Model ───────────────────────────────────

#[test]
fn fpga_model_has_fpga() {
    let m = model_with_hw(HardwareProfile::fpga_appliance());
    assert!(m.hardware().fpga_available);
}

#[test]
fn fpga_scan_cost_positive() {
    let m = populated_model(HardwareProfile::fpga_appliance());
    assert!(m.scan_cost("events") > 0.0);
}

// ── Custom Hardware Profiles ─────────────────────────────────────

#[test]
fn custom_high_bandwidth_profile() {
    let mut hw = HardwareProfile::cpu_only();
    hw.storage_bandwidth_gbps = 28.0;
    hw.cpu_cores = 128;
    hw.simd_width_bits = 512;
    hw.l3_cache_bytes = 512 * 1024 * 1024;

    let m = populated_model(hw);
    assert!(m.scan_cost("events") > 0.0);
    assert!(m.filter_cost("events") > 0.0);
    assert!(m.sort_cost("events") > 0.0);
}

#[test]
fn custom_embedded_profile() {
    let mut hw = HardwareProfile::cpu_only();
    hw.storage_bandwidth_gbps = 0.05;
    hw.cpu_cores = 2;
    hw.simd_width_bits = 128;
    hw.l3_cache_bytes = 1024 * 1024;

    let m = populated_model(hw);
    let cost = m.scan_cost("events");
    assert!(cost > 0.0);
    assert!(cost.is_finite());
}

// ── Optimizer with Hardware Profiles ─────────────────────────────

#[test]
fn optimizer_with_cpu_only() {
    let mut opt = Optimizer::new();
    opt.set_hardware_profile(HardwareProfile::cpu_only());
    opt.add_table_stats("users", Statistics::new(10_000.0));

    let plan = scan("users");
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

#[test]
fn optimizer_with_gpu_server() {
    let mut opt = Optimizer::new();
    opt.set_hardware_profile(HardwareProfile::gpu_server());
    opt.add_table_stats("users", Statistics::new(10_000.0));

    let plan = scan("users");
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

#[test]
fn optimizer_with_fpga_appliance() {
    let mut opt = Optimizer::new();
    opt.set_hardware_profile(HardwareProfile::fpga_appliance());
    opt.add_table_stats("users", Statistics::new(10_000.0));

    let plan = scan("users");
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

#[test]
fn optimizer_with_detected_hardware() {
    let mut opt = Optimizer::new();
    opt.set_hardware_profile(detect_hardware());
    opt.add_table_stats("users", Statistics::new(10_000.0));

    let plan = scan("users");
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

#[test]
fn optimizer_join_with_hardware() {
    let mut opt = Optimizer::new();
    opt.set_hardware_profile(HardwareProfile::cpu_only());
    opt.add_table_stats("users", Statistics::new(10_000.0));
    opt.add_table_stats("orders", Statistics::new(100_000.0));

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("user_id")),
        left: Box::new(scan("users")),
        right: Box::new(scan("orders")),
    };
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Join { .. }));
}

// ── Hardware + Statistics Combined ───────────────────────────────

#[test]
fn stale_stats_with_fast_hardware() {
    let mut hw = HardwareProfile::cpu_only();
    hw.storage_bandwidth_gbps = 14.0;
    hw.simd_width_bits = 512;
    hw.l3_cache_bytes = 256 * 1024 * 1024;

    let mut m = IntegratedCostModel::new(
        StatisticsProfile::standard(),
        hw,
    );
    let mut stale = managed(100_000);
    stale.state.record_modifications(30_000);
    m.add_table("t".into(), stale);

    assert_eq!(m.staleness("t"), Staleness::VeryStale);
    assert!(m.scan_cost("t") > 0.0);
}

#[test]
fn fresh_stats_with_slow_hardware() {
    let mut hw = HardwareProfile::cpu_only();
    hw.storage_bandwidth_gbps = 0.15;
    hw.cpu_cores = 2;
    hw.simd_width_bits = 128;

    let m = populated_model(hw);
    assert_eq!(m.staleness("users"), Staleness::Fresh);
    assert!(m.scan_cost("users") > 0.0);
}

// ── IntegratedCostFn with Hardware ───────────────────────────────

#[test]
fn integrated_cost_fn_cpu_only() {
    let stats = HashMap::new();
    let smap = HashMap::new();
    let _cfn = IntegratedCostFn::new(
        HardwareProfile::cpu_only(),
        stats,
        smap,
    );
}

#[test]
fn integrated_cost_fn_gpu_server() {
    let mut stats = HashMap::new();
    stats.insert("t".into(), Statistics::new(10_000.0));
    let mut smap = HashMap::new();
    smap.insert("t".into(), Staleness::Fresh);
    let _cfn = IntegratedCostFn::new(
        HardwareProfile::gpu_server(),
        stats,
        smap,
    );
}

#[test]
fn integrated_cost_fn_from_model_gpu() {
    let mut model = IntegratedCostModel::new(
        StatisticsProfile::standard(),
        HardwareProfile::gpu_server(),
    );
    model.add_table("t".into(), managed(10_000));

    let _cfn = IntegratedCostFn::from_model(
        &model,
        &["t".to_string()],
    );
}

// ── NUMA Topology ────────────────────────────────────────────────

#[test]
fn single_numa_node() {
    let mut hw = HardwareProfile::cpu_only();
    hw.numa_nodes = 1;
    let m = populated_model(hw);
    assert_eq!(m.hardware().numa_nodes, 1);
    assert!(m.scan_cost("events") > 0.0);
}

#[test]
fn multi_numa_node() {
    let mut hw = HardwareProfile::cpu_only();
    hw.numa_nodes = 4;
    let m = populated_model(hw);
    assert_eq!(m.hardware().numa_nodes, 4);
    assert!(m.scan_cost("events") > 0.0);
}

// ── Memory Level Parallelism ─────────────────────────────────────

#[test]
fn high_mlp_hardware() {
    let mut hw = HardwareProfile::cpu_only();
    hw.memory_level_parallelism = 20;
    let m = populated_model(hw);
    assert_eq!(m.hardware().memory_level_parallelism, 20);
}

#[test]
fn low_mlp_hardware() {
    let mut hw = HardwareProfile::cpu_only();
    hw.memory_level_parallelism = 4;
    let m = populated_model(hw);
    assert_eq!(m.hardware().memory_level_parallelism, 4);
}

// ── HardwareCostModel Integration ────────────────────────────────

#[test]
fn hardware_cost_model_scan_cpu() {
    use ra_hardware::cost::HardwareCostModel;
    use ra_hardware::device::Device;

    let model = HardwareCostModel::new(HardwareProfile::cpu_only());
    let cost = model.scan_cost(1_000_000.0, 100, Device::Cpu);
    assert!(cost.cpu > 0.0);
    assert_eq!(cost.io, 0.0);
}

#[test]
fn hardware_cost_model_hash_join_gpu() {
    use ra_hardware::cost::HardwareCostModel;
    use ra_hardware::device::Device;

    let model = HardwareCostModel::new(HardwareProfile::gpu_server());
    let cost = model.hash_join_cost(
        1_000_000.0,
        10_000_000.0,
        100,
        Device::Gpu,
    );
    assert!(cost.cpu > 0.0);
}

#[test]
fn hardware_cost_model_sort_fpga_unsupported() {
    use ra_hardware::cost::HardwareCostModel;
    use ra_hardware::device::Device;

    let model =
        HardwareCostModel::new(HardwareProfile::fpga_appliance());
    let cost = model.sort_cost(1_000_000.0, 100, Device::Fpga);
    assert!(cost.cpu.is_infinite());
}

// ── Serialization Roundtrip ──────────────────────────────────────

#[test]
fn hardware_profile_serialize_roundtrip() {
    let profile = HardwareProfile::gpu_server();
    let json = serde_json::to_string(&profile)
        .expect("should serialize");
    let deser: HardwareProfile = serde_json::from_str(&json)
        .expect("should deserialize");
    assert_eq!(profile, deser);
}

#[test]
fn statistics_profile_serialize_roundtrip() {
    let profile = StatisticsProfile::standard();
    let json = serde_json::to_string(&profile)
        .expect("should serialize");
    let deser: StatisticsProfile = serde_json::from_str(&json)
        .expect("should deserialize");
    assert_eq!(profile, deser);
}

// ── CostCalibration Integration ──────────────────────────────────

#[test]
fn calibration_cpu_only_profile() {
    let cal = CostCalibration::from_hardware(
        &HardwareProfile::cpu_only(),
    );
    assert!(cal.overall_factor() > 0.0);
    assert!(cal.overall_factor().is_finite());
    assert!(!cal.gpu_available);
    assert!(!cal.fpga_available);
}

#[test]
fn calibration_gpu_server_profile() {
    let cal = CostCalibration::from_hardware(
        &HardwareProfile::gpu_server(),
    );
    assert!(cal.gpu_available);
    assert!(cal.overall_factor() > 0.0);
}

#[test]
fn calibration_fpga_profile() {
    let cal = CostCalibration::from_hardware(
        &HardwareProfile::fpga_appliance(),
    );
    assert!(cal.fpga_available);
}

#[test]
fn calibration_detected_hardware() {
    let cal = CostCalibration::from_hardware(&detect_hardware());
    assert!(cal.scan_factor > 0.0);
    assert!(cal.filter_factor > 0.0);
    assert!(cal.overall_factor() > 0.0);
}

#[test]
fn calibration_consistent_with_cost_model() {
    let hw = HardwareProfile::cpu_only();
    let cal = CostCalibration::from_hardware(&hw);
    let m = populated_model(hw);

    // Faster storage => lower scan_factor, lower scan_cost
    let mut hw_fast = HardwareProfile::cpu_only();
    hw_fast.storage_bandwidth_gbps = 14.0;
    let cal_fast = CostCalibration::from_hardware(&hw_fast);
    let m_fast = populated_model(hw_fast);

    assert!(cal_fast.scan_factor < cal.scan_factor);
    assert!(m_fast.scan_cost("events") < m.scan_cost("events"));
}

// ── Profile Comparison Benchmarks ────────────────────────────────

#[test]
fn compare_cpu_only_vs_gpu_server_scan() {
    let m_cpu = populated_model(HardwareProfile::cpu_only());
    let m_gpu = populated_model(HardwareProfile::gpu_server());

    let cpu_cost = m_cpu.scan_cost("events");
    let gpu_cost = m_gpu.scan_cost("events");

    // Both should produce positive costs
    assert!(cpu_cost > 0.0);
    assert!(gpu_cost > 0.0);
}

#[test]
fn compare_cpu_only_vs_gpu_server_join() {
    let m_cpu = populated_model(HardwareProfile::cpu_only());
    let m_gpu = populated_model(HardwareProfile::gpu_server());

    let cpu_cost = m_cpu.join_cost("users", "orders");
    let gpu_cost = m_gpu.join_cost("users", "orders");

    assert!(cpu_cost > 0.0);
    assert!(gpu_cost > 0.0);
}

#[test]
fn compare_cpu_only_vs_gpu_server_sort() {
    let m_cpu = populated_model(HardwareProfile::cpu_only());
    let m_gpu = populated_model(HardwareProfile::gpu_server());

    let cpu_cost = m_cpu.sort_cost("events");
    let gpu_cost = m_gpu.sort_cost("events");

    assert!(cpu_cost > 0.0);
    assert!(gpu_cost > 0.0);
}

#[test]
fn compare_cpu_only_vs_gpu_server_aggregate() {
    let m_cpu = populated_model(HardwareProfile::cpu_only());
    let m_gpu = populated_model(HardwareProfile::gpu_server());

    assert!(m_cpu.aggregate_cost("events", 1000.0) > 0.0);
    assert!(m_gpu.aggregate_cost("events", 1000.0) > 0.0);
}

#[test]
fn compare_cpu_only_vs_fpga_scan() {
    let m_cpu = populated_model(HardwareProfile::cpu_only());
    let m_fpga = populated_model(HardwareProfile::fpga_appliance());

    assert!(m_cpu.scan_cost("events") > 0.0);
    assert!(m_fpga.scan_cost("events") > 0.0);
}

#[test]
fn compare_gpu_vs_fpga_scan() {
    let m_gpu = populated_model(HardwareProfile::gpu_server());
    let m_fpga = populated_model(HardwareProfile::fpga_appliance());

    assert!(m_gpu.scan_cost("events") > 0.0);
    assert!(m_fpga.scan_cost("events") > 0.0);
}

#[test]
fn compare_all_profiles_scan_cost_finite() {
    for profile in &[
        HardwareProfile::cpu_only(),
        HardwareProfile::gpu_server(),
        HardwareProfile::fpga_appliance(),
    ] {
        let m = populated_model(profile.clone());
        for table in &["users", "orders", "products", "events"] {
            let cost = m.scan_cost(table);
            assert!(cost > 0.0, "scan cost should be positive");
            assert!(cost.is_finite(), "scan cost should be finite");
        }
    }
}

#[test]
fn compare_all_profiles_join_cost_finite() {
    for profile in &[
        HardwareProfile::cpu_only(),
        HardwareProfile::gpu_server(),
        HardwareProfile::fpga_appliance(),
    ] {
        let m = populated_model(profile.clone());
        let cost = m.join_cost("users", "orders");
        assert!(cost > 0.0);
        assert!(cost.is_finite());
    }
}

#[test]
fn compare_all_profiles_sort_cost_finite() {
    for profile in &[
        HardwareProfile::cpu_only(),
        HardwareProfile::gpu_server(),
        HardwareProfile::fpga_appliance(),
    ] {
        let m = populated_model(profile.clone());
        let cost = m.sort_cost("events");
        assert!(cost > 0.0);
        assert!(cost.is_finite());
    }
}

#[test]
fn compare_all_profiles_aggregate_cost_finite() {
    for profile in &[
        HardwareProfile::cpu_only(),
        HardwareProfile::gpu_server(),
        HardwareProfile::fpga_appliance(),
    ] {
        let m = populated_model(profile.clone());
        let cost = m.aggregate_cost("events", 1000.0);
        assert!(cost > 0.0);
        assert!(cost.is_finite());
    }
}

#[test]
fn compare_all_profiles_filter_cost_finite() {
    for profile in &[
        HardwareProfile::cpu_only(),
        HardwareProfile::gpu_server(),
        HardwareProfile::fpga_appliance(),
    ] {
        let m = populated_model(profile.clone());
        let cost = m.filter_cost("events");
        assert!(cost > 0.0);
        assert!(cost.is_finite());
    }
}

// ── Calibration vs Actual Cost Ordering ──────────────────────────

#[test]
fn calibration_ordering_matches_scan_cost() {
    let mut hw_fast = HardwareProfile::cpu_only();
    hw_fast.storage_bandwidth_gbps = 14.0;
    let mut hw_slow = HardwareProfile::cpu_only();
    hw_slow.storage_bandwidth_gbps = 0.5;

    let cal_fast = CostCalibration::from_hardware(&hw_fast);
    let cal_slow = CostCalibration::from_hardware(&hw_slow);

    let m_fast = populated_model(hw_fast);
    let m_slow = populated_model(hw_slow);

    // Calibration factor ordering matches cost ordering
    assert!(cal_fast.scan_factor < cal_slow.scan_factor);
    assert!(m_fast.scan_cost("events") < m_slow.scan_cost("events"));
}

#[test]
fn calibration_ordering_matches_filter_cost() {
    let mut hw_wide = HardwareProfile::cpu_only();
    hw_wide.simd_width_bits = 512;
    let mut hw_narrow = HardwareProfile::cpu_only();
    hw_narrow.simd_width_bits = 128;

    let cal_wide = CostCalibration::from_hardware(&hw_wide);
    let cal_narrow = CostCalibration::from_hardware(&hw_narrow);

    let m_wide = populated_model(hw_wide);
    let m_narrow = populated_model(hw_narrow);

    assert!(cal_wide.filter_factor < cal_narrow.filter_factor);
    assert!(m_wide.filter_cost("events") < m_narrow.filter_cost("events"));
}

#[test]
fn calibration_ordering_matches_join_cost() {
    let mut hw_big = HardwareProfile::cpu_only();
    hw_big.l3_cache_bytes = 128 * 1024 * 1024;
    let mut hw_small = HardwareProfile::cpu_only();
    hw_small.l3_cache_bytes = 4 * 1024 * 1024;

    let cal_big = CostCalibration::from_hardware(&hw_big);
    let cal_small = CostCalibration::from_hardware(&hw_small);

    let m_big = populated_model(hw_big);
    let m_small = populated_model(hw_small);

    assert!(cal_big.join_factor < cal_small.join_factor);
    assert!(
        m_big.join_cost("users", "orders")
            < m_small.join_cost("users", "orders")
    );
}

#[test]
fn calibration_ordering_matches_sort_cost() {
    let mut hw_many = HardwareProfile::cpu_only();
    hw_many.cpu_cores = 64;
    let mut hw_few = HardwareProfile::cpu_only();
    hw_few.cpu_cores = 4;

    let cal_many = CostCalibration::from_hardware(&hw_many);
    let cal_few = CostCalibration::from_hardware(&hw_few);

    let m_many = populated_model(hw_many);
    let m_few = populated_model(hw_few);

    assert!(cal_many.sort_factor < cal_few.sort_factor);
    assert!(m_many.sort_cost("events") < m_few.sort_cost("events"));
}

// ── Optimizer with Calibrated Profiles ───────────────────────────

#[test]
fn optimizer_with_fast_nvme() {
    let mut hw = HardwareProfile::cpu_only();
    hw.storage_bandwidth_gbps = 7.0;
    hw.cpu_cores = 16;
    hw.simd_width_bits = 512;
    hw.l3_cache_bytes = 32 * 1024 * 1024;

    let mut opt = Optimizer::new();
    opt.set_hardware_profile(hw);
    opt.add_table_stats("users", Statistics::new(100_000.0));

    let plan = scan("users").filter(eq(col("active"), col("true")));
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(
        matches!(result, RelExpr::Filter { .. })
            || matches!(result, RelExpr::Scan { .. })
    );
}

#[test]
fn optimizer_with_slow_hdd() {
    let mut hw = HardwareProfile::cpu_only();
    hw.storage_bandwidth_gbps = 0.15;
    hw.cpu_cores = 4;
    hw.simd_width_bits = 128;
    hw.l3_cache_bytes = 8 * 1024 * 1024;

    let mut opt = Optimizer::new();
    opt.set_hardware_profile(hw);
    opt.add_table_stats("users", Statistics::new(100_000.0));

    let plan = scan("users");
    let result = opt.optimize(&plan).expect("should optimize");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

#[test]
fn optimizer_join_different_profiles_both_succeed() {
    for hw in &[
        HardwareProfile::cpu_only(),
        HardwareProfile::gpu_server(),
        HardwareProfile::fpga_appliance(),
    ] {
        let mut opt = Optimizer::new();
        opt.set_hardware_profile(hw.clone());
        opt.add_table_stats("a", Statistics::new(10_000.0));
        opt.add_table_stats("b", Statistics::new(100_000.0));

        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("id"), col("fk")),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let result = opt.optimize(&plan).expect("should optimize");
        assert!(matches!(result, RelExpr::Join { .. }));
    }
}
