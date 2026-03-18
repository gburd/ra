//! Statistics timeline format and playback engine.
//!
//! Provides a TOML-based timeline format for describing how database
//! statistics evolve over time, and a playback engine for stepping
//! through snapshots to drive adaptive query optimization demos.
//!
//! # Format overview
//!
//! A timeline file contains:
//! - **metadata**: description, database context, time range
//! - **snapshots**: ordered statistics snapshots with time offsets
//! - **events**: data modification events (inserts, deletes, analyze)
//! - **feedback**: execution feedback with estimated vs actual rows

use crate::accuracy::{StatisticsSource, StatisticsState};
use crate::integration::ManagedTableStats;
use crate::types::{ColumnStats, TableStats};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors from timeline parsing and playback.
#[derive(Debug, Error)]
pub enum TimelineError {
    /// TOML parsing failed.
    #[error("failed to parse timeline TOML: {0}")]
    ParseError(String),
    /// Snapshot index out of bounds.
    #[error("snapshot index {index} out of bounds (total: {total})")]
    SnapshotOutOfBounds {
        /// Requested index.
        index: usize,
        /// Total snapshot count.
        total: usize,
    },
    /// Timeline has no snapshots.
    #[error("timeline has no snapshots")]
    EmptyTimeline,
    /// Duplicate snapshot time offset.
    #[error("duplicate time offset: {0}")]
    DuplicateOffset(u64),
    /// Snapshot time offsets not in ascending order.
    #[error("snapshot offsets not in ascending order at index {0}")]
    UnsortedOffsets(usize),
    /// I/O error reading timeline file.
    #[error("I/O error: {0}")]
    IoError(String),
}

// -- TOML format types --

/// Top-level timeline document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    /// Timeline metadata.
    pub metadata: TimelineMetadata,
    /// Ordered statistics snapshots.
    pub snapshots: Vec<Snapshot>,
    /// Data modification events.
    #[serde(default)]
    pub events: Vec<TimelineEvent>,
    /// Execution feedback entries.
    #[serde(default)]
    pub feedback: Vec<ExecutionFeedback>,
}

/// Timeline metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineMetadata {
    /// Human-readable name.
    pub name: String,
    /// Description of the scenario.
    pub description: String,
    /// Target database system (e.g. "postgresql", "duckdb").
    #[serde(default)]
    pub database: Option<String>,
    /// Schema or benchmark name (e.g. "TPC-H", "TPC-DS").
    #[serde(default)]
    pub schema: Option<String>,
    /// Scale factor for the benchmark.
    #[serde(default)]
    pub scale_factor: Option<f64>,
    /// Total simulated duration in seconds.
    #[serde(default)]
    pub duration_seconds: Option<u64>,
}

/// A statistics snapshot at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Time offset in seconds from timeline start.
    pub time_offset: u64,
    /// Optional label for this snapshot.
    #[serde(default)]
    pub label: Option<String>,
    /// Per-table statistics at this point.
    pub tables: Vec<TableSnapshot>,
}

/// Statistics for a single table at a snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSnapshot {
    /// Table name.
    pub name: String,
    /// Total row count.
    pub row_count: u64,
    /// Total page count.
    #[serde(default)]
    pub page_count: Option<u64>,
    /// Average row size in bytes.
    #[serde(default)]
    pub avg_row_size: Option<f64>,
    /// Table size in bytes.
    #[serde(default)]
    pub table_size_bytes: Option<u64>,
    /// Per-column statistics.
    #[serde(default)]
    pub columns: Vec<ColumnSnapshot>,
}

/// Statistics for a single column at a snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnSnapshot {
    /// Column name.
    pub name: String,
    /// Number of distinct values.
    pub ndv: u64,
    /// NULL fraction (0.0 to 1.0).
    #[serde(default)]
    pub null_fraction: f64,
    /// Average width in bytes.
    #[serde(default = "default_avg_width")]
    pub avg_width: f64,
    /// Physical correlation (-1.0 to 1.0).
    #[serde(default)]
    pub correlation: Option<f64>,
    /// Minimum value (for display/documentation).
    #[serde(default)]
    pub min_value: Option<String>,
    /// Maximum value (for display/documentation).
    #[serde(default)]
    pub max_value: Option<String>,
}

fn default_avg_width() -> f64 {
    8.0
}

/// A data modification or system event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Time offset in seconds.
    pub time_offset: u64,
    /// Event kind.
    pub kind: EventKind,
    /// Affected table.
    pub table: String,
    /// Number of rows affected (for DML events).
    #[serde(default)]
    pub row_count: Option<u64>,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Kind of timeline event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Bulk insert.
    Insert,
    /// Bulk update.
    Update,
    /// Bulk delete.
    Delete,
    /// ANALYZE / statistics refresh.
    Analyze,
    /// Query optimizer triggered replanning.
    Reoptimize,
    /// Schema change (add column, add index, etc.).
    SchemaChange,
    /// Vacuum / compaction.
    Vacuum,
}

/// Execution feedback comparing estimated vs actual cardinalities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionFeedback {
    /// Time offset in seconds.
    pub time_offset: u64,
    /// Query identifier or template.
    pub query: String,
    /// Operator or plan node name.
    #[serde(default)]
    pub operator: Option<String>,
    /// Estimated row count from the optimizer.
    pub estimated_rows: f64,
    /// Actual row count from execution.
    pub actual_rows: f64,
    /// Estimated cost.
    #[serde(default)]
    pub estimated_cost: Option<f64>,
    /// Actual execution time in milliseconds.
    #[serde(default)]
    pub actual_time_ms: Option<f64>,
}

impl ExecutionFeedback {
    /// Q-error: max(estimated/actual, actual/estimated).
    /// Returns 1.0 for perfect estimates, higher for worse.
    pub fn q_error(&self) -> f64 {
        let est = self.estimated_rows.max(1.0);
        let act = self.actual_rows.max(1.0);
        (est / act).max(act / est)
    }

    /// Whether the estimate was an overestimate.
    pub fn is_overestimate(&self) -> bool {
        self.estimated_rows > self.actual_rows
    }

    /// Whether the estimate was an underestimate.
    pub fn is_underestimate(&self) -> bool {
        self.estimated_rows < self.actual_rows
    }
}

// -- Parsing --

impl Timeline {
    /// Parse a timeline from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns `TimelineError` if parsing fails or validation fails.
    pub fn from_toml(input: &str) -> Result<Self, TimelineError> {
        let timeline: Self = toml::from_str(input)
            .map_err(|e| TimelineError::ParseError(e.to_string()))?;
        timeline.validate()?;
        Ok(timeline)
    }

    /// Validate internal consistency.
    ///
    /// # Errors
    ///
    /// Returns `TimelineError` if snapshots are empty, unsorted, or
    /// contain duplicate time offsets.
    pub fn validate(&self) -> Result<(), TimelineError> {
        if self.snapshots.is_empty() {
            return Err(TimelineError::EmptyTimeline);
        }

        for i in 1..self.snapshots.len() {
            if self.snapshots[i].time_offset
                <= self.snapshots[i - 1].time_offset
            {
                return Err(TimelineError::UnsortedOffsets(i));
            }
        }

        let mut seen_offsets = std::collections::HashSet::new();
        for snap in &self.snapshots {
            if !seen_offsets.insert(snap.time_offset) {
                return Err(TimelineError::DuplicateOffset(
                    snap.time_offset,
                ));
            }
        }

        Ok(())
    }

    /// Total number of snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Total number of events.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Total number of feedback entries.
    pub fn feedback_count(&self) -> usize {
        self.feedback.len()
    }

    /// Duration from first to last snapshot.
    pub fn time_span(&self) -> u64 {
        if self.snapshots.len() < 2 {
            return 0;
        }
        self.snapshots.last().map_or(0, |l| l.time_offset)
            - self.snapshots.first().map_or(0, |f| f.time_offset)
    }

    /// All table names mentioned in any snapshot.
    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .snapshots
            .iter()
            .flat_map(|s| s.tables.iter().map(|t| t.name.clone()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        names.sort();
        names
    }

    /// Events within a time range (inclusive).
    pub fn events_in_range(
        &self,
        start: u64,
        end: u64,
    ) -> Vec<&TimelineEvent> {
        self.events
            .iter()
            .filter(|e| e.time_offset >= start && e.time_offset <= end)
            .collect()
    }

    /// Feedback entries within a time range (inclusive).
    pub fn feedback_in_range(
        &self,
        start: u64,
        end: u64,
    ) -> Vec<&ExecutionFeedback> {
        self.feedback
            .iter()
            .filter(|f| f.time_offset >= start && f.time_offset <= end)
            .collect()
    }
}

// -- Snapshot conversion --

impl Snapshot {
    /// Convert this snapshot into `ManagedTableStats` for each table.
    pub fn to_managed_stats(
        &self,
    ) -> HashMap<String, ManagedTableStats> {
        let mut result = HashMap::new();
        for table in &self.tables {
            let page_count = table.page_count.unwrap_or_else(|| {
                // Estimate: 8KB pages, using avg_row_size
                let avg = table.avg_row_size.unwrap_or(100.0);
                let rows_per_page = (8192.0 / avg).max(1.0) as u64;
                (table.row_count / rows_per_page).max(1)
            });

            let avg_row_size = table.avg_row_size.unwrap_or(100.0);

            let table_size = table.table_size_bytes.unwrap_or_else(|| {
                table.row_count * avg_row_size as u64
            });

            let table_stats = TableStats {
                row_count: table.row_count,
                page_count,
                average_row_size: avg_row_size,
                table_size_bytes: table_size,
                live_tuples: Some(table.row_count),
                dead_tuples: Some(0),
                last_analyzed: None,
            };

            let mut columns = HashMap::new();
            for col in &table.columns {
                columns.insert(
                    col.name.clone(),
                    ColumnStats {
                        column_id: col.name.clone(),
                        ndv: col.ndv,
                        null_fraction: col.null_fraction,
                        avg_width: col.avg_width,
                        mcv: None,
                        histogram: None,
                        correlation: col.correlation,
                    },
                );
            }

            let state = StatisticsState::new(
                StatisticsSource::ExactCount,
                table.row_count,
            );

            result.insert(
                table.name.clone(),
                ManagedTableStats {
                    table: table_stats,
                    columns,
                    state,
                },
            );
        }
        result
    }
}

// -- Playback engine --

/// State of timeline playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Positioned before the first snapshot.
    BeforeStart,
    /// Positioned at a specific snapshot index.
    AtSnapshot(usize),
    /// Past the last snapshot.
    AfterEnd,
}

/// Playback engine for stepping through a statistics timeline.
#[derive(Debug, Clone)]
pub struct TimelinePlayer {
    timeline: Timeline,
    position: PlaybackState,
}

impl TimelinePlayer {
    /// Create a new player positioned before the first snapshot.
    ///
    /// # Errors
    ///
    /// Returns `TimelineError::EmptyTimeline` if the timeline has no
    /// snapshots.
    pub fn new(timeline: Timeline) -> Result<Self, TimelineError> {
        if timeline.snapshots.is_empty() {
            return Err(TimelineError::EmptyTimeline);
        }
        Ok(Self {
            timeline,
            position: PlaybackState::BeforeStart,
        })
    }

    /// Current playback position.
    pub fn position(&self) -> PlaybackState {
        self.position
    }

    /// Current snapshot index, if positioned at one.
    pub fn current_index(&self) -> Option<usize> {
        match self.position {
            PlaybackState::AtSnapshot(i) => Some(i),
            _ => None,
        }
    }

    /// Reference to the underlying timeline.
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Total number of snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.timeline.snapshots.len()
    }

    /// Current time offset, if positioned at a snapshot.
    pub fn current_time(&self) -> Option<u64> {
        self.current_snapshot()
            .map(|s| s.time_offset)
    }

    /// Get the current snapshot, if positioned at one.
    pub fn current_snapshot(&self) -> Option<&Snapshot> {
        match self.position {
            PlaybackState::AtSnapshot(i) => {
                self.timeline.snapshots.get(i)
            }
            _ => None,
        }
    }

    /// Get managed stats for the current snapshot.
    pub fn current_managed_stats(
        &self,
    ) -> Option<HashMap<String, ManagedTableStats>> {
        self.current_snapshot().map(Snapshot::to_managed_stats)
    }

    /// Step to the next snapshot. Returns the new position.
    pub fn step_forward(&mut self) -> PlaybackState {
        self.position = match self.position {
            PlaybackState::BeforeStart => PlaybackState::AtSnapshot(0),
            PlaybackState::AtSnapshot(i) => {
                if i + 1 < self.timeline.snapshots.len() {
                    PlaybackState::AtSnapshot(i + 1)
                } else {
                    PlaybackState::AfterEnd
                }
            }
            PlaybackState::AfterEnd => PlaybackState::AfterEnd,
        };
        self.position
    }

    /// Step to the previous snapshot. Returns the new position.
    pub fn step_backward(&mut self) -> PlaybackState {
        self.position = match self.position {
            PlaybackState::BeforeStart
            | PlaybackState::AtSnapshot(0) => PlaybackState::BeforeStart,
            PlaybackState::AtSnapshot(i) => {
                PlaybackState::AtSnapshot(i - 1)
            }
            PlaybackState::AfterEnd => {
                let last = self.timeline.snapshots.len() - 1;
                PlaybackState::AtSnapshot(last)
            }
        };
        self.position
    }

    /// Seek to a specific snapshot index.
    ///
    /// # Errors
    ///
    /// Returns `TimelineError::SnapshotOutOfBounds` if index exceeds
    /// the number of snapshots.
    pub fn seek(
        &mut self,
        index: usize,
    ) -> Result<PlaybackState, TimelineError> {
        if index >= self.timeline.snapshots.len() {
            return Err(TimelineError::SnapshotOutOfBounds {
                index,
                total: self.timeline.snapshots.len(),
            });
        }
        self.position = PlaybackState::AtSnapshot(index);
        Ok(self.position)
    }

    /// Seek to the first snapshot.
    pub fn seek_start(&mut self) -> PlaybackState {
        self.position = PlaybackState::AtSnapshot(0);
        self.position
    }

    /// Seek to the last snapshot.
    pub fn seek_end(&mut self) -> PlaybackState {
        let last = self.timeline.snapshots.len() - 1;
        self.position = PlaybackState::AtSnapshot(last);
        self.position
    }

    /// Reset to before-start position.
    pub fn reset(&mut self) -> PlaybackState {
        self.position = PlaybackState::BeforeStart;
        self.position
    }

    /// Seek to the snapshot closest to a given time offset.
    pub fn seek_to_time(&mut self, time: u64) -> PlaybackState {
        let mut best_idx = 0;
        let mut best_diff = u64::MAX;

        for (i, snap) in self.timeline.snapshots.iter().enumerate() {
            let diff = snap.time_offset.abs_diff(time);
            if diff < best_diff {
                best_diff = diff;
                best_idx = i;
            }
        }

        self.position = PlaybackState::AtSnapshot(best_idx);
        self.position
    }

    /// Whether there are more snapshots ahead.
    pub fn has_next(&self) -> bool {
        match self.position {
            PlaybackState::BeforeStart => true,
            PlaybackState::AtSnapshot(i) => {
                i + 1 < self.timeline.snapshots.len()
            }
            PlaybackState::AfterEnd => false,
        }
    }

    /// Whether there are snapshots behind.
    pub fn has_previous(&self) -> bool {
        match self.position {
            PlaybackState::BeforeStart => false,
            PlaybackState::AtSnapshot(i) => i > 0,
            PlaybackState::AfterEnd => true,
        }
    }

    /// Events between current and next snapshot.
    pub fn events_until_next(&self) -> Vec<&TimelineEvent> {
        let (start, end) = match self.position {
            PlaybackState::AtSnapshot(i) => {
                let start = self.timeline.snapshots[i].time_offset;
                let end = self
                    .timeline
                    .snapshots
                    .get(i + 1)
                    .map_or(u64::MAX, |s| s.time_offset);
                (start, end)
            }
            _ => return Vec::new(),
        };
        self.timeline
            .events
            .iter()
            .filter(|e| e.time_offset >= start && e.time_offset < end)
            .collect()
    }

    /// Feedback entries at or before the current snapshot time.
    pub fn feedback_at_current(&self) -> Vec<&ExecutionFeedback> {
        let Some(time) = self.current_time() else {
            return Vec::new();
        };
        self.timeline
            .feedback
            .iter()
            .filter(|f| f.time_offset == time)
            .collect()
    }

    /// Collect all snapshot references from the timeline.
    pub fn all_snapshots(&self) -> &[Snapshot] {
        &self.timeline.snapshots
    }

    /// Compute the row count delta for a table between two snapshots.
    pub fn row_count_delta(
        &self,
        table: &str,
        from_idx: usize,
        to_idx: usize,
    ) -> Option<i64> {
        let from_snap = self.timeline.snapshots.get(from_idx)?;
        let to_snap = self.timeline.snapshots.get(to_idx)?;

        let from_rows = from_snap
            .tables
            .iter()
            .find(|t| t.name == table)?
            .row_count;
        let to_rows = to_snap
            .tables
            .iter()
            .find(|t| t.name == table)?
            .row_count;

        Some(to_rows as i64 - from_rows as i64)
    }

    /// Average Q-error across all feedback entries.
    pub fn average_q_error(&self) -> Option<f64> {
        if self.timeline.feedback.is_empty() {
            return None;
        }
        let sum: f64 = self
            .timeline
            .feedback
            .iter()
            .map(ExecutionFeedback::q_error)
            .sum();
        Some(sum / self.timeline.feedback.len() as f64)
    }

    /// Maximum Q-error across all feedback entries.
    pub fn max_q_error(&self) -> Option<f64> {
        self.timeline
            .feedback
            .iter()
            .map(ExecutionFeedback::q_error)
            .reduce(f64::max)
    }

    /// Compute the statistics delta between two snapshot indices.
    ///
    /// Returns `None` if either index is out of bounds.
    pub fn compute_delta(
        &self,
        from_idx: usize,
        to_idx: usize,
    ) -> Option<crate::delta::DeltaSet> {
        let from = self.timeline.snapshots.get(from_idx)?;
        let to = self.timeline.snapshots.get(to_idx)?;
        Some(crate::delta::DeltaSet::compute(from, to))
    }

    /// Compute the delta from the current snapshot to the next one.
    ///
    /// Returns `None` if not positioned at a snapshot, or if at the
    /// last snapshot.
    pub fn delta_to_next(&self) -> Option<crate::delta::DeltaSet> {
        let idx = self.current_index()?;
        self.compute_delta(idx, idx + 1)
    }

    /// Compute cumulative delta from a starting snapshot to the
    /// current position.
    ///
    /// Merges all consecutive deltas from `from_idx` to the current
    /// snapshot index. Returns `None` if not positioned at a snapshot
    /// or if `from_idx` >= current index.
    pub fn cumulative_delta(
        &self,
        from_idx: usize,
    ) -> Option<crate::delta::DeltaSet> {
        let current = self.current_index()?;
        if from_idx >= current {
            return None;
        }
        let mut merged = self.compute_delta(from_idx, from_idx + 1)?;
        for i in (from_idx + 1)..current {
            if let Some(next) = self.compute_delta(i, i + 1) {
                merged.merge(&next);
            }
        }
        Some(merged)
    }

    /// Find the most recent ANALYZE event index at or before the
    /// current position.
    ///
    /// Returns the snapshot index closest to (but not after) the
    /// ANALYZE event time, or `None` if no ANALYZE events exist
    /// before current position.
    pub fn last_analyze_snapshot_idx(&self) -> Option<usize> {
        let current_time = self.current_time()?;
        let mut best_idx = None;

        for event in &self.timeline.events {
            if event.kind == EventKind::Analyze
                && event.time_offset <= current_time
            {
                // Find the snapshot closest to this ANALYZE event.
                for (i, snap) in
                    self.timeline.snapshots.iter().enumerate()
                {
                    if snap.time_offset <= event.time_offset {
                        best_idx = Some(i);
                    }
                }
            }
        }

        best_idx
    }

    /// Whether a full reoptimization is needed based on the delta
    /// from the last ANALYZE event to the current position.
    ///
    /// Returns `true` if:
    /// - No ANALYZE events found (always full-optimize)
    /// - The cumulative delta since ANALYZE warrants full reopt
    pub fn needs_full_reoptimization(&self) -> bool {
        let Some(analyze_idx) = self.last_analyze_snapshot_idx() else {
            return true;
        };
        let Some(delta) = self.cumulative_delta(analyze_idx) else {
            return false;
        };
        delta.needs_full_reoptimization()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    // -- Helper to build minimal timelines --

    fn minimal_toml() -> &'static str {
        r#"
[metadata]
name = "test-timeline"
description = "A minimal test timeline"

[[snapshots]]
time_offset = 0
label = "initial"

[[snapshots.tables]]
name = "orders"
row_count = 1000

[[snapshots]]
time_offset = 60
label = "after inserts"

[[snapshots.tables]]
name = "orders"
row_count = 1500
"#
    }

    fn full_toml() -> &'static str {
        r#"
[metadata]
name = "tpch-q1-evolution"
description = "TPC-H Q1 lineitem statistics evolution"
database = "postgresql"
schema = "TPC-H"
scale_factor = 1.0
duration_seconds = 3600

[[snapshots]]
time_offset = 0
label = "initial load"

[[snapshots.tables]]
name = "lineitem"
row_count = 6001215
page_count = 80000
avg_row_size = 127.0
table_size_bytes = 762154305

[[snapshots.tables.columns]]
name = "l_orderkey"
ndv = 1500000
null_fraction = 0.0
avg_width = 8.0
correlation = 0.98

[[snapshots.tables.columns]]
name = "l_quantity"
ndv = 50
null_fraction = 0.0
avg_width = 8.0
min_value = "1"
max_value = "50"

[[snapshots]]
time_offset = 600
label = "after batch insert"

[[snapshots.tables]]
name = "lineitem"
row_count = 6501215
page_count = 86500
avg_row_size = 127.0
table_size_bytes = 825654305

[[snapshots.tables.columns]]
name = "l_orderkey"
ndv = 1625000
null_fraction = 0.0
avg_width = 8.0
correlation = 0.85

[[snapshots.tables.columns]]
name = "l_quantity"
ndv = 50
null_fraction = 0.0
avg_width = 8.0

[[snapshots]]
time_offset = 1200
label = "post-analyze"

[[snapshots.tables]]
name = "lineitem"
row_count = 6501215
page_count = 86500
avg_row_size = 127.0
table_size_bytes = 825654305

[[snapshots.tables.columns]]
name = "l_orderkey"
ndv = 1625304
null_fraction = 0.0
avg_width = 8.0
correlation = 0.85

[[snapshots.tables.columns]]
name = "l_quantity"
ndv = 50
null_fraction = 0.0
avg_width = 8.0

[[events]]
time_offset = 300
kind = "insert"
table = "lineitem"
row_count = 500000
description = "Batch load of new orders"

[[events]]
time_offset = 900
kind = "analyze"
table = "lineitem"
description = "Triggered ANALYZE"

[[events]]
time_offset = 1100
kind = "reoptimize"
table = "lineitem"
description = "Optimizer replanned Q1"

[[feedback]]
time_offset = 100
query = "SELECT l_returnflag, l_linestatus, sum(l_quantity) FROM lineitem WHERE l_shipdate <= '1998-09-02' GROUP BY l_returnflag, l_linestatus"
operator = "SeqScan on lineitem"
estimated_rows = 5916591.0
actual_rows = 5916591.0

[[feedback]]
time_offset = 700
query = "SELECT l_returnflag, l_linestatus, sum(l_quantity) FROM lineitem WHERE l_shipdate <= '1998-09-02' GROUP BY l_returnflag, l_linestatus"
operator = "SeqScan on lineitem"
estimated_rows = 5916591.0
actual_rows = 6408591.0
estimated_cost = 1500000.0
actual_time_ms = 2350.0

[[feedback]]
time_offset = 1200
query = "SELECT l_returnflag, l_linestatus, sum(l_quantity) FROM lineitem WHERE l_shipdate <= '1998-09-02' GROUP BY l_returnflag, l_linestatus"
operator = "SeqScan on lineitem"
estimated_rows = 6408000.0
actual_rows = 6408591.0
estimated_cost = 1625000.0
actual_time_ms = 2400.0
"#
    }

    fn empty_snapshots_toml() -> &'static str {
        r#"
[metadata]
name = "empty"
description = "No snapshots"
"#
    }

    fn unsorted_toml() -> &'static str {
        r#"
[metadata]
name = "unsorted"
description = "Out-of-order snapshots"

[[snapshots]]
time_offset = 100

[[snapshots.tables]]
name = "t"
row_count = 1

[[snapshots]]
time_offset = 50

[[snapshots.tables]]
name = "t"
row_count = 1
"#
    }

    // -- Parsing tests --

    #[test]
    fn parse_minimal() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse minimal");
        assert_eq!(tl.metadata.name, "test-timeline");
        assert_eq!(tl.snapshots.len(), 2);
    }

    #[test]
    fn parse_full() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse full");
        assert_eq!(tl.metadata.name, "tpch-q1-evolution");
        assert_eq!(tl.metadata.database, Some("postgresql".to_string()));
        assert_eq!(tl.metadata.schema, Some("TPC-H".to_string()));
        assert_eq!(tl.metadata.scale_factor, Some(1.0));
        assert_eq!(tl.metadata.duration_seconds, Some(3600));
    }

    #[test]
    fn parse_snapshot_count() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        assert_eq!(tl.snapshot_count(), 3);
    }

    #[test]
    fn parse_events() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        assert_eq!(tl.event_count(), 3);
        assert_eq!(tl.events[0].kind, EventKind::Insert);
        assert_eq!(tl.events[1].kind, EventKind::Analyze);
        assert_eq!(tl.events[2].kind, EventKind::Reoptimize);
    }

    #[test]
    fn parse_feedback() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        assert_eq!(tl.feedback_count(), 3);
    }

    #[test]
    fn parse_empty_snapshots_fails() {
        let err = Timeline::from_toml(empty_snapshots_toml());
        assert!(err.is_err());
    }

    #[test]
    fn validate_empty_snapshots_vec_fails() {
        let tl = Timeline {
            metadata: TimelineMetadata {
                name: "empty".to_string(),
                description: "empty vec".to_string(),
                database: None,
                schema: None,
                scale_factor: None,
                duration_seconds: None,
            },
            snapshots: vec![],
            events: vec![],
            feedback: vec![],
        };
        let err = tl.validate();
        assert!(err.is_err());
        let msg = format!("{}", err.err().expect("should be error"));
        assert!(msg.contains("no snapshots"));
    }

    #[test]
    fn parse_unsorted_offsets_fails() {
        let err = Timeline::from_toml(unsorted_toml());
        assert!(err.is_err());
        let msg = format!("{}", err.err().expect("should be error"));
        assert!(msg.contains("ascending order"));
    }

    #[test]
    fn parse_invalid_toml_fails() {
        let err = Timeline::from_toml("not valid toml {{{");
        assert!(err.is_err());
    }

    #[test]
    fn parse_snapshot_labels() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        assert_eq!(
            tl.snapshots[0].label,
            Some("initial".to_string())
        );
        assert_eq!(
            tl.snapshots[1].label,
            Some("after inserts".to_string())
        );
    }

    #[test]
    fn parse_column_stats() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let cols = &tl.snapshots[0].tables[0].columns;
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].name, "l_orderkey");
        assert_eq!(cols[0].ndv, 1_500_000);
        assert_eq!(cols[0].correlation, Some(0.98));
    }

    #[test]
    fn parse_column_min_max() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let col = &tl.snapshots[0].tables[0].columns[1];
        assert_eq!(col.min_value, Some("1".to_string()));
        assert_eq!(col.max_value, Some("50".to_string()));
    }

    #[test]
    fn parse_table_optional_fields_present() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let table = &tl.snapshots[0].tables[0];
        assert_eq!(table.page_count, Some(80_000));
        assert_eq!(table.avg_row_size, Some(127.0));
        assert_eq!(table.table_size_bytes, Some(762_154_305));
    }

    #[test]
    fn parse_table_optional_fields_absent() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let table = &tl.snapshots[0].tables[0];
        assert!(table.page_count.is_none());
        assert!(table.avg_row_size.is_none());
        assert!(table.table_size_bytes.is_none());
    }

    #[test]
    fn parse_event_row_count() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        assert_eq!(tl.events[0].row_count, Some(500_000));
    }

    #[test]
    fn parse_event_description() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        assert_eq!(
            tl.events[0].description,
            Some("Batch load of new orders".to_string())
        );
    }

    #[test]
    fn parse_feedback_cost_and_time() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let fb = &tl.feedback[1];
        assert_eq!(fb.estimated_cost, Some(1_500_000.0));
        assert_eq!(fb.actual_time_ms, Some(2350.0));
    }

    #[test]
    fn parse_feedback_operator() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        assert_eq!(
            tl.feedback[0].operator,
            Some("SeqScan on lineitem".to_string())
        );
    }

    // -- Timeline methods --

    #[test]
    fn time_span_full() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        assert_eq!(tl.time_span(), 1200);
    }

    #[test]
    fn time_span_minimal() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        assert_eq!(tl.time_span(), 60);
    }

    #[test]
    fn table_names() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let names = tl.table_names();
        assert_eq!(names, vec!["lineitem"]);
    }

    #[test]
    fn table_names_minimal() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let names = tl.table_names();
        assert_eq!(names, vec!["orders"]);
    }

    #[test]
    fn events_in_range_all() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let events = tl.events_in_range(0, 2000);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn events_in_range_partial() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let events = tl.events_in_range(200, 1000);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn events_in_range_empty() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let events = tl.events_in_range(1500, 2000);
        assert!(events.is_empty());
    }

    #[test]
    fn feedback_in_range_all() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let fb = tl.feedback_in_range(0, 2000);
        assert_eq!(fb.len(), 3);
    }

    #[test]
    fn feedback_in_range_partial() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let fb = tl.feedback_in_range(100, 700);
        assert_eq!(fb.len(), 2);
    }

    // -- Snapshot conversion --

    #[test]
    fn snapshot_to_managed_stats() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let stats = tl.snapshots[0].to_managed_stats();
        assert!(stats.contains_key("lineitem"));
        let li = &stats["lineitem"];
        assert_eq!(li.table.row_count, 6_001_215);
        assert_eq!(li.table.page_count, 80_000);
    }

    #[test]
    fn snapshot_column_stats_converted() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let stats = tl.snapshots[0].to_managed_stats();
        let li = &stats["lineitem"];
        assert!(li.columns.contains_key("l_orderkey"));
        assert_eq!(li.columns["l_orderkey"].ndv, 1_500_000);
    }

    #[test]
    fn snapshot_defaults_page_count() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let stats = tl.snapshots[0].to_managed_stats();
        let orders = &stats["orders"];
        assert!(orders.table.page_count > 0);
    }

    #[test]
    fn snapshot_defaults_table_size() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let stats = tl.snapshots[0].to_managed_stats();
        let orders = &stats["orders"];
        assert!(orders.table.table_size_bytes > 0);
    }

    // -- ExecutionFeedback --

    #[test]
    fn q_error_perfect() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!((fb.q_error() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_overestimate() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 2000.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!((fb.q_error() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_underestimate() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 500.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!((fb.q_error() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_zero_estimates_clamped() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 0.0,
            actual_rows: 0.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!((fb.q_error() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn is_overestimate() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 2000.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!(fb.is_overestimate());
        assert!(!fb.is_underestimate());
    }

    #[test]
    fn is_underestimate() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 500.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!(fb.is_underestimate());
        assert!(!fb.is_overestimate());
    }

    #[test]
    fn exact_is_neither_over_nor_under() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!(!fb.is_overestimate());
        assert!(!fb.is_underestimate());
    }

    // -- TimelinePlayer: creation --

    #[test]
    fn player_creation() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert_eq!(player.position(), PlaybackState::BeforeStart);
    }

    #[test]
    fn player_empty_timeline_fails() {
        let tl = Timeline {
            metadata: TimelineMetadata {
                name: "e".to_string(),
                description: "e".to_string(),
                database: None,
                schema: None,
                scale_factor: None,
                duration_seconds: None,
            },
            snapshots: vec![],
            events: vec![],
            feedback: vec![],
        };
        assert!(TimelinePlayer::new(tl).is_err());
    }

    #[test]
    fn player_snapshot_count() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert_eq!(player.snapshot_count(), 3);
    }

    // -- step_forward --

    #[test]
    fn step_forward_from_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        let pos = player.step_forward();
        assert_eq!(pos, PlaybackState::AtSnapshot(0));
    }

    #[test]
    fn step_forward_through_all() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        assert_eq!(player.step_forward(), PlaybackState::AtSnapshot(0));
        assert_eq!(player.step_forward(), PlaybackState::AtSnapshot(1));
        assert_eq!(player.step_forward(), PlaybackState::AfterEnd);
    }

    #[test]
    fn step_forward_at_end_stays() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        player.step_forward();
        player.step_forward();
        assert_eq!(player.step_forward(), PlaybackState::AfterEnd);
    }

    // -- step_backward --

    #[test]
    fn step_backward_from_after_end() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        player.step_forward();
        player.step_forward(); // AfterEnd
        let pos = player.step_backward();
        assert_eq!(pos, PlaybackState::AtSnapshot(1));
    }

    #[test]
    fn step_backward_to_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward(); // AtSnapshot(0)
        let pos = player.step_backward();
        assert_eq!(pos, PlaybackState::BeforeStart);
    }

    #[test]
    fn step_backward_at_before_start_stays() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        assert_eq!(
            player.step_backward(),
            PlaybackState::BeforeStart
        );
    }

    #[test]
    fn step_backward_middle() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward(); // 0
        player.step_forward(); // 1
        player.step_forward(); // 2
        assert_eq!(
            player.step_backward(),
            PlaybackState::AtSnapshot(1)
        );
    }

    // -- seek --

    #[test]
    fn seek_valid() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        let pos = player.seek(2).expect("seek");
        assert_eq!(pos, PlaybackState::AtSnapshot(2));
    }

    #[test]
    fn seek_out_of_bounds() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        assert!(player.seek(5).is_err());
    }

    #[test]
    fn seek_start() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek(2).expect("seek");
        let pos = player.seek_start();
        assert_eq!(pos, PlaybackState::AtSnapshot(0));
    }

    #[test]
    fn seek_end() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        let pos = player.seek_end();
        assert_eq!(pos, PlaybackState::AtSnapshot(2));
    }

    #[test]
    fn reset_position() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        let pos = player.reset();
        assert_eq!(pos, PlaybackState::BeforeStart);
    }

    // -- seek_to_time --

    #[test]
    fn seek_to_time_exact() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_to_time(600);
        assert_eq!(player.current_index(), Some(1));
    }

    #[test]
    fn seek_to_time_closest() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_to_time(500);
        assert_eq!(player.current_index(), Some(1));
    }

    #[test]
    fn seek_to_time_zero() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_to_time(0);
        assert_eq!(player.current_index(), Some(0));
    }

    #[test]
    fn seek_to_time_beyond_end() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_to_time(99999);
        assert_eq!(player.current_index(), Some(2));
    }

    // -- current_snapshot / current_time --

    #[test]
    fn current_snapshot_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.current_snapshot().is_none());
    }

    #[test]
    fn current_snapshot_at_position() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        let snap = player.current_snapshot().expect("snapshot");
        assert_eq!(snap.time_offset, 0);
    }

    #[test]
    fn current_time_at_position() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek(1).expect("seek");
        assert_eq!(player.current_time(), Some(600));
    }

    #[test]
    fn current_time_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.current_time().is_none());
    }

    #[test]
    fn current_index_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.current_index().is_none());
    }

    #[test]
    fn current_index_at_snapshot() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        assert_eq!(player.current_index(), Some(0));
    }

    // -- has_next / has_previous --

    #[test]
    fn has_next_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.has_next());
    }

    #[test]
    fn has_next_at_last() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_end();
        assert!(!player.has_next());
    }

    #[test]
    fn has_next_after_end() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        player.step_forward();
        player.step_forward();
        assert!(!player.has_next());
    }

    #[test]
    fn has_previous_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(!player.has_previous());
    }

    #[test]
    fn has_previous_at_first() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        assert!(!player.has_previous());
    }

    #[test]
    fn has_previous_at_second() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        player.step_forward();
        assert!(player.has_previous());
    }

    #[test]
    fn has_previous_after_end() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward();
        player.step_forward();
        player.step_forward();
        assert!(player.has_previous());
    }

    // -- events_until_next --

    #[test]
    fn events_until_next_at_first() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_start();
        let events = player.events_until_next();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::Insert);
    }

    #[test]
    fn events_until_next_at_second() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek(1).expect("seek");
        let events = player.events_until_next();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn events_until_next_at_last() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_end();
        let events = player.events_until_next();
        assert!(events.is_empty());
    }

    #[test]
    fn events_until_next_before_start() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let events = player.events_until_next();
        assert!(events.is_empty());
    }

    // -- feedback_at_current --

    #[test]
    fn feedback_at_current_with_match() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek(2).expect("seek"); // time_offset=1200
        let fb = player.feedback_at_current();
        assert_eq!(fb.len(), 1);
    }

    #[test]
    fn feedback_at_current_no_match() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek(1).expect("seek"); // time_offset=600
        let fb = player.feedback_at_current();
        assert!(fb.is_empty());
    }

    #[test]
    fn feedback_at_current_before_start() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let fb = player.feedback_at_current();
        assert!(fb.is_empty());
    }

    // -- current_managed_stats --

    #[test]
    fn current_managed_stats_at_snapshot() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek_start();
        let stats = player.current_managed_stats().expect("stats");
        assert!(stats.contains_key("lineitem"));
    }

    #[test]
    fn current_managed_stats_before_start() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.current_managed_stats().is_none());
    }

    // -- row_count_delta --

    #[test]
    fn row_count_delta_positive() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let delta = player.row_count_delta("lineitem", 0, 1);
        assert_eq!(delta, Some(500_000));
    }

    #[test]
    fn row_count_delta_zero() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let delta = player.row_count_delta("lineitem", 1, 2);
        assert_eq!(delta, Some(0));
    }

    #[test]
    fn row_count_delta_nonexistent_table() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.row_count_delta("nonexistent", 0, 1).is_none());
    }

    #[test]
    fn row_count_delta_out_of_bounds() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.row_count_delta("lineitem", 0, 99).is_none());
    }

    // -- average/max q_error --

    #[test]
    fn average_q_error_full() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let avg = player.average_q_error().expect("avg");
        assert!(avg >= 1.0);
    }

    #[test]
    fn max_q_error_full() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let max = player.max_q_error().expect("max");
        assert!(max >= 1.0);
    }

    #[test]
    fn average_q_error_no_feedback() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.average_q_error().is_none());
    }

    #[test]
    fn max_q_error_no_feedback() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert!(player.max_q_error().is_none());
    }

    // -- all_snapshots --

    #[test]
    fn all_snapshots_full() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let snaps = player.all_snapshots();
        assert_eq!(snaps.len(), 3);
        assert_eq!(snaps[0].time_offset, 0);
        assert_eq!(snaps[1].time_offset, 600);
        assert_eq!(snaps[2].time_offset, 1200);
    }

    #[test]
    fn all_snapshots_minimal() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        let snaps = player.all_snapshots();
        assert_eq!(snaps.len(), 2);
    }

    // -- Serialize roundtrip --

    #[test]
    fn timeline_serialize_roundtrip() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let serialized = toml::to_string(&tl).expect("serialize");
        let tl2 = Timeline::from_toml(&serialized)
            .expect("re-parse");
        assert_eq!(tl, tl2);
    }

    #[test]
    fn timeline_json_roundtrip() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let json = serde_json::to_string(&tl).expect("json");
        let tl2: Timeline =
            serde_json::from_str(&json).expect("from json");
        assert_eq!(tl, tl2);
    }

    // -- EventKind variants --

    #[test]
    fn event_kind_insert() {
        let input = r#""insert""#;
        let kind: EventKind =
            serde_json::from_str(input).expect("parse");
        assert_eq!(kind, EventKind::Insert);
    }

    #[test]
    fn event_kind_update() {
        let input = r#""update""#;
        let kind: EventKind =
            serde_json::from_str(input).expect("parse");
        assert_eq!(kind, EventKind::Update);
    }

    #[test]
    fn event_kind_delete() {
        let input = r#""delete""#;
        let kind: EventKind =
            serde_json::from_str(input).expect("parse");
        assert_eq!(kind, EventKind::Delete);
    }

    #[test]
    fn event_kind_analyze() {
        let input = r#""analyze""#;
        let kind: EventKind =
            serde_json::from_str(input).expect("parse");
        assert_eq!(kind, EventKind::Analyze);
    }

    #[test]
    fn event_kind_reoptimize() {
        let input = r#""reoptimize""#;
        let kind: EventKind =
            serde_json::from_str(input).expect("parse");
        assert_eq!(kind, EventKind::Reoptimize);
    }

    #[test]
    fn event_kind_schema_change() {
        let input = r#""schema_change""#;
        let kind: EventKind =
            serde_json::from_str(input).expect("parse");
        assert_eq!(kind, EventKind::SchemaChange);
    }

    #[test]
    fn event_kind_vacuum() {
        let input = r#""vacuum""#;
        let kind: EventKind =
            serde_json::from_str(input).expect("parse");
        assert_eq!(kind, EventKind::Vacuum);
    }

    // -- Edge cases --

    #[test]
    fn single_snapshot_timeline() {
        let toml = r#"
[metadata]
name = "single"
description = "One snapshot"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100
"#;
        let tl = Timeline::from_toml(toml).expect("parse");
        assert_eq!(tl.snapshot_count(), 1);
        assert_eq!(tl.time_span(), 0);
    }

    #[test]
    fn single_snapshot_player_nav() {
        let toml = r#"
[metadata]
name = "single"
description = "One snapshot"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100
"#;
        let tl = Timeline::from_toml(toml).expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        assert_eq!(player.step_forward(), PlaybackState::AtSnapshot(0));
        assert!(!player.has_next());
        assert!(!player.has_previous());
        assert_eq!(player.step_forward(), PlaybackState::AfterEnd);
    }

    #[test]
    fn many_tables_snapshot() {
        let toml = r#"
[metadata]
name = "multi"
description = "Multiple tables"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "users"
row_count = 1000

[[snapshots.tables]]
name = "orders"
row_count = 5000

[[snapshots.tables]]
name = "items"
row_count = 20000
"#;
        let tl = Timeline::from_toml(toml).expect("parse");
        let stats = tl.snapshots[0].to_managed_stats();
        assert_eq!(stats.len(), 3);
        assert!(stats.contains_key("users"));
        assert!(stats.contains_key("orders"));
        assert!(stats.contains_key("items"));
    }

    #[test]
    fn timeline_reference_accessor() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        let player = TimelinePlayer::new(tl).expect("create");
        assert_eq!(player.timeline().metadata.name, "test-timeline");
    }

    #[test]
    fn forward_backward_roundtrip() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.step_forward(); // 0
        player.step_forward(); // 1
        player.step_backward(); // 0
        assert_eq!(player.current_index(), Some(0));
    }

    #[test]
    fn seek_then_step() {
        let tl = Timeline::from_toml(full_toml())
            .expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("create");
        player.seek(1).expect("seek");
        player.step_forward(); // 2
        assert_eq!(player.current_index(), Some(2));
    }

    #[test]
    fn column_default_avg_width() {
        let toml = r#"
[metadata]
name = "col-defaults"
description = "Column with default width"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100

[[snapshots.tables.columns]]
name = "c"
ndv = 50
"#;
        let tl = Timeline::from_toml(toml).expect("parse");
        let col = &tl.snapshots[0].tables[0].columns[0];
        assert!((col.avg_width - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn column_null_fraction_default() {
        let toml = r#"
[metadata]
name = "null-default"
description = "Default null fraction"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100

[[snapshots.tables.columns]]
name = "c"
ndv = 50
"#;
        let tl = Timeline::from_toml(toml).expect("parse");
        let col = &tl.snapshots[0].tables[0].columns[0];
        assert!((col.null_fraction - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_large_overestimate() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 1_000_000.0,
            actual_rows: 100.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!((fb.q_error() - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn q_error_large_underestimate() {
        let fb = ExecutionFeedback {
            time_offset: 0,
            query: "q".to_string(),
            operator: None,
            estimated_rows: 1.0,
            actual_rows: 10_000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        assert!((fb.q_error() - 10_000.0).abs() < 1.0);
    }

    // -- Playback State equality --

    #[test]
    fn playback_state_equality() {
        assert_eq!(PlaybackState::BeforeStart, PlaybackState::BeforeStart);
        assert_eq!(PlaybackState::AfterEnd, PlaybackState::AfterEnd);
        assert_eq!(
            PlaybackState::AtSnapshot(1),
            PlaybackState::AtSnapshot(1)
        );
        assert_ne!(
            PlaybackState::AtSnapshot(0),
            PlaybackState::AtSnapshot(1)
        );
        assert_ne!(PlaybackState::BeforeStart, PlaybackState::AfterEnd);
    }

    // -- Metadata validation --

    #[test]
    fn metadata_optional_fields_default() {
        let tl = Timeline::from_toml(minimal_toml())
            .expect("parse");
        assert!(tl.metadata.database.is_none());
        assert!(tl.metadata.schema.is_none());
        assert!(tl.metadata.scale_factor.is_none());
        assert!(tl.metadata.duration_seconds.is_none());
    }

    // -- Error display --

    #[test]
    fn error_display_parse() {
        let err = TimelineError::ParseError("bad toml".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("bad toml"));
    }

    #[test]
    fn error_display_out_of_bounds() {
        let err = TimelineError::SnapshotOutOfBounds {
            index: 5,
            total: 3,
        };
        let msg = format!("{err}");
        assert!(msg.contains("5"));
        assert!(msg.contains("3"));
    }

    #[test]
    fn error_display_empty() {
        let err = TimelineError::EmptyTimeline;
        let msg = format!("{err}");
        assert!(msg.contains("no snapshots"));
    }

    #[test]
    fn error_display_duplicate() {
        let err = TimelineError::DuplicateOffset(100);
        let msg = format!("{err}");
        assert!(msg.contains("100"));
    }

    #[test]
    fn error_display_unsorted() {
        let err = TimelineError::UnsortedOffsets(2);
        let msg = format!("{err}");
        assert!(msg.contains("2"));
    }

    #[test]
    fn error_display_io() {
        let err = TimelineError::IoError("not found".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("not found"));
    }

    // -- Integration: parse example timeline files --

    fn load_example(name: &str) -> Timeline {
        let path = format!(
            "{}/timelines/{name}",
            env!("CARGO_MANIFEST_DIR")
                .strip_suffix("/crates/ra-stats")
                .unwrap_or(env!("CARGO_MANIFEST_DIR")),
        );
        let content =
            std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("failed to read {path}: {e}");
            });
        Timeline::from_toml(&content).unwrap_or_else(|e| {
            panic!("failed to parse {path}: {e}");
        })
    }

    #[test]
    fn example_tpch_q1_parses() {
        let tl = load_example("tpch-q1-evolution.toml");
        assert_eq!(tl.metadata.name, "tpch-q1-evolution");
        assert!(tl.snapshot_count() >= 3);
        assert!(!tl.events.is_empty());
        assert!(!tl.feedback.is_empty());
    }

    #[test]
    fn example_tpch_q1_player_walkthrough() {
        let tl = load_example("tpch-q1-evolution.toml");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_start();
        let stats = player.current_managed_stats().expect("stats");
        assert!(stats.contains_key("lineitem"));
        assert!(player.has_next());
    }

    #[test]
    fn example_streaming_inserts_parses() {
        let tl = load_example("streaming-inserts.toml");
        assert_eq!(tl.metadata.name, "streaming-inserts");
        assert!(tl.snapshot_count() >= 3);
        let names = tl.table_names();
        assert!(names.contains(&"events".to_string()));
    }

    #[test]
    fn example_streaming_row_count_grows() {
        let tl = load_example("streaming-inserts.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        let delta = player.row_count_delta("events", 0, 3);
        assert!(delta.is_some());
        assert!(delta.expect("delta") > 0);
    }

    #[test]
    fn example_bulk_update_skew_parses() {
        let tl = load_example("bulk-update-skew.toml");
        assert_eq!(tl.metadata.name, "bulk-update-skew");
        let names = tl.table_names();
        assert!(names.contains(&"orders".to_string()));
        assert!(names.contains(&"customers".to_string()));
    }

    #[test]
    fn example_bulk_update_q_error_improves() {
        let tl = load_example("bulk-update-skew.toml");
        let first = &tl.feedback[0];
        let last = tl.feedback.last().expect("last");
        assert!(last.q_error() <= first.q_error() + 0.5);
    }

    #[test]
    fn example_multi_table_join_parses() {
        let tl = load_example("multi-table-join.toml");
        assert_eq!(tl.metadata.name, "multi-table-join");
        let names = tl.table_names();
        assert!(names.contains(&"fact_sales".to_string()));
        assert!(names.contains(&"dim_product".to_string()));
        assert!(names.contains(&"dim_store".to_string()));
    }

    #[test]
    fn example_multi_table_events_and_feedback() {
        let tl = load_example("multi-table-join.toml");
        assert!(tl.event_count() >= 5);
        assert!(tl.feedback_count() >= 3);
    }

    #[test]
    fn example_analyze_feedback_loop_parses() {
        let tl = load_example("analyze-feedback-loop.toml");
        assert_eq!(tl.metadata.name, "analyze-feedback-loop");
        assert!(tl.snapshot_count() >= 4);
    }

    #[test]
    fn example_analyze_feedback_loop_q_error_cycle() {
        let tl = load_example("analyze-feedback-loop.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        let avg_q = player.average_q_error().expect("avg");
        assert!(avg_q >= 1.0);
        assert!(avg_q < 3.0);
    }

    #[test]
    fn example_delete_heavy_parses() {
        let tl = load_example("delete-heavy-workload.toml");
        assert_eq!(tl.metadata.name, "delete-heavy-workload");
        let names = tl.table_names();
        assert!(names.contains(&"messages".to_string()));
        assert!(names.contains(&"users".to_string()));
    }

    #[test]
    fn example_delete_heavy_row_count_decreases() {
        let tl = load_example("delete-heavy-workload.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        let delta = player.row_count_delta("messages", 0, 1);
        assert!(delta.is_some());
        assert!(delta.expect("delta") < 0);
    }

    // -- join-reordering-cascade.toml --

    #[test]
    fn example_join_reordering_cascade_parses() {
        let tl = load_example("join-reordering-cascade.toml");
        assert_eq!(tl.metadata.name, "join-reordering-cascade");
        assert_eq!(tl.snapshot_count(), 10);
        let names = tl.table_names();
        assert!(names.contains(&"fact_transactions".to_string()));
        assert!(names.contains(&"promotions".to_string()));
        assert!(names.contains(&"dim_products".to_string()));
        assert!(names.contains(&"dim_stores".to_string()));
        assert!(names.contains(&"dim_customers".to_string()));
    }

    #[test]
    fn example_join_reordering_cascade_promotions_lifecycle() {
        let tl = load_example("join-reordering-cascade.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        // Promotions grow from 50 to 500K then shrink to 5K
        let growth = player.row_count_delta("promotions", 0, 4);
        assert!(growth.expect("growth") > 400_000);
        let shrink = player.row_count_delta("promotions", 4, 7);
        assert!(shrink.expect("shrink") < -400_000);
    }

    #[test]
    fn example_join_reordering_cascade_feedback_stale_period() {
        let tl = load_example("join-reordering-cascade.toml");
        // Should have stale period where estimates diverge
        let stale_fb: Vec<_> = tl
            .feedback
            .iter()
            .filter(|f| f.q_error() > 1.5)
            .collect();
        assert!(
            !stale_fb.is_empty(),
            "join-reordering should have stale feedback"
        );
    }

    #[test]
    fn example_join_reordering_cascade_events() {
        let tl = load_example("join-reordering-cascade.toml");
        assert!(tl.event_count() >= 10);
        let reopt_count = tl
            .events
            .iter()
            .filter(|e| e.kind == EventKind::Reoptimize)
            .count();
        assert!(
            reopt_count >= 3,
            "should have multiple reoptimize events"
        );
    }

    // -- index-vs-seqscan.toml --

    #[test]
    fn example_index_vs_seqscan_parses() {
        let tl = load_example("index-vs-seqscan.toml");
        assert_eq!(tl.metadata.name, "index-vs-seqscan");
        assert_eq!(tl.snapshot_count(), 8);
        let names = tl.table_names();
        assert!(names.contains(&"http_requests".to_string()));
    }

    #[test]
    fn example_index_vs_seqscan_table_growth() {
        let tl = load_example("index-vs-seqscan.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        let delta = player.row_count_delta("http_requests", 0, 4);
        assert!(delta.expect("delta") > 4_000_000);
    }

    #[test]
    fn example_index_vs_seqscan_plan_transitions() {
        let tl = load_example("index-vs-seqscan.toml");
        // Verify feedback shows scan type transitions
        let has_index = tl.feedback.iter().any(|f| {
            f.operator
                .as_ref()
                .is_some_and(|o| o.contains("IndexScan"))
        });
        let has_seq = tl.feedback.iter().any(|f| {
            f.operator
                .as_ref()
                .is_some_and(|o| o.contains("SeqScan"))
        });
        assert!(has_index, "should have IndexScan feedback");
        assert!(has_seq, "should have SeqScan feedback");
    }

    #[test]
    fn example_index_vs_seqscan_accuracy_recovery() {
        let tl = load_example("index-vs-seqscan.toml");
        let last = tl.feedback.last().expect("last");
        assert!(
            last.q_error() < 1.1,
            "final feedback should be accurate"
        );
    }

    // -- aggregation-strategy-evolution.toml --

    #[test]
    fn example_aggregation_strategy_parses() {
        let tl =
            load_example("aggregation-strategy-evolution.toml");
        assert_eq!(
            tl.metadata.name,
            "aggregation-strategy-evolution"
        );
        assert_eq!(tl.snapshot_count(), 11);
        let names = tl.table_names();
        assert!(names.contains(&"sensor_readings".to_string()));
        assert!(names.contains(&"devices".to_string()));
    }

    #[test]
    fn example_aggregation_strategy_device_growth() {
        let tl =
            load_example("aggregation-strategy-evolution.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        // Snapshot 0 = 1K devices, snapshot 7 = 105K devices
        let delta = player.row_count_delta("devices", 0, 7);
        assert!(
            delta.expect("delta") > 99_000,
            "devices should grow from 1K to 100K+"
        );
    }

    #[test]
    fn example_aggregation_strategy_transitions() {
        let tl =
            load_example("aggregation-strategy-evolution.toml");
        // Should have HashAgg, GroupAgg, and 2-phase transitions
        let has_hash = tl.feedback.iter().any(|f| {
            f.operator
                .as_ref()
                .is_some_and(|o| o.contains("HashAggregate"))
        });
        let has_group = tl.feedback.iter().any(|f| {
            f.operator
                .as_ref()
                .is_some_and(|o| o.contains("GroupAggregate"))
        });
        assert!(has_hash, "should have HashAggregate feedback");
        assert!(has_group, "should have GroupAggregate feedback");
    }

    #[test]
    fn example_aggregation_archive_shrinks_table() {
        let tl =
            load_example("aggregation-strategy-evolution.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        // After archive (snapshot 7->8), readings drop
        let delta =
            player.row_count_delta("sensor_readings", 7, 8);
        assert!(delta.expect("delta") < -50_000_000);
    }

    // -- partition-pruning-effectiveness.toml --

    #[test]
    fn example_partition_pruning_parses() {
        let tl =
            load_example("partition-pruning-effectiveness.toml");
        assert_eq!(
            tl.metadata.name,
            "partition-pruning-effectiveness"
        );
        assert_eq!(tl.snapshot_count(), 8);
    }

    #[test]
    fn example_partition_pruning_has_partitions() {
        let tl =
            load_example("partition-pruning-effectiveness.toml");
        let names = tl.table_names();
        // Should have weekly sub-partitions after split
        assert!(names
            .iter()
            .any(|n| n.starts_with("event_log_2025_10")));
        assert!(names.contains(&"event_log_2025_11".to_string()));
    }

    #[test]
    fn example_partition_pruning_stale_period() {
        let tl =
            load_example("partition-pruning-effectiveness.toml");
        // Should have feedback where stale stats cause big errors
        let stale_fb: Vec<_> = tl
            .feedback
            .iter()
            .filter(|f| f.q_error() > 3.0)
            .collect();
        assert!(
            !stale_fb.is_empty(),
            "should have badly stale partition estimates"
        );
    }

    #[test]
    fn example_partition_pruning_recovery() {
        let tl =
            load_example("partition-pruning-effectiveness.toml");
        let last = tl.feedback.last().expect("last");
        assert!(
            last.q_error() < 1.1,
            "final feedback should be accurate after ANALYZE"
        );
    }

    #[test]
    fn example_partition_pruning_events() {
        let tl =
            load_example("partition-pruning-effectiveness.toml");
        assert!(tl.event_count() >= 10);
        let has_schema_change = tl
            .events
            .iter()
            .any(|e| e.kind == EventKind::SchemaChange);
        assert!(
            has_schema_change,
            "should have partition split schema change"
        );
    }

    #[test]
    fn all_examples_have_valid_event_kinds() {
        let files = [
            "tpch-q1-evolution.toml",
            "streaming-inserts.toml",
            "bulk-update-skew.toml",
            "multi-table-join.toml",
            "analyze-feedback-loop.toml",
            "delete-heavy-workload.toml",
            "bulk-load.toml",
            "mixed-workload.toml",
            "join-reordering-cascade.toml",
            "index-vs-seqscan.toml",
            "aggregation-strategy-evolution.toml",
            "partition-pruning-effectiveness.toml",
        ];
        for file in &files {
            let tl = load_example(file);
            for event in &tl.events {
                // All events should have a table name
                assert!(
                    !event.table.is_empty(),
                    "{file}: event has empty table"
                );
            }
        }
    }

    #[test]
    fn all_examples_feedback_q_error_at_least_one() {
        let files = [
            "tpch-q1-evolution.toml",
            "streaming-inserts.toml",
            "bulk-update-skew.toml",
            "multi-table-join.toml",
            "analyze-feedback-loop.toml",
            "delete-heavy-workload.toml",
            "bulk-load.toml",
            "mixed-workload.toml",
            "join-reordering-cascade.toml",
            "index-vs-seqscan.toml",
            "aggregation-strategy-evolution.toml",
            "partition-pruning-effectiveness.toml",
        ];
        for file in &files {
            let tl = load_example(file);
            for fb in &tl.feedback {
                assert!(
                    fb.q_error() >= 1.0,
                    "{file}: Q-error should be >= 1.0"
                );
            }
        }
    }

    #[test]
    fn all_examples_snapshots_sorted() {
        let files = [
            "tpch-q1-evolution.toml",
            "streaming-inserts.toml",
            "bulk-update-skew.toml",
            "multi-table-join.toml",
            "analyze-feedback-loop.toml",
            "delete-heavy-workload.toml",
            "bulk-load.toml",
            "mixed-workload.toml",
            "join-reordering-cascade.toml",
            "index-vs-seqscan.toml",
            "aggregation-strategy-evolution.toml",
            "partition-pruning-effectiveness.toml",
        ];
        for file in &files {
            let tl = load_example(file);
            for i in 1..tl.snapshots.len() {
                assert!(
                    tl.snapshots[i].time_offset
                        > tl.snapshots[i - 1].time_offset,
                    "{file}: snapshots not sorted at index {i}"
                );
            }
        }
    }

    // -- bulk-load.toml --

    #[test]
    fn example_bulk_load_parses() {
        let tl = load_example("bulk-load.toml");
        assert_eq!(tl.metadata.name, "bulk-load");
        assert!(tl.snapshot_count() >= 4);
        let names = tl.table_names();
        assert!(names.contains(&"inventory".to_string()));
    }

    #[test]
    fn example_bulk_load_row_count_grows() {
        let tl = load_example("bulk-load.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        let delta = player.row_count_delta("inventory", 0, 1);
        assert!(delta.is_some());
        assert!(delta.expect("delta") > 0);
    }

    #[test]
    fn example_bulk_load_has_staleness_period() {
        let tl = load_example("bulk-load.toml");
        // Should have feedback where estimate != actual (stale)
        let stale_fb: Vec<_> = tl
            .feedback
            .iter()
            .filter(|f| f.q_error() > 1.5)
            .collect();
        assert!(
            !stale_fb.is_empty(),
            "bulk-load should have stale feedback entries"
        );
    }

    #[test]
    fn example_bulk_load_recovery_after_analyze() {
        let tl = load_example("bulk-load.toml");
        // Last feedback should be close to accurate (post-ANALYZE)
        let last = tl.feedback.last().expect("feedback");
        assert!(
            last.q_error() < 1.5,
            "post-ANALYZE feedback should be accurate"
        );
    }

    #[test]
    fn example_bulk_load_events() {
        let tl = load_example("bulk-load.toml");
        assert!(tl.event_count() >= 5);
        let insert_count = tl
            .events
            .iter()
            .filter(|e| e.kind == EventKind::Insert)
            .count();
        assert!(insert_count >= 3);
    }

    #[test]
    fn example_bulk_load_player_walkthrough() {
        let tl = load_example("bulk-load.toml");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_start();
        let stats = player.current_managed_stats().expect("stats");
        assert!(stats.contains_key("inventory"));
        let inv = &stats["inventory"];
        assert_eq!(inv.table.row_count, 0);
    }

    // -- mixed-workload.toml --

    #[test]
    fn example_mixed_workload_parses() {
        let tl = load_example("mixed-workload.toml");
        assert_eq!(tl.metadata.name, "mixed-workload");
        assert!(tl.snapshot_count() >= 4);
    }

    #[test]
    fn example_mixed_workload_has_multiple_tables() {
        let tl = load_example("mixed-workload.toml");
        let names = tl.table_names();
        assert!(names.contains(&"users".to_string()));
        assert!(names.contains(&"orders".to_string()));
        assert!(names.contains(&"order_items".to_string()));
    }

    #[test]
    fn example_mixed_workload_events() {
        let tl = load_example("mixed-workload.toml");
        assert!(tl.event_count() >= 10);
        let has_insert = tl
            .events
            .iter()
            .any(|e| e.kind == EventKind::Insert);
        let has_update = tl
            .events
            .iter()
            .any(|e| e.kind == EventKind::Update);
        let has_delete = tl
            .events
            .iter()
            .any(|e| e.kind == EventKind::Delete);
        assert!(has_insert);
        assert!(has_update);
        assert!(has_delete);
    }

    #[test]
    fn example_mixed_workload_orders_grow() {
        let tl = load_example("mixed-workload.toml");
        let player = TimelinePlayer::new(tl).expect("player");
        let last = player.snapshot_count() - 1;
        let delta = player.row_count_delta("orders", 0, last);
        assert!(delta.is_some());
        assert!(delta.expect("delta") > 0);
    }

    #[test]
    fn example_mixed_workload_feedback() {
        let tl = load_example("mixed-workload.toml");
        assert!(tl.feedback_count() >= 3);
        let player = TimelinePlayer::new(tl).expect("player");
        let avg = player.average_q_error().expect("avg");
        assert!(avg >= 1.0);
    }

    #[test]
    fn example_mixed_workload_player_walkthrough() {
        let tl = load_example("mixed-workload.toml");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_start();
        let stats = player.current_managed_stats().expect("stats");
        assert_eq!(stats.len(), 3);
    }

    // ---- Delta integration tests ----

    #[test]
    fn compute_delta_basic() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let player = TimelinePlayer::new(tl).expect("player");
        let ds = player.compute_delta(0, 1).expect("delta");
        assert!(!ds.is_empty());
        assert_eq!(ds.from_time, 0);
        assert_eq!(ds.to_time, 60);
    }

    #[test]
    fn compute_delta_out_of_bounds() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let player = TimelinePlayer::new(tl).expect("player");
        assert!(player.compute_delta(0, 99).is_none());
        assert!(player.compute_delta(99, 0).is_none());
    }

    #[test]
    fn compute_delta_same_index() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let player = TimelinePlayer::new(tl).expect("player");
        let ds = player.compute_delta(0, 0).expect("delta");
        assert!(ds.is_empty());
    }

    #[test]
    fn compute_delta_row_count_change() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let player = TimelinePlayer::new(tl).expect("player");
        let ds = player.compute_delta(0, 1).expect("delta");
        assert_eq!(ds.len(), 1);
        assert!(ds.row_count_change_pct() > 0.0);
    }

    #[test]
    fn delta_to_next_at_first_snapshot() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_start();
        let ds = player.delta_to_next().expect("delta");
        assert!(!ds.is_empty());
    }

    #[test]
    fn delta_to_next_at_last_snapshot() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_end();
        let ds = player.delta_to_next();
        assert!(ds.is_none());
    }

    #[test]
    fn delta_to_next_before_start() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let player = TimelinePlayer::new(tl).expect("player");
        let ds = player.delta_to_next();
        assert!(ds.is_none());
    }

    #[test]
    fn cumulative_delta_two_snapshots() {
        let tl = Timeline::from_toml(full_toml()).expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_end();
        let ds = player.cumulative_delta(0).expect("delta");
        assert!(!ds.is_empty());
    }

    #[test]
    fn cumulative_delta_same_index_returns_none() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_start();
        let ds = player.cumulative_delta(0);
        assert!(ds.is_none());
    }

    #[test]
    fn cumulative_delta_before_start_returns_none() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let player = TimelinePlayer::new(tl).expect("player");
        let ds = player.cumulative_delta(0);
        assert!(ds.is_none());
    }

    #[test]
    fn needs_full_reopt_no_analyze_events() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_end();
        assert!(player.needs_full_reoptimization());
    }

    #[test]
    fn needs_full_reopt_before_start() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let player = TimelinePlayer::new(tl).expect("player");
        assert!(player.needs_full_reoptimization());
    }

    #[test]
    fn last_analyze_snapshot_idx_no_events() {
        let tl = Timeline::from_toml(minimal_toml()).expect("parse");
        let mut player = TimelinePlayer::new(tl).expect("player");
        player.seek_end();
        assert!(player.last_analyze_snapshot_idx().is_none());
    }
}
