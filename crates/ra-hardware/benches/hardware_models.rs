//! Benchmarks for hardware model performance estimation.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ra_hardware::{CompleteHardwareProfile, CpuModel, GpuModel, MemoryConfig, StorageDevice};

fn bench_cpu_models(c: &mut Criterion) {
    c.bench_function("cpu_scan_intel_xeon", |b| {
        let cpu = CpuModel::intel_xeon_8380();
        b.iter(|| cpu.scan_time_s(black_box(10_000_000), black_box(100)));
    });

    c.bench_function("cpu_scan_amd_epyc", |b| {
        let cpu = CpuModel::amd_epyc_7763();
        b.iter(|| cpu.scan_time_s(black_box(10_000_000), black_box(100)));
    });

    c.bench_function("cpu_hash_join", |b| {
        let cpu = CpuModel::intel_xeon_8380();
        b.iter(|| cpu.hash_join_time_s(black_box(1_000_000), black_box(10_000_000)));
    });
}

fn bench_gpu_models(c: &mut Criterion) {
    c.bench_function("gpu_scan_a100", |b| {
        let gpu = GpuModel::nvidia_a100_80gb();
        b.iter(|| gpu.scan_time_s(black_box(100_000_000), black_box(100)));
    });

    c.bench_function("gpu_scan_h100", |b| {
        let gpu = GpuModel::nvidia_h100_80gb();
        b.iter(|| gpu.scan_time_s(black_box(100_000_000), black_box(100)));
    });

    c.bench_function("gpu_hash_join", |b| {
        let gpu = GpuModel::nvidia_a100_80gb();
        b.iter(|| gpu.hash_join_time_s(black_box(1_000_000), black_box(100_000_000)));
    });

    c.bench_function("gpu_transfer_time", |b| {
        let gpu = GpuModel::nvidia_a100_80gb();
        b.iter(|| gpu.transfer.transfer_time_s(black_box(10_000_000_000)));
    });
}

fn bench_memory_models(c: &mut Criterion) {
    c.bench_function("memory_sequential_access_ddr4", |b| {
        let mem = MemoryConfig::ddr4_dual_socket();
        b.iter(|| mem.sequential_access_time_s(black_box(10_000_000_000)));
    });

    c.bench_function("memory_sequential_access_hbm3", |b| {
        let mem = MemoryConfig::hbm3();
        b.iter(|| mem.sequential_access_time_s(black_box(10_000_000_000)));
    });

    c.bench_function("memory_random_access", |b| {
        let mem = MemoryConfig::ddr4_dual_socket();
        b.iter(|| mem.random_access_time_s(black_box(100_000)));
    });
}

fn bench_storage_models(c: &mut Criterion) {
    c.bench_function("storage_sequential_read_nvme", |b| {
        let storage = StorageDevice::nvme_gen4_samsung_990_pro();
        b.iter(|| storage.sequential_read_time_s(black_box(10_000_000_000)));
    });

    c.bench_function("storage_sequential_read_ssd", |b| {
        let storage = StorageDevice::sata_ssd_consumer();
        b.iter(|| storage.sequential_read_time_s(black_box(10_000_000_000)));
    });

    c.bench_function("storage_sequential_read_hdd", |b| {
        let storage = StorageDevice::hdd_7200rpm_enterprise();
        b.iter(|| storage.sequential_read_time_s(black_box(10_000_000_000)));
    });

    c.bench_function("storage_random_read_nvme", |b| {
        let storage = StorageDevice::nvme_gen4_samsung_990_pro();
        b.iter(|| storage.random_read_time_s(black_box(10_000)));
    });
}

fn bench_profiles(c: &mut Criterion) {
    c.bench_function("profile_desktop_workstation", |b| {
        b.iter(|| black_box(CompleteHardwareProfile::desktop_workstation()));
    });

    c.bench_function("profile_gpu_server_a100", |b| {
        b.iter(|| black_box(CompleteHardwareProfile::gpu_server_a100()));
    });

    c.bench_function("profile_all_profiles", |b| {
        b.iter(|| black_box(CompleteHardwareProfile::all_profiles()));
    });
}

criterion_group!(
    benches,
    bench_cpu_models,
    bench_gpu_models,
    bench_memory_models,
    bench_storage_models,
    bench_profiles
);
criterion_main!(benches);
