//! PostgreSQL configuration recommendations.
//!
//! Analyzes memory settings, query planner parameters, and
//! parallelism configuration against best practices and the
//! actual hardware environment.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::recommendations::Severity;

/// PostgreSQL configuration values collected from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgConfig {
    // Memory settings
    /// `shared_buffers` in bytes.
    pub shared_buffers: u64,
    /// `effective_cache_size` in bytes.
    pub effective_cache_size: u64,
    /// `work_mem` in bytes.
    pub work_mem: u64,
    /// `maintenance_work_mem` in bytes.
    pub maintenance_work_mem: u64,

    // Planner settings
    /// `random_page_cost`.
    pub random_page_cost: f64,
    /// `effective_io_concurrency`.
    pub effective_io_concurrency: u32,
    /// `default_statistics_target`.
    pub default_statistics_target: u32,

    // Parallelism
    /// `max_parallel_workers_per_gather`.
    pub max_parallel_workers_per_gather: u32,
    /// `parallel_tuple_cost`.
    pub parallel_tuple_cost: f64,

    // System info
    /// Total system RAM in bytes.
    pub system_ram: u64,
    /// Number of CPU cores.
    pub cpu_cores: u32,
    /// Whether storage is SSD.
    pub is_ssd: bool,
}

impl Default for PgConfig {
    fn default() -> Self {
        Self {
            shared_buffers: 128 * 1024 * 1024,
            effective_cache_size: 4 * 1024 * 1024 * 1024,
            work_mem: 4 * 1024 * 1024,
            maintenance_work_mem: 64 * 1024 * 1024,
            random_page_cost: 4.0,
            effective_io_concurrency: 1,
            default_statistics_target: 100,
            max_parallel_workers_per_gather: 2,
            parallel_tuple_cost: 0.01,
            system_ram: 16 * 1024 * 1024 * 1024,
            cpu_cores: 8,
            is_ssd: true,
        }
    }
}

/// A configuration issue with recommended fix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigIssue {
    /// Severity of the issue.
    pub severity: Severity,
    /// Configuration parameter name.
    pub parameter: String,
    /// Current value (human-readable).
    pub current_value: String,
    /// Recommended value (human-readable).
    pub recommended_value: String,
    /// Description of the problem.
    pub message: String,
    /// Suggested fix.
    pub suggestion: String,
}

impl fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {} (current: {}, recommended: {})",
            self.severity,
            self.parameter,
            self.message,
            self.current_value,
            self.recommended_value,
        )
    }
}

/// Checks PostgreSQL configuration against best practices.
pub struct ConfigChecker {
    config: Option<PgConfig>,
    issues: Vec<ConfigIssue>,
}

impl ConfigChecker {
    /// Create a new config checker with no config loaded.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: None,
            issues: Vec::new(),
        }
    }

    /// Load a PostgreSQL configuration for analysis.
    pub fn load_config(&mut self, config: PgConfig) {
        self.config = Some(config);
    }

    /// Run all configuration checks.
    pub fn analyze(&mut self) {
        self.issues.clear();
        let config = match &self.config {
            Some(c) => c.clone(),
            None => return,
        };

        self.check_shared_buffers(&config);
        self.check_effective_cache_size(&config);
        self.check_work_mem(&config);
        self.check_maintenance_work_mem(&config);
        self.check_random_page_cost(&config);
        self.check_effective_io_concurrency(&config);
        self.check_statistics_target(&config);
        self.check_parallelism(&config);
    }

    /// Get all detected configuration issues.
    #[must_use]
    pub fn issues(&self) -> &[ConfigIssue] {
        &self.issues
    }

    fn check_shared_buffers(&mut self, config: &PgConfig) {
        let recommended = config.system_ram / 4;
        let lower = recommended / 2;
        let upper = recommended * 2;

        if config.shared_buffers < lower {
            self.issues.push(ConfigIssue {
                severity: Severity::Warning,
                parameter: "shared_buffers".to_string(),
                current_value: format_bytes(
                    config.shared_buffers,
                ),
                recommended_value: format_bytes(recommended),
                message: "shared_buffers is too low for \
                          available RAM"
                    .to_string(),
                suggestion: format!(
                    "ALTER SYSTEM SET shared_buffers = '{}';",
                    format_bytes(recommended),
                ),
            });
        } else if config.shared_buffers > upper {
            self.issues.push(ConfigIssue {
                severity: Severity::Info,
                parameter: "shared_buffers".to_string(),
                current_value: format_bytes(
                    config.shared_buffers,
                ),
                recommended_value: format_bytes(recommended),
                message: "shared_buffers is unusually high; \
                          diminishing returns above 25% of RAM"
                    .to_string(),
                suggestion: format!(
                    "ALTER SYSTEM SET shared_buffers = '{}';",
                    format_bytes(recommended),
                ),
            });
        }
    }

    fn check_effective_cache_size(&mut self, config: &PgConfig) {
        let recommended = (config.system_ram * 3) / 4;
        let lower = recommended / 2;

        if config.effective_cache_size < lower {
            self.issues.push(ConfigIssue {
                severity: Severity::Warning,
                parameter: "effective_cache_size".to_string(),
                current_value: format_bytes(
                    config.effective_cache_size,
                ),
                recommended_value: format_bytes(recommended),
                message: "effective_cache_size is too low; \
                          planner may avoid index scans"
                    .to_string(),
                suggestion: format!(
                    "ALTER SYSTEM SET effective_cache_size \
                     = '{}';",
                    format_bytes(recommended),
                ),
            });
        }
    }

    fn check_work_mem(&mut self, config: &PgConfig) {
        let min_recommended: u64 = 8 * 1024 * 1024;
        if config.work_mem < min_recommended {
            self.issues.push(ConfigIssue {
                severity: Severity::Info,
                parameter: "work_mem".to_string(),
                current_value: format_bytes(config.work_mem),
                recommended_value: format_bytes(
                    min_recommended,
                ),
                message: "work_mem is low; sorts and hash \
                          joins may spill to disk"
                    .to_string(),
                suggestion: format!(
                    "ALTER SYSTEM SET work_mem = '{}';",
                    format_bytes(min_recommended),
                ),
            });
        }

        let max_safe =
            config.system_ram / (u64::from(config.cpu_cores) * 4);
        if config.work_mem > max_safe {
            self.issues.push(ConfigIssue {
                severity: Severity::Warning,
                parameter: "work_mem".to_string(),
                current_value: format_bytes(config.work_mem),
                recommended_value: format_bytes(max_safe),
                message: "work_mem is dangerously high; \
                          concurrent queries may exhaust RAM"
                    .to_string(),
                suggestion: format!(
                    "ALTER SYSTEM SET work_mem = '{}';",
                    format_bytes(max_safe),
                ),
            });
        }
    }

    fn check_maintenance_work_mem(
        &mut self,
        config: &PgConfig,
    ) {
        let recommended = (config.system_ram / 16)
            .min(2 * 1024 * 1024 * 1024);
        let lower = recommended / 4;

        if config.maintenance_work_mem < lower {
            self.issues.push(ConfigIssue {
                severity: Severity::Info,
                parameter: "maintenance_work_mem".to_string(),
                current_value: format_bytes(
                    config.maintenance_work_mem,
                ),
                recommended_value: format_bytes(recommended),
                message: "maintenance_work_mem is low; \
                          VACUUM and CREATE INDEX will be \
                          slower"
                    .to_string(),
                suggestion: format!(
                    "ALTER SYSTEM SET maintenance_work_mem \
                     = '{}';",
                    format_bytes(recommended),
                ),
            });
        }
    }

    fn check_random_page_cost(&mut self, config: &PgConfig) {
        if config.is_ssd && config.random_page_cost > 1.5 {
            self.issues.push(ConfigIssue {
                severity: Severity::Warning,
                parameter: "random_page_cost".to_string(),
                current_value: format!(
                    "{:.1}",
                    config.random_page_cost,
                ),
                recommended_value: "1.1".to_string(),
                message: "random_page_cost is too high for \
                          SSD storage; planner overestimates \
                          random I/O cost"
                    .to_string(),
                suggestion: "ALTER SYSTEM SET \
                             random_page_cost = 1.1;"
                    .to_string(),
            });
        } else if !config.is_ssd
            && config.random_page_cost < 3.0
        {
            self.issues.push(ConfigIssue {
                severity: Severity::Info,
                parameter: "random_page_cost".to_string(),
                current_value: format!(
                    "{:.1}",
                    config.random_page_cost,
                ),
                recommended_value: "4.0".to_string(),
                message: "random_page_cost may be too low \
                          for HDD storage"
                    .to_string(),
                suggestion: "ALTER SYSTEM SET \
                             random_page_cost = 4.0;"
                    .to_string(),
            });
        }
    }

    fn check_effective_io_concurrency(
        &mut self,
        config: &PgConfig,
    ) {
        if config.is_ssd
            && config.effective_io_concurrency < 200
        {
            self.issues.push(ConfigIssue {
                severity: Severity::Info,
                parameter: "effective_io_concurrency"
                    .to_string(),
                current_value: config
                    .effective_io_concurrency
                    .to_string(),
                recommended_value: "200".to_string(),
                message: "effective_io_concurrency is low \
                          for SSD; bitmap scans won't \
                          prefetch efficiently"
                    .to_string(),
                suggestion: "ALTER SYSTEM SET \
                             effective_io_concurrency = 200;"
                    .to_string(),
            });
        }
    }

    fn check_statistics_target(
        &mut self,
        config: &PgConfig,
    ) {
        if config.default_statistics_target < 100 {
            self.issues.push(ConfigIssue {
                severity: Severity::Warning,
                parameter: "default_statistics_target"
                    .to_string(),
                current_value: config
                    .default_statistics_target
                    .to_string(),
                recommended_value: "100".to_string(),
                message: "default_statistics_target is below \
                          the standard value; planner may \
                          produce poor estimates"
                    .to_string(),
                suggestion: "ALTER SYSTEM SET \
                             default_statistics_target = 100;"
                    .to_string(),
            });
        }
    }

    fn check_parallelism(&mut self, config: &PgConfig) {
        if config.cpu_cores >= 4
            && config.max_parallel_workers_per_gather == 0
        {
            self.issues.push(ConfigIssue {
                severity: Severity::Warning,
                parameter:
                    "max_parallel_workers_per_gather"
                        .to_string(),
                current_value: "0".to_string(),
                recommended_value: format!(
                    "{}",
                    (config.cpu_cores / 2).max(2),
                ),
                message: "Parallelism is disabled on a \
                          multi-core system"
                    .to_string(),
                suggestion: format!(
                    "ALTER SYSTEM SET \
                     max_parallel_workers_per_gather = {};",
                    (config.cpu_cores / 2).max(2),
                ),
            });
        }
    }
}

impl Default for ConfigChecker {
    fn default() -> Self {
        Self::new()
    }
}

fn format_bytes(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;

    if bytes >= GB {
        format!("{}GB", bytes / GB)
    } else if bytes >= MB {
        format!("{}MB", bytes / MB)
    } else if bytes >= KB {
        format!("{}KB", bytes / KB)
    } else {
        format!("{bytes}B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = PgConfig::default();
        assert_eq!(config.shared_buffers, 128 * 1024 * 1024);
        assert_eq!(config.random_page_cost, 4.0);
        assert!(config.is_ssd);
    }

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1024), "1KB");
        assert_eq!(format_bytes(1024 * 1024), "1MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1GB");
        assert_eq!(
            format_bytes(4 * 1024 * 1024 * 1024),
            "4GB",
        );
    }

    #[test]
    fn detect_low_shared_buffers() {
        let mut checker = ConfigChecker::new();
        checker.load_config(PgConfig {
            shared_buffers: 32 * 1024 * 1024, // 32MB
            system_ram: 16 * 1024 * 1024 * 1024,
            ..PgConfig::default()
        });
        checker.analyze();

        let sb_issues: Vec<_> = checker
            .issues()
            .iter()
            .filter(|i| i.parameter == "shared_buffers")
            .collect();
        assert!(!sb_issues.is_empty());
        assert!(
            sb_issues[0].message.contains("too low"),
        );
    }

    #[test]
    fn detect_ssd_random_page_cost() {
        let mut checker = ConfigChecker::new();
        checker.load_config(PgConfig {
            random_page_cost: 4.0,
            is_ssd: true,
            ..PgConfig::default()
        });
        checker.analyze();

        let rpc_issues: Vec<_> = checker
            .issues()
            .iter()
            .filter(|i| i.parameter == "random_page_cost")
            .collect();
        assert!(!rpc_issues.is_empty());
        assert!(
            rpc_issues[0].message.contains("too high for SSD"),
        );
    }

    #[test]
    fn detect_low_io_concurrency_on_ssd() {
        let mut checker = ConfigChecker::new();
        checker.load_config(PgConfig {
            effective_io_concurrency: 1,
            is_ssd: true,
            ..PgConfig::default()
        });
        checker.analyze();

        let io_issues: Vec<_> = checker
            .issues()
            .iter()
            .filter(|i| {
                i.parameter == "effective_io_concurrency"
            })
            .collect();
        assert!(!io_issues.is_empty());
    }

    #[test]
    fn detect_disabled_parallelism() {
        let mut checker = ConfigChecker::new();
        checker.load_config(PgConfig {
            max_parallel_workers_per_gather: 0,
            cpu_cores: 8,
            ..PgConfig::default()
        });
        checker.analyze();

        let par_issues: Vec<_> = checker
            .issues()
            .iter()
            .filter(|i| {
                i.parameter
                    == "max_parallel_workers_per_gather"
            })
            .collect();
        assert!(!par_issues.is_empty());
        assert!(
            par_issues[0]
                .message
                .contains("disabled"),
        );
    }

    #[test]
    fn no_issues_with_good_config() {
        let mut checker = ConfigChecker::new();
        let ram: u64 = 16 * 1024 * 1024 * 1024;
        checker.load_config(PgConfig {
            shared_buffers: ram / 4,
            effective_cache_size: (ram * 3) / 4,
            work_mem: 64 * 1024 * 1024,
            maintenance_work_mem: ram / 16,
            random_page_cost: 1.1,
            effective_io_concurrency: 200,
            default_statistics_target: 100,
            max_parallel_workers_per_gather: 4,
            parallel_tuple_cost: 0.01,
            system_ram: ram,
            cpu_cores: 8,
            is_ssd: true,
        });
        checker.analyze();

        assert!(
            checker.issues().is_empty(),
            "Well-tuned config should produce no issues, \
             got: {:?}",
            checker.issues(),
        );
    }

    #[test]
    fn no_analysis_without_config() {
        let mut checker = ConfigChecker::new();
        checker.analyze();
        assert!(checker.issues().is_empty());
    }

    #[test]
    fn config_issue_display() {
        let issue = ConfigIssue {
            severity: Severity::Warning,
            parameter: "shared_buffers".to_string(),
            current_value: "128MB".to_string(),
            recommended_value: "4GB".to_string(),
            message: "too low".to_string(),
            suggestion: "increase it".to_string(),
        };
        let display = issue.to_string();
        assert!(display.contains("shared_buffers"));
        assert!(display.contains("128MB"));
    }
}
