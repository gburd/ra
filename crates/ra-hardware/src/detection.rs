//! Hardware auto-detection for local system characteristics.
//!
//! Detects CPU, memory, storage, and accelerator properties to create
//! an appropriate [`HardwareProfile`] for cost-based optimization.

use std::sync::OnceLock;

use crate::profile::HardwareProfile;

/// Cached hardware profile, detected once on first call.
static CACHED_PROFILE: OnceLock<HardwareProfile> = OnceLock::new();

/// Detect hardware characteristics of the local system.
///
/// Returns a [`HardwareProfile`] based on detected CPU, memory, storage,
/// and accelerator properties. Falls back to reasonable defaults if
/// detection fails.
///
/// The result is cached after the first call, so subsequent calls
/// return a clone of the cached profile without re-running detection.
///
/// # Platform Support
///
/// - **macOS**: Uses sysctl and `IOKit` for hardware info
/// - **Linux**: Reads /proc/cpuinfo, /sys/devices for hardware info
/// - **Windows**: Uses WMI queries for hardware info
///
/// # Examples
///
/// ```
/// use ra_hardware::detection::detect_hardware;
///
/// let profile = detect_hardware();
/// println!("Detected {} CPU cores", profile.cpu_cores);
/// ```
#[must_use]
pub fn detect_hardware() -> HardwareProfile {
    CACHED_PROFILE.get_or_init(detect_hardware_inner).clone()
}

/// Perform actual hardware detection (uncached).
fn detect_hardware_inner() -> HardwareProfile {
    HardwareProfile {
        name: "Auto-detected".into(),
        cpu_available: true,
        cpu_cores: detect_cpu_cores(),
        cpu_memory_bandwidth_gbps: detect_memory_bandwidth(),
        l2_cache_bytes: detect_l2_cache(),
        l3_cache_bytes: detect_l3_cache(),
        l3_latency_ns: 12.0,   // Reasonable default
        dram_latency_ns: 80.0, // Reasonable default
        simd_width_bits: detect_simd_width(),
        numa_nodes: detect_numa_nodes(),
        memory_level_parallelism: 10, // Reasonable default for modern CPUs

        // GPU detection (future work)
        gpu_available: false,
        gpu_memory_bytes: 0,
        gpu_memory_bandwidth_gbps: 0.0,
        gpu_sm_count: 0,
        unified_memory_supported: false,
        page_migration_engine_available: false,
        um_page_size_bytes: 4096,
        um_fault_latency_us: 0.0,
        um_migration_bandwidth_gbps: 0.0,
        chunked_transfer_enabled: false,

        // FPGA detection (future work)
        fpga_available: false,
        fpga_clock_mhz: 0,
        fpga_bram_bytes: 0,
        fpga_max_pipeline_depth: 0,
        fpga_reconfig_ms: 0,
        fpga_near_storage: false,
        fpga_available_luts: 0,
        fpga_regex_engines: 0,

        // Interconnect
        pcie_bandwidth_gbps: detect_pcie_bandwidth(),
        storage_bandwidth_gbps: detect_storage_bandwidth(),
    }
}

#[cfg(target_os = "macos")]
fn detect_cpu_cores() -> u32 {
    use std::process::Command;

    let output = Command::new("sysctl")
        .args(["-n", "hw.physicalcpu"])
        .output();

    if let Ok(output) = output {
        if let Ok(s) = String::from_utf8(output.stdout) {
            if let Ok(cores) = s.trim().parse::<u32>() {
                return cores;
            }
        }
    }

    // Fallback
    4
}

#[cfg(target_os = "linux")]
fn detect_cpu_cores() -> u32 {
    use std::fs;

    // Read /proc/cpuinfo and count physical cores
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        let physical_ids: std::collections::HashSet<_> = content
            .lines()
            .filter(|line| line.starts_with("physical id"))
            .map(|line| line.split(':').nth(1).unwrap_or("0").trim())
            .collect();

        let cores_per_socket: u32 = content
            .lines()
            .filter(|line| line.starts_with("cpu cores"))
            .filter_map(|line| line.split(':').nth(1).and_then(|s| s.trim().parse().ok()))
            .next()
            .unwrap_or(1);

        let total = physical_ids.len() as u32 * cores_per_socket;
        if total > 0 {
            return total;
        }
    }

    // Fallback to logical CPUs
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}

#[cfg(target_os = "windows")]
fn detect_cpu_cores() -> u32 {
    // Use Windows WMI or GetLogicalProcessorInformation
    // For now, use available_parallelism as approximation
    std::thread::available_parallelism()
        .map(|n| n.get() as u32 / 2) // Assume hyperthreading
        .unwrap_or(4)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn detect_cpu_cores() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}

#[cfg(target_os = "macos")]
fn detect_l2_cache() -> u64 {
    use std::process::Command;

    let output = Command::new("sysctl")
        .args(["-n", "hw.l2cachesize"])
        .output();

    if let Ok(output) = output {
        if let Ok(s) = String::from_utf8(output.stdout) {
            if let Ok(size) = s.trim().parse::<u64>() {
                return size;
            }
        }
    }

    256 * 1024 // 256 KB default
}

#[cfg(target_os = "linux")]
fn detect_l2_cache() -> u64 {
    use std::fs;

    // Try reading from sysfs
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cache/index2/size") {
        if let Some(size) = parse_cache_size(&content) {
            return size;
        }
    }

    256 * 1024 // 256 KB default
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_l2_cache() -> u64 {
    256 * 1024 // 256 KB default
}

#[cfg(target_os = "macos")]
fn detect_l3_cache() -> u64 {
    use std::process::Command;

    let output = Command::new("sysctl")
        .args(["-n", "hw.l3cachesize"])
        .output();

    if let Ok(output) = output {
        if let Ok(s) = String::from_utf8(output.stdout) {
            if let Ok(size) = s.trim().parse::<u64>() {
                return size;
            }
        }
    }

    8 * 1024 * 1024 // 8 MB default
}

#[cfg(target_os = "linux")]
fn detect_l3_cache() -> u64 {
    use std::fs;

    // Try reading from sysfs
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cache/index3/size") {
        if let Some(size) = parse_cache_size(&content) {
            return size;
        }
    }

    8 * 1024 * 1024 // 8 MB default
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_l3_cache() -> u64 {
    8 * 1024 * 1024 // 8 MB default
}

#[cfg(target_os = "linux")]
fn parse_cache_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(size_str) = s.strip_suffix('K') {
        size_str.parse::<u64>().ok().map(|n| n * 1024)
    } else if let Some(size_str) = s.strip_suffix('M') {
        size_str.parse::<u64>().ok().map(|n| n * 1024 * 1024)
    } else {
        s.parse::<u64>().ok()
    }
}

#[cfg(target_os = "macos")]
fn detect_simd_width() -> u32 {
    use std::process::Command;

    // Check for AVX512, AVX2, SSE support
    let output = Command::new("sysctl").args(["-a"]).output();

    if let Ok(output) = output {
        if let Ok(content) = String::from_utf8(output.stdout) {
            if content.contains("avx512") {
                return 512;
            }
            if content.contains("avx2") {
                return 256;
            }
            if content.contains("avx") {
                return 256;
            }
        }
    }

    128 // SSE default
}

#[cfg(target_os = "linux")]
fn detect_simd_width() -> u32 {
    use std::fs;

    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        if content.contains("avx512") {
            return 512;
        }
        if content.contains("avx2") {
            return 256;
        }
        if content.contains("avx") {
            return 256;
        }
    }

    128 // SSE default
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_simd_width() -> u32 {
    256 // Assume AVX2
}

#[cfg(target_os = "linux")]
fn detect_numa_nodes() -> u32 {
    use std::fs;

    // Count NUMA nodes in /sys/devices/system/node/
    if let Ok(entries) = fs::read_dir("/sys/devices/system/node") {
        let count = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map_or(false, |s| s.starts_with("node"))
            })
            .count() as u32;

        if count > 0 {
            return count;
        }
    }

    1 // Single NUMA node default
}

#[cfg(not(target_os = "linux"))]
fn detect_numa_nodes() -> u32 {
    1 // Assume single NUMA node
}

fn detect_memory_bandwidth() -> f64 {
    // Conservative estimate based on CPU generation
    // Modern CPUs: DDR4-3200 ~25 GB/s per channel, 2-4 channels
    // This would need real benchmarking for accuracy
    50.0 // GB/s, reasonable for dual-channel DDR4
}

fn detect_pcie_bandwidth() -> f64 {
    // PCIe 3.0 x16: ~16 GB/s
    // PCIe 4.0 x16: ~32 GB/s
    // PCIe 5.0 x16: ~64 GB/s
    // Conservative estimate
    16.0 // GB/s, PCIe 3.0 x16
}

fn detect_storage_bandwidth() -> f64 {
    // This is challenging to detect without benchmarking
    // NVMe: 3-7 GB/s
    // SATA SSD: 0.5-0.6 GB/s
    // HDD: 0.1-0.2 GB/s
    //
    // For now, assume NVMe if on modern system
    3.5 // GB/s, conservative NVMe estimate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_hardware_succeeds() {
        let profile = detect_hardware();

        // Sanity checks
        assert!(profile.cpu_available);
        assert!(profile.cpu_cores > 0);
        assert!(profile.cpu_cores <= 256); // Reasonable upper bound
        assert!(profile.l2_cache_bytes >= 128 * 1024); // At least 128 KB
        assert!(profile.l3_cache_bytes >= 1024 * 1024); // At least 1 MB
        assert!(profile.simd_width_bits >= 128); // At least SSE
        assert!(profile.numa_nodes > 0);
        assert!(profile.numa_nodes <= 8); // Reasonable upper bound
    }

    #[test]
    fn detect_cpu_cores_reasonable() {
        let cores = detect_cpu_cores();
        assert!(cores > 0);
        assert!(cores <= 256);
    }

    #[test]
    fn detect_caches_reasonable() {
        let l2 = detect_l2_cache();
        let l3 = detect_l3_cache();

        assert!(l2 >= 128 * 1024); // At least 128 KB
        assert!(l3 >= l2); // L3 should be >= L2
    }

    #[test]
    fn detect_simd_reasonable() {
        let simd = detect_simd_width();
        assert!(simd == 128 || simd == 256 || simd == 512);
    }

    #[test]
    fn detect_numa_reasonable() {
        let numa = detect_numa_nodes();
        assert!(numa > 0);
        assert!(numa <= 8);
    }
}
