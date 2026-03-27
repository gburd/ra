//! Runtime system metrics collection.
//!
//! Provides real-time system metrics (CPU usage, memory, I/O, network)
//! that can influence query optimization decisions.

use std::fs;
use std::time::{Duration, Instant};

/// Current system metrics snapshot.
#[derive(Debug, Clone)]
pub struct SystemMetrics {
    /// CPU utilization percentage (0.0-100.0 per core, or average).
    pub cpu_utilization_percent: f64,
    /// System load average (1-minute).
    pub load_average_1min: f64,
    /// Available memory in bytes.
    pub available_memory_bytes: u64,
    /// Total memory in bytes.
    pub total_memory_bytes: u64,
    /// Memory utilization percentage (0.0-100.0).
    pub memory_utilization_percent: f64,
    /// Disk I/O operations per second (reads + writes).
    pub disk_iops: f64,
    /// Network bandwidth usage in bytes/sec.
    pub network_bandwidth_bps: f64,
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

        Self {
            cpu_utilization_percent,
            load_average_1min,
            available_memory_bytes,
            total_memory_bytes,
            memory_utilization_percent,
            disk_iops: 0.0,          // TODO: Implement disk I/O collection
            network_bandwidth_bps: 0.0, // TODO: Implement network collection
        }
    }

    /// Format metrics for display.
    #[must_use]
    pub fn format(&self) -> String {
        format!(
            "CPU: {:.1}% | Load: {:.2} | Memory: {:.1}% ({} / {} MB)",
            self.cpu_utilization_percent,
            self.load_average_1min,
            self.memory_utilization_percent,
            self.available_memory_bytes / (1024 * 1024),
            self.total_memory_bytes / (1024 * 1024)
        )
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
    }

    #[test]
    fn format_metrics_produces_output() {
        let metrics = SystemMetrics::collect();
        let formatted = metrics.format();

        assert!(formatted.contains("CPU"));
        assert!(formatted.contains("Load"));
        assert!(formatted.contains("Memory"));
    }
}
