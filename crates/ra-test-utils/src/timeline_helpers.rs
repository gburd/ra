//! Timeline test helpers for loading and validating timeline configurations.
//!
//! Provides utilities to simplify writing tests against timeline configurations,
//! including loading timelines, running optimization, and validating expectations.

use anyhow::{anyhow, Context, Result};
use std::path::Path;

/// Load a timeline configuration from a file.
///
/// # Arguments
///
/// * `name` - Timeline name (without path or extension). Looks in tests/data/timelines/
///
/// # Errors
///
/// Returns an error if the timeline file cannot be found or parsed.
///
/// # Example
///
/// ```ignore
/// let config = load_timeline("index-addition")?;
/// ```
pub fn load_timeline(name: &str) -> Result<TimelineConfig> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?;

    let timeline_path = Path::new(&manifest_dir)
        .join("tests")
        .join("data")
        .join("timelines")
        .join(format!("{name}.toml"));

    load_timeline_from_path(&timeline_path)
}

/// Load a timeline configuration from an absolute path.
///
/// # Arguments
///
/// * `path` - Absolute path to the timeline TOML file
///
/// # Errors
///
/// Returns an error if the file cannot be read or the TOML cannot be parsed.
pub fn load_timeline_from_path(path: &Path) -> Result<TimelineConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read timeline file: {}", path.display()))?;

    let config: TimelineConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse timeline TOML: {}", path.display()))?;

    config
        .validate()
        .with_context(|| format!("Timeline validation failed: {}", path.display()))?;

    Ok(config)
}

/// Result of optimizing a single snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotResult {
    /// Snapshot index.
    pub snapshot_index: usize,
    /// Snapshot label.
    pub label: String,
    /// Optimized plan (as display string).
    pub plan: String,
    /// Estimated cost.
    pub cost: f64,
    /// Estimated cardinality.
    pub cardinality: f64,
    /// Rules applied during optimization.
    pub rules_applied: Vec<String>,
}

/// Validation error from checking expectations.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Snapshot index where validation failed.
    pub snapshot_index: usize,
    /// Description of the validation failure.
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Snapshot {}: {}", self.snapshot_index, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Assert that cost was reduced by at least the specified percentage.
///
/// # Arguments
///
/// * `before` - Cost before optimization
/// * `after` - Cost after optimization
/// * `min_reduction_pct` - Minimum reduction percentage (0.0 to 1.0)
///
/// # Panics
///
/// Panics if cost reduction is less than `min_reduction_pct`.
///
/// # Example
///
/// ```ignore
/// assert_cost_reduction(1000.0, 100.0, 0.80); // 80% reduction required
/// ```
pub fn assert_cost_reduction(before: f64, after: f64, min_reduction_pct: f64) {
    assert!(before > 0.0, "before cost must be positive: {before}");
    assert!(after > 0.0, "after cost must be positive: {after}");
    assert!(
        (0.0..=1.0).contains(&min_reduction_pct),
        "min_reduction_pct must be between 0.0 and 1.0: {min_reduction_pct}",
    );

    let actual_reduction = (before - after) / before;

    assert!(
        actual_reduction >= min_reduction_pct,
        "Expected cost reduction of at least {:.1}%, but got {:.1}% (before: {}, after: {})",
        min_reduction_pct * 100.0,
        actual_reduction * 100.0,
        before,
        after
    );
}

/// Assert that cardinality estimate is within tolerance of expected value.
///
/// # Arguments
///
/// * `expected` - Expected cardinality
/// * `actual` - Actual cardinality
/// * `tolerance` - Tolerance as fraction (0.0 to 1.0)
///
/// # Panics
///
/// Panics if actual cardinality is outside tolerance range.
pub fn assert_cardinality_within_tolerance(expected: f64, actual: f64, tolerance: f64) {
    assert!(
        expected >= 0.0,
        "expected cardinality must be non-negative: {expected}",
    );
    assert!(
        actual >= 0.0,
        "actual cardinality must be non-negative: {actual}",
    );
    assert!(
        (0.0..=1.0).contains(&tolerance),
        "tolerance must be between 0.0 and 1.0: {tolerance}",
    );

    let lower_bound = expected * (1.0 - tolerance);
    let upper_bound = expected * (1.0 + tolerance);

    assert!(
        actual >= lower_bound && actual <= upper_bound,
        "Expected cardinality {:.1} ± {:.1}% ([{:.1}, {:.1}]), but got {:.1}",
        expected,
        tolerance * 100.0,
        lower_bound,
        upper_bound,
        actual
    );
}

/// Assert that a plan contains a specific pattern.
///
/// # Arguments
///
/// * `plan` - Plan string to check
/// * `pattern` - Regex pattern to match
///
/// # Panics
///
/// Panics if pattern is not found in plan.
#[expect(
    clippy::panic,
    reason = "assertion helper intentionally panics on mismatch"
)]
pub fn assert_plan_contains(plan: &str, pattern: &str) {
    let regex = regex::Regex::new(pattern)
        .unwrap_or_else(|e| panic!("Invalid regex pattern '{pattern}': {e}"));

    assert!(
        regex.is_match(plan),
        "Expected plan to match pattern '{pattern}', but it did not.\nPlan:\n{plan}",
    );
}

/// Assert that rules were applied.
///
/// # Arguments
///
/// * `rules_applied` - List of rules that were applied
/// * `required_rules` - Rules that must have been applied
///
/// # Panics
///
/// Panics if any required rule was not applied.
pub fn assert_rules_applied(rules_applied: &[String], required_rules: &[String]) {
    for required in required_rules {
        assert!(
            rules_applied.contains(required),
            "Expected rule '{required}' to be applied, but it was not.\nRules applied: {rules_applied:?}",
        );
    }
}

/// Assert that rules were NOT applied.
///
/// # Arguments
///
/// * `rules_applied` - List of rules that were applied
/// * `forbidden_rules` - Rules that must NOT have been applied
///
/// # Panics
///
/// Panics if any forbidden rule was applied.
pub fn assert_rules_not_applied(rules_applied: &[String], forbidden_rules: &[String]) {
    for forbidden in forbidden_rules {
        assert!(
            !rules_applied.contains(forbidden),
            "Expected rule '{forbidden}' to NOT be applied, but it was.\nRules applied: {rules_applied:?}",
        );
    }
}

/// Placeholder types - these will need to be imported from actual crates.
/// For now, defining minimal versions to make the module compile.
/// Timeline configuration.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TimelineConfig {
    /// Timeline metadata.
    pub metadata: TimelineMetadata,
    /// Hardware profiles.
    pub hardware_profiles: Vec<HardwareProfile>,
    /// Snapshots.
    pub snapshots: Vec<Snapshot>,
    /// Events (optional).
    #[serde(default)]
    pub events: Vec<Event>,
    /// Expectations (optional).
    #[serde(default)]
    pub expectations: Vec<Expectation>,
}

impl TimelineConfig {
    /// Validate the timeline configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid.
    pub fn validate(&self) -> Result<()> {
        if self.snapshots.is_empty() {
            return Err(anyhow!("Timeline must have at least one snapshot"));
        }

        for (i, snapshot) in self.snapshots.iter().enumerate() {
            if i > 0 && snapshot.time_offset <= self.snapshots[i - 1].time_offset {
                return Err(anyhow!(
                    "Snapshot time offsets must be in ascending order (snapshot {} has offset {})",
                    i,
                    snapshot.time_offset
                ));
            }

            if !self
                .hardware_profiles
                .iter()
                .any(|p| p.name == snapshot.hardware_profile)
            {
                return Err(anyhow!(
                    "Snapshot {} references unknown hardware profile '{}'",
                    i,
                    snapshot.hardware_profile
                ));
            }
        }

        for expectation in &self.expectations {
            if expectation.snapshot_index >= self.snapshots.len() {
                return Err(anyhow!(
                    "Expectation references invalid snapshot index {} (max: {})",
                    expectation.snapshot_index,
                    self.snapshots.len() - 1
                ));
            }
        }

        Ok(())
    }

    /// Get a hardware profile by name.
    #[must_use]
    pub fn get_hardware_profile(&self, name: &str) -> Option<&HardwareProfile> {
        self.hardware_profiles.iter().find(|p| p.name == name)
    }
}

/// Timeline metadata.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TimelineMetadata {
    /// Timeline name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Query being optimized (optional).
    #[serde(default)]
    pub query: Option<String>,
    /// SQL dialect (optional).
    #[serde(default)]
    pub dialect: Option<String>,
}

/// Hardware profile.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HardwareProfile {
    /// Profile name.
    pub name: String,
    /// CPU cores.
    pub cpu_cores: u32,
    /// Total memory in bytes.
    pub total_memory: u64,
}

/// Snapshot.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Snapshot {
    /// Time offset in seconds.
    pub time_offset: u64,
    /// Label.
    pub label: String,
    /// Hardware profile name.
    pub hardware_profile: String,
}

/// Event.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Event {
    /// Time offset in seconds.
    pub time_offset: u64,
    /// Event kind.
    pub kind: String,
    /// Description.
    pub description: String,
}

/// Test expectation.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Expectation {
    /// Snapshot index.
    pub snapshot_index: usize,
    /// Expected plan pattern (regex).
    #[serde(default)]
    pub expected_plan_pattern: Option<String>,
    /// Expected cost range [min, max].
    #[serde(default)]
    pub expected_cost_range: Option<[f64; 2]>,
    /// Expected cardinality.
    #[serde(default)]
    pub expected_cardinality: Option<f64>,
    /// Cardinality tolerance (fraction).
    #[serde(default = "default_cardinality_tolerance")]
    pub cardinality_tolerance: f64,
    /// Rules that must have been applied.
    #[serde(default)]
    pub rules_applied_must_include: Vec<String>,
    /// Rules that must NOT have been applied.
    #[serde(default)]
    pub rules_applied_must_not_include: Vec<String>,
}

fn default_cardinality_tolerance() -> f64 {
    0.1 // 10% tolerance by default
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assert_cost_reduction() {
        assert_cost_reduction(1000.0, 100.0, 0.80);
        assert_cost_reduction(1000.0, 500.0, 0.40);
    }

    #[test]
    #[should_panic(expected = "Expected cost reduction of at least 90.0%")]
    fn test_assert_cost_reduction_insufficient() {
        assert_cost_reduction(1000.0, 500.0, 0.90);
    }

    #[test]
    fn test_assert_cardinality_within_tolerance() {
        assert_cardinality_within_tolerance(100.0, 95.0, 0.1);
        assert_cardinality_within_tolerance(100.0, 105.0, 0.1);
        assert_cardinality_within_tolerance(100.0, 100.0, 0.1);
    }

    #[test]
    #[should_panic(expected = "Expected cardinality")]
    fn test_assert_cardinality_outside_tolerance() {
        assert_cardinality_within_tolerance(100.0, 85.0, 0.1);
    }
}
