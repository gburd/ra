//! Regression detection CLI commands.
//!
//! TODO: These commands are currently stubbed out due to dependencies on
//! DataFusion and API changes. Need to either:
//! 1. Remove DataFusion dependency and use ra-parser directly
//! 2. Update HardwareProfile API calls (from_preset, auto_detect removed)
//! 3. Implement expr_to_sql or use alternate serialization
//! 4. Make functions async or remove .await calls

use anyhow::{bail, Result};
use std::path::Path;

/// Establish a baseline for a query.
pub fn cmd_regression_baseline(
    _query_file: &Path,
    _query_id: Option<&str>,
    _storage_type: &str,
    _storage_path: &Path,
    _hardware_profile: &str,
    _verbose: bool,
    _quiet: bool,
) -> Result<()> {
    bail!("Regression baseline command is currently disabled - requires DataFusion integration")
}

/// Check for regressions in a query.
pub fn cmd_regression_check(
    _query_file: &Path,
    _query_id: Option<&str>,
    _storage_type: &str,
    _storage_path: &Path,
    _hardware_profile: &str,
    _warn_threshold: Option<f64>,
    _error_threshold: Option<f64>,
    _verbose: bool,
    _quiet: bool,
) -> Result<()> {
    bail!("Regression check command is currently disabled - requires DataFusion integration")
}

/// Show regression report for all queries.
pub fn cmd_regression_report(
    _storage_type: &str,
    _storage_path: &Path,
    _format: &str,
    _only_regressions: bool,
    _verbose: bool,
    _quiet: bool,
) -> Result<()> {
    bail!("Regression report command is currently disabled - requires DataFusion integration")
}
