//! Timeline-based optimization loop with change detection.
//!
//! This module implements Phase 2 of the timeline-based fingerprint configuration
//! system. It processes timeline configurations by:
//! - Iterating through each snapshot in time order
//! - Creating a `SnapshotFactsProvider` for each snapshot
//! - Running the optimizer with snapshot-specific facts
//! - Tracking plan evolution and dependencies
//! - Detecting changes in schema, statistics, hardware, and facts
//! - Recording optimization results for each snapshot
//!
//! # Example
//!
//! ```
//! use ra_engine::{TimelineConfig, TimelineOptimizer, Optimizer};
//! use ra_core::algebra::RelExpr;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = TimelineConfig::from_file("timeline.toml".as_ref())?;
//! let query = RelExpr::scan("orders");
//!
//! let optimizer = Optimizer::new();
//! let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);
//!
//! let result = timeline_optimizer.optimize_timeline()?;
//!
//! // Analyze results
//! for snapshot_result in &result.snapshot_results {
//!     println!("Snapshot {}: cost = {}, changes = {}",
//!         snapshot_result.snapshot_index,
//!         snapshot_result.cost,
//!         snapshot_result.changes_from_previous.len()
//!     );
//! }
//! # Ok(())
//! # }
//! ```

use crate::differential::{PlanDependencies, StalenessThresholds};
use crate::egraph::{EGraphError, Optimizer};
use crate::timeline_config::{
    FingerPrintSnapshot, SchemaSnapshot, StatisticsSnapshot, TimelineConfig,
};
use crate::timeline_facts::SnapshotFactsProvider;
use ra_core::algebra::RelExpr;
use ra_core::facts::HardwareProfile;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::{debug, info};

/// Result of optimizing a timeline configuration.
#[derive(Debug, Clone)]
pub struct TimelineOptimizationResult {
    /// Results for each snapshot in time order.
    pub snapshot_results: Vec<SnapshotResult>,
    /// The timeline configuration that was optimized.
    pub timeline_config: TimelineConfig,
}

/// Result of optimizing a single snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotResult {
    /// Snapshot index (0-based).
    pub snapshot_index: usize,
    /// Time offset in seconds from timeline start.
    pub time_offset: u64,
    /// Optional label for this snapshot.
    pub label: Option<String>,
    /// Optimized plan as a formatted string.
    pub optimized_plan: String,
    /// Estimated cost of the optimized plan.
    pub cost: f64,
    /// Optimization time in milliseconds.
    pub optimization_time_ms: u128,
    /// Names of rules that were applied.
    pub rules_applied: Vec<String>,
    /// Plan dependencies on statistics resources.
    pub dependencies: PlanDependencies,
    /// Changes detected from the previous snapshot.
    pub changes_from_previous: Vec<ChangeDescription>,
}

/// Description of a detected change between snapshots.
#[derive(Debug, Clone)]
pub struct ChangeDescription {
    /// Type of change.
    pub change_type: ChangeType,
    /// Severity level.
    pub severity: ChangeSeverity,
    /// Human-readable description.
    pub description: String,
}

/// Type of change detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// Schema change (index, column, constraint).
    Schema,
    /// Statistics change (row count, NDV, histogram).
    Statistics,
    /// Hardware profile change (CPU, memory, GPU).
    Hardware,
    /// Facts change (feature flags, configuration).
    Facts,
}

/// Severity level of a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeSeverity {
    /// Minor change (unlikely to affect plan).
    Low,
    /// Moderate change (may affect plan).
    Medium,
    /// Major change (likely to affect plan).
    High,
    /// Critical change (almost certainly affects plan).
    Critical,
}

/// Timeline optimizer that processes snapshots and tracks plan evolution.
pub struct TimelineOptimizer {
    /// Timeline configuration.
    config: TimelineConfig,
    /// Query being optimized.
    query: RelExpr,
    /// Optimizer instance.
    optimizer: Optimizer,
    /// Staleness thresholds for change detection.
    thresholds: StalenessThresholds,
}

impl TimelineOptimizer {
    /// Create a new timeline optimizer.
    pub fn new(config: TimelineConfig, query: RelExpr, optimizer: Optimizer) -> Self {
        Self {
            config,
            query,
            optimizer,
            thresholds: StalenessThresholds::default(),
        }
    }

    /// Create with custom staleness thresholds.
    pub fn with_thresholds(
        config: TimelineConfig,
        query: RelExpr,
        optimizer: Optimizer,
        thresholds: StalenessThresholds,
    ) -> Self {
        Self {
            config,
            query,
            optimizer,
            thresholds,
        }
    }

    /// Optimize the query at each snapshot in the timeline.
    ///
    /// # Errors
    ///
    /// Returns an error if optimization fails for any snapshot.
    pub fn optimize_timeline(&mut self) -> Result<TimelineOptimizationResult, EGraphError> {
        info!(
            "Starting timeline optimization: {} snapshots",
            self.config.snapshots.len()
        );

        let mut snapshot_results = Vec::new();
        let mut previous_snapshot: Option<&FingerPrintSnapshot> = None;
        let mut previous_hardware: Option<HardwareProfile> = None;

        for (index, snapshot) in self.config.snapshots.iter().enumerate() {
            info!(
                "Optimizing snapshot {} at t={}s: {:?}",
                index, snapshot.time_offset, snapshot.label
            );

            let start = Instant::now();

            // Get hardware profile for this snapshot
            let hardware_def = self
                .config
                .get_hardware_profile(&snapshot.hardware_profile)
                .ok_or_else(|| {
                    EGraphError::ConversionError(format!(
                        "Hardware profile '{}' not found",
                        snapshot.hardware_profile
                    ))
                })?;

            let hardware = hardware_def.to_hardware_profile();

            // Detect changes from previous snapshot
            let changes = if let Some(prev) = previous_snapshot {
                self.detect_changes(prev, snapshot, previous_hardware.as_ref(), &hardware)
            } else {
                Vec::new()
            };

            // Create facts provider for this snapshot
            let facts = SnapshotFactsProvider::new(snapshot, hardware_def);

            // Optimize with snapshot facts
            let optimized = self.optimizer.optimize_with_facts(&self.query, &facts)?;

            // Extract plan details
            let optimized_plan = format!("{optimized:?}");
            let cost = 0.0; // Cost extraction would require StatisticsProvider access

            // Track dependencies (simplified - would extract from actual plan)
            let dependencies = PlanDependencies {
                table_cardinalities: extract_table_cardinalities(snapshot),
                indexes: extract_indexes(snapshot),
                distinct_counts: extract_distinct_counts(snapshot),
                histogram_digests: HashMap::new(),
                facts: extract_fact_dependencies(snapshot),
            };

            let optimization_time_ms = start.elapsed().as_millis();

            let result = SnapshotResult {
                snapshot_index: index,
                time_offset: snapshot.time_offset,
                label: snapshot.label.clone(),
                optimized_plan,
                cost,
                optimization_time_ms,
                rules_applied: Vec::new(), // Would extract from optimizer
                dependencies,
                changes_from_previous: changes,
            };

            debug!(
                "Snapshot {} optimized in {}ms, detected {} changes",
                index,
                optimization_time_ms,
                result.changes_from_previous.len()
            );

            snapshot_results.push(result);
            previous_snapshot = Some(snapshot);
            previous_hardware = Some(hardware);
        }

        info!(
            "Timeline optimization complete: {} snapshots",
            snapshot_results.len()
        );

        Ok(TimelineOptimizationResult {
            snapshot_results,
            timeline_config: self.config.clone(),
        })
    }

    /// Detect changes between two snapshots.
    fn detect_changes(
        &self,
        prev: &FingerPrintSnapshot,
        current: &FingerPrintSnapshot,
        prev_hardware: Option<&HardwareProfile>,
        current_hardware: &HardwareProfile,
    ) -> Vec<ChangeDescription> {
        let mut changes = Vec::new();

        // Detect schema changes
        changes.extend(detect_schema_changes(
            &prev.schema,
            &current.schema,
            &self.thresholds,
        ));

        // Detect statistics changes
        changes.extend(detect_stats_changes(
            &prev.statistics,
            &current.statistics,
            &self.thresholds,
        ));

        // Detect hardware changes
        if let Some(prev_hw) = prev_hardware {
            changes.extend(detect_hardware_changes(prev_hw, current_hardware));
        }

        // Detect fact changes
        changes.extend(detect_fact_changes(&prev.facts, &current.facts));

        changes
    }

    /// Get the timeline configuration.
    pub fn config(&self) -> &TimelineConfig {
        &self.config
    }

    /// Get the query being optimized.
    pub fn query(&self) -> &RelExpr {
        &self.query
    }

    /// Get the staleness thresholds.
    pub fn thresholds(&self) -> &StalenessThresholds {
        &self.thresholds
    }
}

/// Detect schema changes between snapshots.
pub fn detect_schema_changes(
    prev: &SchemaSnapshot,
    current: &SchemaSnapshot,
    thresholds: &StalenessThresholds,
) -> Vec<ChangeDescription> {
    let mut changes = Vec::new();

    let prev_tables: HashMap<_, _> = prev.tables.iter().map(|t| (&t.name, t)).collect();
    let current_tables: HashMap<_, _> = current.tables.iter().map(|t| (&t.name, t)).collect();

    // Detect added/removed tables
    for name in current_tables.keys() {
        if !prev_tables.contains_key(name) {
            changes.push(ChangeDescription {
                change_type: ChangeType::Schema,
                severity: ChangeSeverity::Critical,
                description: format!("Table '{name}' added"),
            });
        }
    }

    for name in prev_tables.keys() {
        if !current_tables.contains_key(name) {
            changes.push(ChangeDescription {
                change_type: ChangeType::Schema,
                severity: ChangeSeverity::Critical,
                description: format!("Table '{name}' removed"),
            });
        }
    }

    // Detect index changes for common tables
    for (name, current_table) in &current_tables {
        if let Some(prev_table) = prev_tables.get(name) {
            let prev_indexes: HashSet<_> =
                prev_table.indexes.iter().map(|i| &i.name).collect();
            let current_indexes: HashSet<_> =
                current_table.indexes.iter().map(|i| &i.name).collect();

            for idx in &current_indexes {
                if !prev_indexes.contains(idx) {
                    let severity = if thresholds.index_changes_trigger {
                        ChangeSeverity::High
                    } else {
                        ChangeSeverity::Medium
                    };
                    changes.push(ChangeDescription {
                        change_type: ChangeType::Schema,
                        severity,
                        description: format!("Index '{idx}' added to table '{name}'"),
                    });
                }
            }

            for idx in &prev_indexes {
                if !current_indexes.contains(idx) {
                    let severity = if thresholds.index_changes_trigger {
                        ChangeSeverity::High
                    } else {
                        ChangeSeverity::Medium
                    };
                    changes.push(ChangeDescription {
                        change_type: ChangeType::Schema,
                        severity,
                        description: format!("Index '{idx}' dropped from table '{name}'"),
                    });
                }
            }

            // Detect column changes
            let prev_columns: HashSet<_> =
                prev_table.columns.iter().map(|c| &c.name).collect();
            let current_columns: HashSet<_> =
                current_table.columns.iter().map(|c| &c.name).collect();

            for col in &current_columns {
                if !prev_columns.contains(col) {
                    changes.push(ChangeDescription {
                        change_type: ChangeType::Schema,
                        severity: ChangeSeverity::High,
                        description: format!("Column '{col}' added to table '{name}'"),
                    });
                }
            }

            for col in &prev_columns {
                if !current_columns.contains(col) {
                    changes.push(ChangeDescription {
                        change_type: ChangeType::Schema,
                        severity: ChangeSeverity::High,
                        description: format!("Column '{col}' removed from table '{name}'"),
                    });
                }
            }
        }
    }

    changes
}

/// Detect statistics changes between snapshots.
pub fn detect_stats_changes(
    prev: &StatisticsSnapshot,
    current: &StatisticsSnapshot,
    thresholds: &StalenessThresholds,
) -> Vec<ChangeDescription> {
    let mut changes = Vec::new();

    let prev_tables: HashMap<_, _> = prev.tables.iter().map(|t| (&t.name, t)).collect();
    let current_tables: HashMap<_, _> = current.tables.iter().map(|t| (&t.name, t)).collect();

    for (name, current_table) in &current_tables {
        if let Some(prev_table) = prev_tables.get(name) {
            // Check row count changes
            let ratio = crate::differential::change_ratio(
                prev_table.row_count as f64,
                current_table.row_count as f64,
            );

            if ratio >= thresholds.cardinality_ratio {
                let severity = if ratio >= 10.0 {
                    ChangeSeverity::Critical
                } else if ratio >= 5.0 {
                    ChangeSeverity::High
                } else {
                    ChangeSeverity::Medium
                };

                changes.push(ChangeDescription {
                    change_type: ChangeType::Statistics,
                    severity,
                    description: format!(
                        "Table '{name}' row count changed from {} to {} (ratio: {:.2}x)",
                        prev_table.row_count, current_table.row_count, ratio
                    ),
                });
            }

            // Check column NDV changes
            let prev_cols: HashMap<_, _> =
                prev_table.columns.iter().map(|c| (&c.name, c)).collect();
            let current_cols: HashMap<_, _> =
                current_table.columns.iter().map(|c| (&c.name, c)).collect();

            for (col_name, current_col) in &current_cols {
                if let Some(prev_col) = prev_cols.get(col_name) {
                    let ndv_ratio = crate::differential::change_ratio(
                        prev_col.ndv as f64,
                        current_col.ndv as f64,
                    );

                    if ndv_ratio >= thresholds.ndistinct_ratio {
                        let severity = if ndv_ratio >= 5.0 {
                            ChangeSeverity::High
                        } else if ndv_ratio >= 2.0 {
                            ChangeSeverity::Medium
                        } else {
                            ChangeSeverity::Low
                        };

                        changes.push(ChangeDescription {
                            change_type: ChangeType::Statistics,
                            severity,
                            description: format!(
                                "Column '{name}.{col_name}' NDV changed from {} to {} (ratio: {:.2}x)",
                                prev_col.ndv, current_col.ndv, ndv_ratio
                            ),
                        });
                    }
                }
            }
        }
    }

    changes
}

/// Detect hardware profile changes.
pub fn detect_hardware_changes(
    prev: &HardwareProfile,
    current: &HardwareProfile,
) -> Vec<ChangeDescription> {
    let mut changes = Vec::new();

    // CPU cores change
    if prev.cpu_cores != current.cpu_cores {
        let severity = if current.cpu_cores > prev.cpu_cores * 2
            || prev.cpu_cores > current.cpu_cores * 2
        {
            ChangeSeverity::High
        } else {
            ChangeSeverity::Medium
        };

        changes.push(ChangeDescription {
            change_type: ChangeType::Hardware,
            severity,
            description: format!(
                "CPU cores changed from {} to {}",
                prev.cpu_cores, current.cpu_cores
            ),
        });
    }

    // Memory change
    let mem_ratio = crate::differential::change_ratio(
        prev.available_memory as f64,
        current.available_memory as f64,
    );

    if mem_ratio >= 1.5 {
        let severity = if mem_ratio >= 4.0 {
            ChangeSeverity::High
        } else {
            ChangeSeverity::Medium
        };

        changes.push(ChangeDescription {
            change_type: ChangeType::Hardware,
            severity,
            description: format!(
                "Available memory changed from {} to {} bytes (ratio: {:.2}x)",
                prev.available_memory, current.available_memory, mem_ratio
            ),
        });
    }

    // SIMD width change
    if prev.simd_width != current.simd_width {
        changes.push(ChangeDescription {
            change_type: ChangeType::Hardware,
            severity: ChangeSeverity::Low,
            description: format!(
                "SIMD width changed from {} to {} bits",
                prev.simd_width, current.simd_width
            ),
        });
    }

    // GPU availability change
    if prev.has_gpu != current.has_gpu {
        changes.push(ChangeDescription {
            change_type: ChangeType::Hardware,
            severity: ChangeSeverity::High,
            description: format!(
                "GPU availability changed from {} to {}",
                prev.has_gpu, current.has_gpu
            ),
        });
    }

    changes
}

/// Detect fact changes between snapshots.
pub fn detect_fact_changes(
    prev: &crate::timeline_config::FactsSnapshot,
    current: &crate::timeline_config::FactsSnapshot,
) -> Vec<ChangeDescription> {
    let mut changes = Vec::new();

    // Hash join support
    if prev.supports_hash_join != current.supports_hash_join {
        changes.push(ChangeDescription {
            change_type: ChangeType::Facts,
            severity: ChangeSeverity::High,
            description: format!(
                "Hash join support changed from {:?} to {:?}",
                prev.supports_hash_join, current.supports_hash_join
            ),
        });
    }

    // Parallel scan support
    if prev.supports_parallel_scan != current.supports_parallel_scan {
        changes.push(ChangeDescription {
            change_type: ChangeType::Facts,
            severity: ChangeSeverity::High,
            description: format!(
                "Parallel scan support changed from {:?} to {:?}",
                prev.supports_parallel_scan, current.supports_parallel_scan
            ),
        });
    }

    // Parallel workers
    if prev.parallel_workers != current.parallel_workers {
        let severity = match (prev.parallel_workers, current.parallel_workers) {
            (Some(p), Some(c)) if p * 2 < c || c * 2 < p => ChangeSeverity::High,
            _ => ChangeSeverity::Medium,
        };

        changes.push(ChangeDescription {
            change_type: ChangeType::Facts,
            severity,
            description: format!(
                "Parallel workers changed from {:?} to {:?}",
                prev.parallel_workers, current.parallel_workers
            ),
        });
    }

    // Work memory
    if prev.work_mem_bytes != current.work_mem_bytes {
        if let (Some(p), Some(c)) = (prev.work_mem_bytes, current.work_mem_bytes) {
            let ratio = crate::differential::change_ratio(p as f64, c as f64);
            if ratio >= 2.0 {
                changes.push(ChangeDescription {
                    change_type: ChangeType::Facts,
                    severity: ChangeSeverity::Medium,
                    description: format!(
                        "Work memory changed from {} to {} bytes (ratio: {:.2}x)",
                        p, c, ratio
                    ),
                });
            }
        }
    }

    changes
}

/// Extract table cardinalities from snapshot.
fn extract_table_cardinalities(snapshot: &FingerPrintSnapshot) -> HashMap<String, f64> {
    snapshot
        .statistics
        .tables
        .iter()
        .map(|t| (t.name.clone(), t.row_count as f64))
        .collect()
}

/// Extract indexes from snapshot.
fn extract_indexes(snapshot: &FingerPrintSnapshot) -> HashSet<(String, String)> {
    let mut indexes = HashSet::new();
    for table in &snapshot.schema.tables {
        for index in &table.indexes {
            indexes.insert((table.name.clone(), index.name.clone()));
        }
    }
    indexes
}

/// Extract distinct counts from snapshot.
fn extract_distinct_counts(snapshot: &FingerPrintSnapshot) -> HashMap<(String, String), f64> {
    let mut counts = HashMap::new();
    for table in &snapshot.statistics.tables {
        for col in &table.columns {
            counts.insert((table.name.clone(), col.name.clone()), col.ndv as f64);
        }
    }
    counts
}

/// Extract fact dependencies from snapshot.
fn extract_fact_dependencies(snapshot: &FingerPrintSnapshot) -> HashSet<String> {
    let mut facts = HashSet::new();

    if snapshot.facts.supports_hash_join == Some(true) {
        facts.insert("hash_join".to_string());
    }

    if snapshot.facts.supports_parallel_scan == Some(true) {
        facts.insert("parallel_scan".to_string());
    }

    if snapshot.facts.parallel_workers.is_some() {
        facts.insert("parallel_workers".to_string());
    }

    facts
}

impl TimelineOptimizationResult {
    /// Serialize to JSON format.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self)
    }

    /// Serialize to TOML format.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(&self)
    }

    /// Format as Markdown report.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("# Timeline Optimization Report: {}\n\n", self.timeline_config.metadata.name));
        md.push_str(&format!("{}\n\n", self.timeline_config.metadata.description));

        if let Some(query) = &self.timeline_config.metadata.query {
            md.push_str(&format!("**Query:** `{query}`\n\n"));
        }

        md.push_str(&format!("**Snapshots:** {}\n\n", self.snapshot_results.len()));
        md.push_str(&format!("**Duration:** {} seconds\n\n", self.timeline_config.metadata.duration_seconds.unwrap_or(0)));

        md.push_str("## Snapshots\n\n");

        for result in &self.snapshot_results {
            md.push_str(&format!("### Snapshot {} (t={}s)\n\n", result.snapshot_index, result.time_offset));

            if let Some(label) = &result.label {
                md.push_str(&format!("**Label:** {label}\n\n"));
            }

            md.push_str(&format!("- **Cost:** {:.2}\n", result.cost));
            md.push_str(&format!("- **Optimization time:** {}ms\n", result.optimization_time_ms));
            md.push_str(&format!("- **Changes detected:** {}\n", result.changes_from_previous.len()));

            if !result.changes_from_previous.is_empty() {
                md.push_str("\n**Changes from previous snapshot:**\n\n");
                for change in &result.changes_from_previous {
                    let severity_icon = match change.severity {
                        ChangeSeverity::Low => "ℹ️",
                        ChangeSeverity::Medium => "⚠️",
                        ChangeSeverity::High => "🔴",
                        ChangeSeverity::Critical => "🚨",
                    };
                    md.push_str(&format!("- {severity_icon} [{:?}] {}\n", change.change_type, change.description));
                }
            }

            md.push_str("\n");
        }

        md
    }

    /// Format as text with ASCII tables.
    pub fn to_text(&self) -> String {
        let mut text = String::new();

        text.push_str(&format!("Timeline Optimization Report: {}\n", self.timeline_config.metadata.name));
        text.push_str(&format!("{}\n", self.timeline_config.metadata.description));
        text.push_str(&format!("Snapshots: {}\n\n", self.snapshot_results.len()));

        text.push_str("┌──────┬─────────┬──────────┬──────────┬──────────┐\n");
        text.push_str("│ Snap │ Time(s) │   Cost   │  Time(ms)│  Changes │\n");
        text.push_str("├──────┼─────────┼──────────┼──────────┼──────────┤\n");

        for result in &self.snapshot_results {
            text.push_str(&format!(
                "│ {:>4} │ {:>7} │ {:>8.2} │ {:>8} │ {:>8} │\n",
                result.snapshot_index,
                result.time_offset,
                result.cost,
                result.optimization_time_ms,
                result.changes_from_previous.len()
            ));
        }

        text.push_str("└──────┴─────────┴──────────┴──────────┴──────────┘\n");

        text
    }
}

// Implement Serialize for result types
use serde::Serialize;

impl Serialize for TimelineOptimizationResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("TimelineOptimizationResult", 2)?;
        state.serialize_field("snapshot_results", &self.snapshot_results)?;
        state.serialize_field("timeline_name", &self.timeline_config.metadata.name)?;
        state.end()
    }
}

impl Serialize for SnapshotResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("SnapshotResult", 8)?;
        state.serialize_field("snapshot_index", &self.snapshot_index)?;
        state.serialize_field("time_offset", &self.time_offset)?;
        state.serialize_field("label", &self.label)?;
        state.serialize_field("cost", &self.cost)?;
        state.serialize_field("optimization_time_ms", &self.optimization_time_ms)?;
        state.serialize_field("rules_applied", &self.rules_applied)?;
        state.serialize_field("changes_from_previous", &self.changes_from_previous)?;
        state.serialize_field("dependencies_count", &self.dependencies.all_resources().len())?;
        state.end()
    }
}

impl Serialize for ChangeDescription {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ChangeDescription", 3)?;
        state.serialize_field("change_type", &format!("{:?}", self.change_type))?;
        state.serialize_field("severity", &format!("{:?}", self.severity))?;
        state.serialize_field("description", &self.description)?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_config::{
        ColumnDef, ColumnStatsDef, DataTypeDef, FactsSnapshot, HardwareProfileDef,
        IndexDef, IndexTypeDef, SchemaSnapshot, StorageFormatDef, TableDef,
        TableStatsDef, TimelineMetadata,
    };

    fn create_test_config() -> TimelineConfig {
        let hardware = HardwareProfileDef {
            name: "test".to_string(),
            cpu_cores: 4,
            total_memory: 8_000_000_000,
            available_memory: Some(6_000_000_000),
            simd_width: 256,
            has_gpu: false,
            gpu_memory: None,
            l1_cache_size: 32768,
            l2_cache_size: 262144,
            l3_cache_size: 8388608,
        };

        let snapshot1 = FingerPrintSnapshot {
            time_offset: 0,
            label: Some("Initial".to_string()),
            hardware_profile: "test".to_string(),
            schema: SchemaSnapshot {
                tables: vec![TableDef {
                    name: "users".to_string(),
                    storage_format: StorageFormatDef::RowBased,
                    columns: vec![ColumnDef {
                        name: "id".to_string(),
                        data_type: DataTypeDef::Integer,
                        nullable: false,
                    }],
                    indexes: vec![],
                    primary_key: vec!["id".to_string()],
                    foreign_keys: vec![],
                }],
            },
            statistics: StatisticsSnapshot {
                tables: vec![TableStatsDef {
                    name: "users".to_string(),
                    row_count: 1000,
                    page_count: Some(10),
                    avg_row_size: Some(100.0),
                    table_size_bytes: Some(100_000),
                    columns: vec![ColumnStatsDef {
                        name: "id".to_string(),
                        ndv: 1000,
                        null_fraction: 0.0,
                        avg_width: 8.0,
                        correlation: Some(1.0),
                        min_value: None,
                        max_value: None,
                    }],
                }],
            },
            facts: FactsSnapshot {
                supports_hash_join: Some(true),
                supports_parallel_scan: Some(false),
                parallel_workers: Some(1),
                work_mem_bytes: Some(64 * 1024 * 1024),
                custom: HashMap::new(),
            },
        };

        TimelineConfig {
            metadata: TimelineMetadata {
                name: "Test Timeline".to_string(),
                description: "Test".to_string(),
                query: Some("SELECT * FROM users".to_string()),
                dialect: Some("postgresql".to_string()),
                duration_seconds: Some(3600),
                schema: None,
                scale_factor: None,
            },
            hardware_profiles: vec![hardware],
            snapshots: vec![snapshot1],
            events: vec![],
            expectations: vec![],
        }
    }

    #[test]
    fn timeline_optimizer_creation() {
        let config = create_test_config();
        let query = RelExpr::scan("users");
        let optimizer = Optimizer::new();
        let timeline_opt = TimelineOptimizer::new(config, query, optimizer);
        assert_eq!(timeline_opt.config().snapshots.len(), 1);
    }

    #[test]
    fn detect_index_addition() {
        let prev = SchemaSnapshot {
            tables: vec![TableDef {
                name: "users".to_string(),
                storage_format: StorageFormatDef::RowBased,
                columns: vec![],
                indexes: vec![],
                primary_key: vec![],
                foreign_keys: vec![],
            }],
        };

        let current = SchemaSnapshot {
            tables: vec![TableDef {
                name: "users".to_string(),
                storage_format: StorageFormatDef::RowBased,
                columns: vec![],
                indexes: vec![IndexDef {
                    name: "idx_users_id".to_string(),
                    index_type: IndexTypeDef::Btree,
                    columns: vec!["id".to_string()],
                    included_columns: vec![],
                    is_unique: false,
                }],
                primary_key: vec![],
                foreign_keys: vec![],
            }],
        };

        let thresholds = StalenessThresholds::default();
        let changes = detect_schema_changes(&prev, &current, &thresholds);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0].change_type, ChangeType::Schema));
        assert!(changes[0].description.contains("idx_users_id"));
    }

    #[test]
    fn detect_row_count_change() {
        let prev = StatisticsSnapshot {
            tables: vec![TableStatsDef {
                name: "users".to_string(),
                row_count: 1000,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![],
            }],
        };

        let current = StatisticsSnapshot {
            tables: vec![TableStatsDef {
                name: "users".to_string(),
                row_count: 10_000,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![],
            }],
        };

        let thresholds = StalenessThresholds::default();
        let changes = detect_stats_changes(&prev, &current, &thresholds);

        assert!(!changes.is_empty());
        assert!(matches!(
            changes[0].change_type,
            ChangeType::Statistics
        ));
    }

    #[test]
    fn detect_cpu_change() {
        let prev = HardwareProfile {
            cpu_cores: 4,
            total_memory: 8_000_000_000,
            available_memory: 6_000_000_000,
            simd_width: 256,
            has_gpu: false,
            gpu_memory: None,
            l1_cache_size: 32768,
            l2_cache_size: 262144,
            l3_cache_size: 8388608,
        };

        let current = HardwareProfile {
            cpu_cores: 16,
            total_memory: 32_000_000_000,
            available_memory: 24_000_000_000,
            simd_width: 256,
            has_gpu: false,
            gpu_memory: None,
            l1_cache_size: 32768,
            l2_cache_size: 262144,
            l3_cache_size: 8388608,
        };

        let changes = detect_hardware_changes(&prev, &current);

        assert!(!changes.is_empty());
        assert!(changes.iter().any(|c| c.description.contains("CPU")));
        assert!(changes.iter().any(|c| c.description.contains("memory")));
    }

    #[test]
    fn detect_fact_change() {
        let prev = FactsSnapshot {
            supports_hash_join: Some(false),
            supports_parallel_scan: Some(false),
            parallel_workers: Some(1),
            work_mem_bytes: Some(64 * 1024 * 1024),
            custom: HashMap::new(),
        };

        let current = FactsSnapshot {
            supports_hash_join: Some(true),
            supports_parallel_scan: Some(true),
            parallel_workers: Some(8),
            work_mem_bytes: Some(512 * 1024 * 1024),
            custom: HashMap::new(),
        };

        let changes = detect_fact_changes(&prev, &current);

        assert!(changes.len() >= 2);
        assert!(changes.iter().any(|c| c.description.contains("Hash join")));
        assert!(changes.iter().any(|c| c.description.contains("Parallel")));
    }

    #[test]
    fn result_to_markdown() {
        let result = TimelineOptimizationResult {
            snapshot_results: vec![SnapshotResult {
                snapshot_index: 0,
                time_offset: 0,
                label: Some("Test".to_string()),
                optimized_plan: "Scan(users)".to_string(),
                cost: 100.0,
                optimization_time_ms: 50,
                rules_applied: vec![],
                dependencies: PlanDependencies {
                    table_cardinalities: HashMap::new(),
                    indexes: HashSet::new(),
                    distinct_counts: HashMap::new(),
                    histogram_digests: HashMap::new(),
                    facts: HashSet::new(),
                },
                changes_from_previous: vec![],
            }],
            timeline_config: create_test_config(),
        };

        let markdown = result.to_markdown();
        assert!(markdown.contains("# Timeline Optimization Report"));
        assert!(markdown.contains("Snapshot 0"));
    }

    #[test]
    fn result_to_text() {
        let result = TimelineOptimizationResult {
            snapshot_results: vec![SnapshotResult {
                snapshot_index: 0,
                time_offset: 0,
                label: None,
                optimized_plan: "Scan(users)".to_string(),
                cost: 100.0,
                optimization_time_ms: 50,
                rules_applied: vec![],
                dependencies: PlanDependencies {
                    table_cardinalities: HashMap::new(),
                    indexes: HashSet::new(),
                    distinct_counts: HashMap::new(),
                    histogram_digests: HashMap::new(),
                    facts: HashSet::new(),
                },
                changes_from_previous: vec![],
            }],
            timeline_config: create_test_config(),
        };

        let text = result.to_text();
        assert!(text.contains("Timeline Optimization Report"));
        assert!(text.contains("┌──────┬"));
    }
}
