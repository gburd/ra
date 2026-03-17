//! Timely dataflow worker configuration and management.
//!
//! Provides configuration types and helper functions for managing
//! timely dataflow workers that power the incremental optimizer.
//! Workers run differential dataflow computations that incrementally
//! update optimization results when rules or queries change.

use std::fmt;

/// Configuration for the timely dataflow runtime.
#[derive(Debug, Clone)]
pub struct TimelyConfig {
    /// Number of worker threads.
    pub workers: usize,
    /// Whether to use process-level parallelism (multiple
    /// threads) or a single thread.
    pub process_parallelism: bool,
}

impl Default for TimelyConfig {
    fn default() -> Self {
        Self {
            workers: 1,
            process_parallelism: false,
        }
    }
}

impl TimelyConfig {
    /// Create a single-threaded configuration.
    #[must_use]
    pub fn single_thread() -> Self {
        Self::default()
    }

    /// Create a multi-threaded configuration.
    #[must_use]
    pub fn multi_thread(workers: usize) -> Self {
        Self {
            workers: workers.max(1),
            process_parallelism: true,
        }
    }

    /// Convert to a timely `Config`.
    #[must_use]
    pub fn to_timely_config(&self) -> timely::Config {
        if self.process_parallelism && self.workers > 1 {
            timely::Config::process(self.workers)
        } else {
            timely::Config::thread()
        }
    }
}

/// Statistics about a timely dataflow computation.
#[derive(Debug, Clone, Default)]
pub struct ComputationStats {
    /// Number of dataflow steps executed.
    pub steps: u64,
    /// Number of input records processed.
    pub input_records: u64,
    /// Number of output records produced.
    pub output_records: u64,
    /// Current logical timestamp.
    pub current_time: u64,
}

impl fmt::Display for ComputationStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "steps={}, inputs={}, outputs={}, time={}",
            self.steps, self.input_records, self.output_records, self.current_time,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_single_thread() {
        let config = TimelyConfig::default();
        assert_eq!(config.workers, 1);
        assert!(!config.process_parallelism);
    }

    #[test]
    fn multi_thread_config() {
        let config = TimelyConfig::multi_thread(4);
        assert_eq!(config.workers, 4);
        assert!(config.process_parallelism);
    }

    #[test]
    fn multi_thread_clamps_to_one() {
        let config = TimelyConfig::multi_thread(0);
        assert_eq!(config.workers, 1);
    }

    #[test]
    fn computation_stats_display() {
        let stats = ComputationStats {
            steps: 10,
            input_records: 100,
            output_records: 50,
            current_time: 5,
        };
        let s = stats.to_string();
        assert!(s.contains("steps=10"));
        assert!(s.contains("inputs=100"));
        assert!(s.contains("outputs=50"));
        assert!(s.contains("time=5"));
    }

    #[test]
    fn to_timely_config_thread() {
        let config = TimelyConfig::single_thread();
        let _tc = config.to_timely_config();
    }

    #[test]
    fn to_timely_config_process() {
        let config = TimelyConfig::multi_thread(2);
        let _tc = config.to_timely_config();
    }
}
