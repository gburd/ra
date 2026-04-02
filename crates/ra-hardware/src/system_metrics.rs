//! Runtime system metrics collection.
//!
//! Provides real-time system metrics (CPU usage, memory, I/O, network)
//! that can influence query optimization decisions.

#[cfg(target_os = "linux")]
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::time::Duration;

/// Disk I/O statistics for a single device.
#[derive(Debug, Clone)]
pub struct DiskStats {
    /// Device name (e.g., "sda", "nvme0n1").
    pub device: String,
    /// Read operations per second.
    pub reads_per_sec: f64,
    /// Write operations per second.
    pub writes_per_sec: f64,
}

/// Network bandwidth statistics for a single interface.
#[derive(Debug, Clone)]
pub struct NetworkStats {
    /// Interface name (e.g., "eth0", "wlan0").
    pub interface: String,
    /// Bytes received per second.
    pub rx_bytes_per_sec: f64,
    /// Bytes transmitted per second.
    pub tx_bytes_per_sec: f64,
}

/// Current system metrics snapshot.
#[derive(Debug, Clone)]
pub struct SystemMetrics {
    /// CPU utilization percentage (0.0-100.0 per core, or average).
    pub cpu_utilization_percent: f64,
    /// System load average (1-minute).
    ///
    /// Load average represents the number of processes in the run queue
    /// (runnable or waiting for CPU time) averaged over time. This is
    /// NOT disk I/O wait specifically, but overall system process queue depth.
    /// A load of 1.0 on a single-core system means the CPU is fully utilized.
    /// On multi-core systems, divide by CPU count to get per-core load.
    pub load_average_1min: f64,
    /// Available memory in bytes.
    pub available_memory_bytes: u64,
    /// Total memory in bytes.
    pub total_memory_bytes: u64,
    /// Memory utilization percentage (0.0-100.0).
    pub memory_utilization_percent: f64,
    /// Disk I/O statistics per device.
    pub disk_io: Vec<DiskStats>,
    /// Network bandwidth statistics per interface.
    pub network_io: Vec<NetworkStats>,
}

impl SystemMetrics {
    /// Collect current system metrics.
    ///
    /// This is a best-effort operation - if metrics cannot be collected,
    /// returns default/zero values.
    #[must_use]
    pub fn collect() -> Self {
        let cpu_utilization_percent = collect_cpu_utilization().unwrap_or(0.0);
        let load_average_1min = collect_load_average().unwrap_or(0.0);
        let (total_memory_bytes, available_memory_bytes) = collect_memory_info();
        let memory_utilization_percent = if total_memory_bytes > 0 {
            ((total_memory_bytes - available_memory_bytes) as f64 / total_memory_bytes as f64)
                * 100.0
        } else {
            0.0
        };
        let disk_io = collect_disk_io();
        let network_io = collect_network_io();

        Self {
            cpu_utilization_percent,
            load_average_1min,
            available_memory_bytes,
            total_memory_bytes,
            memory_utilization_percent,
            disk_io,
            network_io,
        }
    }

    /// Format metrics for display.
    ///
    /// Format: "CPU: X% | Load: X.XX (process queue) | I/O: device ops/s, ... | Net: X KB/s | Mem: X%"
    #[must_use]
    pub fn format(&self) -> String {
        let mut parts = Vec::new();

        parts.push(format!("CPU: {:.1}%", self.cpu_utilization_percent));
        parts.push(format!(
            "Load: {:.2} (process queue)",
            self.load_average_1min
        ));

        if !self.disk_io.is_empty() {
            let disk_str = self
                .disk_io
                .iter()
                .map(|d| {
                    let total_ops = d.reads_per_sec + d.writes_per_sec;
                    format!("{} {:.0} ops/s", d.device, total_ops)
                })
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("I/O: {}", disk_str));
        }

        if !self.network_io.is_empty() {
            let total_rx: f64 = self.network_io.iter().map(|n| n.rx_bytes_per_sec).sum();
            let total_tx: f64 = self.network_io.iter().map(|n| n.tx_bytes_per_sec).sum();
            let total_kb = (total_rx + total_tx) / 1024.0;
            parts.push(format!("Net: {:.1} KB/s", total_kb));
        }

        parts.push(format!("Mem: {:.1}%", self.memory_utilization_percent));

        parts.join(" | ")
    }
}

/// Collect CPU utilization by sampling /proc/stat twice.
///
/// Returns average CPU utilization across all cores as a percentage.
fn collect_cpu_utilization() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let stat1 = read_proc_stat()?;
        std::thread::sleep(Duration::from_millis(100));
        let stat2 = read_proc_stat()?;

        let total_delta = stat2.total() - stat1.total();
        let idle_delta = stat2.idle - stat1.idle;

        if total_delta > 0 {
            let busy = total_delta - idle_delta;
            Some((busy as f64 / total_delta as f64) * 100.0)
        } else {
            None
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Fallback for non-Linux systems
        None
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct CpuStats {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
}

#[cfg(target_os = "linux")]
impl CpuStats {
    fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq
    }
}

#[cfg(target_os = "linux")]
fn read_proc_stat() -> Option<CpuStats> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    let line = content.lines().next()?;

    if !line.starts_with("cpu ") {
        return None;
    }

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 8 {
        return None;
    }

    Some(CpuStats {
        user: parts[1].parse().ok()?,
        nice: parts[2].parse().ok()?,
        system: parts[3].parse().ok()?,
        idle: parts[4].parse().ok()?,
        iowait: parts[5].parse().ok()?,
        irq: parts[6].parse().ok()?,
        softirq: parts[7].parse().ok()?,
    })
}

/// Collect 1-minute load average.
fn collect_load_average() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let content = fs::read_to_string("/proc/loadavg").ok()?;
        let first = content.split_whitespace().next()?;
        first.parse().ok()
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Collect memory information (total and available).
fn collect_memory_info() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = fs::read_to_string("/proc/meminfo") {
            let mut total = 0u64;
            let mut available = 0u64;

            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(val) = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        total = val * 1024; // Convert KB to bytes
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(val) = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        available = val * 1024; // Convert KB to bytes
                    }
                }

                if total > 0 && available > 0 {
                    break;
                }
            }

            return (total, available);
        }
    }

    // Fallback for non-Linux or if reading failed
    (0, 0)
}

/// Raw disk statistics from /proc/diskstats.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct RawDiskStats {
    reads_completed: u64,
    writes_completed: u64,
}

/// Collect disk I/O statistics by sampling /proc/diskstats twice.
///
/// Returns per-device I/O operations per second. Filters out virtual devices
/// (loop, ram, dm-) and partitions, keeping only whole-disk devices.
fn collect_disk_io() -> Vec<DiskStats> {
    #[cfg(target_os = "linux")]
    {
        let stats1 = read_diskstats();
        std::thread::sleep(Duration::from_millis(100));
        let stats2 = read_diskstats();

        if stats1.is_empty() || stats2.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let interval_secs = 0.1;

        for (device, raw2) in &stats2 {
            if let Some(raw1) = stats1.get(device) {
                let reads_delta = raw2.reads_completed.saturating_sub(raw1.reads_completed);
                let writes_delta = raw2.writes_completed.saturating_sub(raw1.writes_completed);

                results.push(DiskStats {
                    device: device.clone(),
                    reads_per_sec: reads_delta as f64 / interval_secs,
                    writes_per_sec: writes_delta as f64 / interval_secs,
                });
            }
        }

        results
    }

    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Parse /proc/diskstats and return statistics for physical devices.
///
/// Format: major minor device reads ... writes ...
/// Field positions: 0=major, 1=minor, 2=device, 3=reads, 7=writes
#[cfg(target_os = "linux")]
fn read_diskstats() -> HashMap<String, RawDiskStats> {
    let mut result = HashMap::new();

    if let Ok(content) = fs::read_to_string("/proc/diskstats") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 14 {
                continue;
            }

            let device = parts[2];

            // Filter out virtual and partition devices
            // Keep: sda, nvme0n1, vda, xvda, hda
            // Skip: sda1, loop0, ram0, dm-0
            if device.starts_with("loop")
                || device.starts_with("ram")
                || device.starts_with("dm-")
            {
                continue;
            }

            // Skip partitions (devices ending in numbers for non-nvme, or nvme0n1p1 style)
            let is_partition = if device.starts_with("nvme") {
                device.contains('p') && device.chars().last().map_or(false, |c| c.is_ascii_digit())
            } else {
                device.chars().last().map_or(false, |c| c.is_ascii_digit())
            };

            if is_partition {
                continue;
            }

            if let (Ok(reads), Ok(writes)) = (parts[3].parse(), parts[7].parse()) {
                result.insert(
                    device.to_string(),
                    RawDiskStats {
                        reads_completed: reads,
                        writes_completed: writes,
                    },
                );
            }
        }
    }

    result
}

/// Raw network statistics from /proc/net/dev.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct RawNetworkStats {
    rx_bytes: u64,
    tx_bytes: u64,
}

/// Collect network I/O statistics by sampling /proc/net/dev twice.
///
/// Returns per-interface bandwidth in bytes per second. Filters out loopback interface.
fn collect_network_io() -> Vec<NetworkStats> {
    #[cfg(target_os = "linux")]
    {
        let stats1 = read_net_dev();
        std::thread::sleep(Duration::from_millis(100));
        let stats2 = read_net_dev();

        if stats1.is_empty() || stats2.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let interval_secs = 0.1;

        for (interface, raw2) in &stats2 {
            if let Some(raw1) = stats1.get(interface) {
                let rx_delta = raw2.rx_bytes.saturating_sub(raw1.rx_bytes);
                let tx_delta = raw2.tx_bytes.saturating_sub(raw1.tx_bytes);

                // Only include interfaces with activity or non-loopback
                if rx_delta > 0 || tx_delta > 0 || interface != "lo" {
                    results.push(NetworkStats {
                        interface: interface.clone(),
                        rx_bytes_per_sec: rx_delta as f64 / interval_secs,
                        tx_bytes_per_sec: tx_delta as f64 / interval_secs,
                    });
                }
            }
        }

        // Filter loopback if there are other interfaces with activity
        if results.len() > 1 {
            results.retain(|s| s.interface != "lo");
        }

        results
    }

    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Parse /proc/net/dev and return statistics for network interfaces.
///
/// Format:
/// Inter-|   Receive                                                |  Transmit
///  face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets ...
///   eth0: 1234567  123    0    0    0    0      0           0       7654321  123    ...
#[cfg(target_os = "linux")]
fn read_net_dev() -> HashMap<String, RawNetworkStats> {
    let mut result = HashMap::new();

    if let Ok(content) = fs::read_to_string("/proc/net/dev") {
        for line in content.lines() {
            // Skip header lines
            if !line.contains(':') {
                continue;
            }

            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() != 2 {
                continue;
            }

            let interface = parts[0].trim();
            let stats: Vec<&str> = parts[1].split_whitespace().collect();

            if stats.len() < 9 {
                continue;
            }

            // Field 0 is rx_bytes, field 8 is tx_bytes
            if let (Ok(rx_bytes), Ok(tx_bytes)) = (stats[0].parse(), stats[8].parse()) {
                result.insert(
                    interface.to_string(),
                    RawNetworkStats { rx_bytes, tx_bytes },
                );
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_metrics_returns_valid_data() {
        let metrics = SystemMetrics::collect();

        // On Linux, we should get some real data
        #[cfg(target_os = "linux")]
        {
            assert!(metrics.total_memory_bytes > 0, "Should detect total memory");
            // CPU and load might be 0 in test environment, so don't assert on them
        }

        // Should not panic or return invalid percentages
        assert!(metrics.cpu_utilization_percent >= 0.0);
        assert!(metrics.memory_utilization_percent >= 0.0);
        assert!(metrics.memory_utilization_percent <= 100.0 || metrics.total_memory_bytes == 0);

        // Disk and network stats should be valid vectors (may be empty)
        for disk in &metrics.disk_io {
            assert!(disk.reads_per_sec >= 0.0);
            assert!(disk.writes_per_sec >= 0.0);
        }

        for net in &metrics.network_io {
            assert!(net.rx_bytes_per_sec >= 0.0);
            assert!(net.tx_bytes_per_sec >= 0.0);
        }
    }

    #[test]
    fn format_metrics_produces_output() {
        let metrics = SystemMetrics::collect();
        let formatted = metrics.format();

        assert!(formatted.contains("CPU"));
        assert!(formatted.contains("Load"));
        assert!(formatted.contains("Mem"));
    }

    #[test]
    fn format_includes_io_when_available() {
        let metrics = SystemMetrics {
            cpu_utilization_percent: 25.0,
            load_average_1min: 1.5,
            available_memory_bytes: 4_000_000_000,
            total_memory_bytes: 8_000_000_000,
            memory_utilization_percent: 50.0,
            disk_io: vec![DiskStats {
                device: "sda".to_string(),
                reads_per_sec: 100.0,
                writes_per_sec: 50.0,
            }],
            network_io: vec![NetworkStats {
                interface: "eth0".to_string(),
                rx_bytes_per_sec: 1024.0 * 100.0,
                tx_bytes_per_sec: 1024.0 * 50.0,
            }],
        };

        let formatted = metrics.format();
        assert!(formatted.contains("I/O: sda 150 ops/s"));
        assert!(formatted.contains("Net:"));
        assert!(formatted.contains("KB/s"));
    }

    #[test]
    fn load_average_comment_clarity() {
        // This test documents that load average is process queue depth,
        // not disk I/O wait specifically
        let metrics = SystemMetrics::collect();
        let _ = metrics.load_average_1min;
        // The field documentation clarifies this is system-wide process queue depth
    }
}
