//! Statistics delta computation for incremental plan reoptimization.
//!
//! When statistics change between timeline snapshots, this module
//! computes the minimal set of changes (deltas) that describe what
//! shifted. The optimizer can use these deltas to decide whether
//! incremental reoptimization suffices or a full re-plan is required.

use crate::accuracy::Staleness;
use crate::timeline::{ColumnSnapshot, Snapshot, TableSnapshot};
use serde::{Deserialize, Serialize};

/// A single statistics change between two snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatisticsDelta {
    /// Table row count changed.
    TableRowCount {
        /// Table name.
        table: String,
        /// Previous row count.
        old: u64,
        /// New row count.
        new: u64,
    },
    /// Column distinct value count changed.
    ColumnNDV {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
        /// Previous NDV.
        old: u64,
        /// New NDV.
        new: u64,
    },
    /// Column null fraction changed.
    ColumnNullFraction {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
        /// Previous null fraction.
        old: f64,
        /// New null fraction.
        new: f64,
    },
    /// Column correlation changed.
    ColumnCorrelation {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
        /// Previous correlation.
        old: Option<f64>,
        /// New correlation.
        new: Option<f64>,
    },
    /// Table was added (present in new snapshot but not old).
    TableAdded {
        /// Table name.
        table: String,
        /// Row count in new snapshot.
        row_count: u64,
    },
    /// Table was removed (present in old snapshot but not new).
    TableRemoved {
        /// Table name.
        table: String,
        /// Row count in old snapshot.
        row_count: u64,
    },
    /// Staleness level changed for a table.
    StalenessChanged {
        /// Table name.
        table: String,
        /// Previous staleness.
        old: Staleness,
        /// New staleness.
        new: Staleness,
    },
}

impl StatisticsDelta {
    /// Table name affected by this delta.
    pub fn table(&self) -> &str {
        match self {
            Self::TableRowCount { table, .. }
            | Self::ColumnNDV { table, .. }
            | Self::ColumnNullFraction { table, .. }
            | Self::ColumnCorrelation { table, .. }
            | Self::TableAdded { table, .. }
            | Self::TableRemoved { table, .. }
            | Self::StalenessChanged { table, .. } => table,
        }
    }

    /// Magnitude of this change as a fraction of the old value.
    ///
    /// Returns 0.0 for no change, 1.0 for 100% change, and `f64::INFINITY`
    /// for additions/removals or zero-to-nonzero transitions.
    pub fn magnitude(&self) -> f64 {
        match self {
            Self::TableRowCount { old, new, .. }
            | Self::ColumnNDV { old, new, .. } => {
                relative_change(*old as f64, *new as f64)
            }
            Self::ColumnNullFraction { old, new, .. } => {
                (*new - *old).abs()
            }
            Self::ColumnCorrelation { old, new, .. } => {
                match (old, new) {
                    (Some(o), Some(n)) => (n - o).abs(),
                    (None, None) => 0.0,
                    _ => f64::INFINITY,
                }
            }
            Self::TableAdded { .. }
            | Self::TableRemoved { .. }
            | Self::StalenessChanged { .. } => f64::INFINITY,
        }
    }

    /// Whether this delta represents a structural change (table
    /// added/removed) vs a numeric change.
    pub fn is_structural(&self) -> bool {
        matches!(
            self,
            Self::TableAdded { .. }
            | Self::TableRemoved { .. }
        )
    }
}

/// Relative change between two values: |new - old| / max(old, 1).
fn relative_change(old: f64, new: f64) -> f64 {
    let base = old.max(1.0);
    (new - old).abs() / base
}

/// A batch of statistics deltas between two snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaSet {
    /// Individual deltas.
    deltas: Vec<StatisticsDelta>,
    /// Time offset of the source (old) snapshot.
    pub from_time: u64,
    /// Time offset of the target (new) snapshot.
    pub to_time: u64,
}

impl DeltaSet {
    /// Create a new empty delta set.
    pub fn new(from_time: u64, to_time: u64) -> Self {
        Self {
            deltas: Vec::new(),
            from_time,
            to_time,
        }
    }

    /// Compute deltas between two snapshots.
    pub fn compute(prev: &Snapshot, next: &Snapshot) -> Self {
        let mut deltas = Vec::new();
        let prev_tables = table_map(&prev.tables);
        let next_tables = table_map(&next.tables);

        // Tables in both snapshots: compare stats.
        for (&name, prev_table) in &prev_tables {
            if let Some(next_table) = next_tables.get(name) {
                diff_table(&mut deltas, name, prev_table, next_table);
            } else {
                deltas.push(StatisticsDelta::TableRemoved {
                    table: name.to_string(),
                    row_count: prev_table.row_count,
                });
            }
        }

        // Tables only in next snapshot.
        for (&name, next_table) in &next_tables {
            if !prev_tables.contains_key(name) {
                deltas.push(StatisticsDelta::TableAdded {
                    table: name.to_string(),
                    row_count: next_table.row_count,
                });
            }
        }

        Self {
            deltas,
            from_time: prev.time_offset,
            to_time: next.time_offset,
        }
    }

    /// Number of individual deltas.
    pub fn len(&self) -> usize {
        self.deltas.len()
    }

    /// Whether there are no deltas.
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty()
    }

    /// Iterator over deltas.
    pub fn iter(&self) -> std::slice::Iter<'_, StatisticsDelta> {
        self.deltas.iter()
    }

    /// Reference to the underlying delta slice.
    pub fn deltas(&self) -> &[StatisticsDelta] {
        &self.deltas
    }

    /// Push a delta into the set.
    pub fn push(&mut self, delta: StatisticsDelta) {
        self.deltas.push(delta);
    }

    /// Maximum magnitude across all deltas.
    pub fn max_magnitude(&self) -> f64 {
        self.deltas
            .iter()
            .map(StatisticsDelta::magnitude)
            .fold(0.0_f64, f64::max)
    }

    /// Sum of magnitudes (weighted change score).
    pub fn total_magnitude(&self) -> f64 {
        self.deltas
            .iter()
            .map(StatisticsDelta::magnitude)
            .filter(|m| m.is_finite())
            .sum()
    }

    /// Whether any delta is structural (table added/removed).
    pub fn has_structural_changes(&self) -> bool {
        self.deltas.iter().any(StatisticsDelta::is_structural)
    }

    /// Tables affected by any delta.
    pub fn affected_tables(&self) -> Vec<String> {
        let mut tables: Vec<String> = self
            .deltas
            .iter()
            .map(|d| d.table().to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        tables.sort();
        tables
    }

    /// Percentage of row count change across all tables.
    ///
    /// Uses the maximum row count change percentage among all
    /// tables, which is the most relevant metric for deciding
    /// between full and incremental reoptimization.
    pub fn row_count_change_pct(&self) -> f64 {
        self.deltas
            .iter()
            .filter_map(|d| {
                if let StatisticsDelta::TableRowCount { old, new, .. } = d {
                    Some(relative_change(*old as f64, *new as f64) * 100.0)
                } else {
                    None
                }
            })
            .fold(0.0_f64, f64::max)
    }

    /// Whether full reoptimization is recommended instead of
    /// incremental.
    ///
    /// Returns true when:
    /// - There are structural changes (tables added/removed)
    /// - Row count changed by more than 50%
    /// - More than 10 individual deltas (many small changes)
    pub fn needs_full_reoptimization(&self) -> bool {
        if self.has_structural_changes() {
            return true;
        }
        if self.row_count_change_pct() > 50.0 {
            return true;
        }
        self.deltas.len() > 10
    }

    /// Merge another delta set into this one.
    pub fn merge(&mut self, other: &Self) {
        self.deltas.extend(other.deltas.iter().cloned());
        self.to_time = self.to_time.max(other.to_time);
    }
}

impl<'a> IntoIterator for &'a DeltaSet {
    type Item = &'a StatisticsDelta;
    type IntoIter = std::slice::Iter<'a, StatisticsDelta>;

    fn into_iter(self) -> Self::IntoIter {
        self.deltas.iter()
    }
}

/// Build a name -> table snapshot map.
fn table_map(
    tables: &[TableSnapshot],
) -> std::collections::HashMap<&str, &TableSnapshot> {
    let mut map = std::collections::HashMap::new();
    for t in tables {
        map.insert(t.name.as_str(), t);
    }
    map
}

/// Compute deltas between two snapshots of the same table.
fn diff_table(
    deltas: &mut Vec<StatisticsDelta>,
    table: &str,
    prev: &TableSnapshot,
    next: &TableSnapshot,
) {
    if prev.row_count != next.row_count {
        deltas.push(StatisticsDelta::TableRowCount {
            table: table.to_string(),
            old: prev.row_count,
            new: next.row_count,
        });
    }

    let prev_cols = column_map(&prev.columns);
    let next_cols = column_map(&next.columns);

    for (name, prev_col) in &prev_cols {
        if let Some(next_col) = next_cols.get(name) {
            diff_column(deltas, table, name, prev_col, next_col);
        }
    }
}

/// Build a name -> column snapshot map.
fn column_map(
    columns: &[ColumnSnapshot],
) -> std::collections::HashMap<&str, &ColumnSnapshot> {
    let mut map = std::collections::HashMap::new();
    for c in columns {
        map.insert(c.name.as_str(), c);
    }
    map
}

/// Compute deltas between two snapshots of the same column.
fn diff_column(
    deltas: &mut Vec<StatisticsDelta>,
    table: &str,
    column: &str,
    prev: &ColumnSnapshot,
    next: &ColumnSnapshot,
) {
    if prev.ndv != next.ndv {
        deltas.push(StatisticsDelta::ColumnNDV {
            table: table.to_string(),
            column: column.to_string(),
            old: prev.ndv,
            new: next.ndv,
        });
    }

    #[allow(clippy::float_cmp)]
    if prev.null_fraction != next.null_fraction {
        deltas.push(StatisticsDelta::ColumnNullFraction {
            table: table.to_string(),
            column: column.to_string(),
            old: prev.null_fraction,
            new: next.null_fraction,
        });
    }

    if prev.correlation != next.correlation {
        deltas.push(StatisticsDelta::ColumnCorrelation {
            table: table.to_string(),
            column: column.to_string(),
            old: prev.correlation,
            new: next.correlation,
        });
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn snap_simple(time: u64, row_count: u64) -> Snapshot {
        Snapshot {
            time_offset: time,
            label: None,
            tables: vec![TableSnapshot {
                name: "orders".to_string(),
                row_count,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![],
            }],
        }
    }

    fn snap_with_columns(
        time: u64,
        row_count: u64,
        ndv: u64,
        null_frac: f64,
        correlation: Option<f64>,
    ) -> Snapshot {
        Snapshot {
            time_offset: time,
            label: None,
            tables: vec![TableSnapshot {
                name: "orders".to_string(),
                row_count,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![ColumnSnapshot {
                    name: "id".to_string(),
                    ndv,
                    null_fraction: null_frac,
                    avg_width: 8.0,
                    correlation,
                    min_value: None,
                    max_value: None,
                }],
            }],
        }
    }

    fn snap_two_tables(
        time: u64,
        t1_rows: u64,
        t2_rows: u64,
    ) -> Snapshot {
        Snapshot {
            time_offset: time,
            label: None,
            tables: vec![
                TableSnapshot {
                    name: "orders".to_string(),
                    row_count: t1_rows,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
                TableSnapshot {
                    name: "items".to_string(),
                    row_count: t2_rows,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
            ],
        }
    }

    // -- DeltaSet::compute --

    #[test]
    fn no_change_produces_empty_delta() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1000);
        let ds = DeltaSet::compute(&a, &b);
        assert!(ds.is_empty());
        assert_eq!(ds.from_time, 0);
        assert_eq!(ds.to_time, 60);
    }

    #[test]
    fn row_count_increase() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1500);
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
        let d = &ds.deltas()[0];
        assert!(matches!(d,
            StatisticsDelta::TableRowCount { old: 1000, new: 1500, .. }
        ));
    }

    #[test]
    fn row_count_decrease() {
        let a = snap_simple(0, 2000);
        let b = snap_simple(60, 1000);
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
        assert!(matches!(&ds.deltas()[0],
            StatisticsDelta::TableRowCount { old: 2000, new: 1000, .. }
        ));
    }

    #[test]
    fn ndv_change_detected() {
        let a = snap_with_columns(0, 1000, 500, 0.0, None);
        let b = snap_with_columns(60, 1000, 600, 0.0, None);
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
        assert!(matches!(&ds.deltas()[0],
            StatisticsDelta::ColumnNDV { old: 500, new: 600, .. }
        ));
    }

    #[test]
    fn null_fraction_change_detected() {
        let a = snap_with_columns(0, 1000, 500, 0.0, None);
        let b = snap_with_columns(60, 1000, 500, 0.05, None);
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
        assert!(matches!(&ds.deltas()[0],
            StatisticsDelta::ColumnNullFraction { .. }
        ));
    }

    #[test]
    fn correlation_change_detected() {
        let a = snap_with_columns(0, 1000, 500, 0.0, Some(0.98));
        let b = snap_with_columns(60, 1000, 500, 0.0, Some(0.85));
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
        assert!(matches!(&ds.deltas()[0],
            StatisticsDelta::ColumnCorrelation { .. }
        ));
    }

    #[test]
    fn correlation_none_to_some() {
        let a = snap_with_columns(0, 1000, 500, 0.0, None);
        let b = snap_with_columns(60, 1000, 500, 0.0, Some(0.5));
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
    }

    #[test]
    fn multiple_changes_in_one_delta() {
        let a = snap_with_columns(0, 1000, 500, 0.0, Some(0.98));
        let b = snap_with_columns(60, 1500, 700, 0.02, Some(0.85));
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 4); // row_count, ndv, null_fraction, correlation
    }

    #[test]
    fn table_added() {
        let a = snap_simple(0, 1000);
        let b = snap_two_tables(60, 1000, 5000);
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
        assert!(matches!(&ds.deltas()[0],
            StatisticsDelta::TableAdded { table, row_count: 5000 }
            if table == "items"
        ));
    }

    #[test]
    fn table_removed() {
        let a = snap_two_tables(0, 1000, 5000);
        let b = snap_simple(60, 1000);
        let ds = DeltaSet::compute(&a, &b);
        assert_eq!(ds.len(), 1);
        assert!(matches!(&ds.deltas()[0],
            StatisticsDelta::TableRemoved { table, row_count: 5000 }
            if table == "items"
        ));
    }

    // -- Magnitude --

    #[test]
    fn magnitude_row_count_50_pct() {
        let d = StatisticsDelta::TableRowCount {
            table: "t".to_string(),
            old: 1000,
            new: 1500,
        };
        assert!((d.magnitude() - 0.5).abs() < 0.001);
    }

    #[test]
    fn magnitude_row_count_zero_old() {
        let d = StatisticsDelta::TableRowCount {
            table: "t".to_string(),
            old: 0,
            new: 100,
        };
        assert!((d.magnitude() - 100.0).abs() < 0.001);
    }

    #[test]
    fn magnitude_ndv_change() {
        let d = StatisticsDelta::ColumnNDV {
            table: "t".to_string(),
            column: "c".to_string(),
            old: 100,
            new: 120,
        };
        assert!((d.magnitude() - 0.2).abs() < 0.001);
    }

    #[test]
    fn magnitude_null_fraction_absolute() {
        let d = StatisticsDelta::ColumnNullFraction {
            table: "t".to_string(),
            column: "c".to_string(),
            old: 0.0,
            new: 0.05,
        };
        assert!((d.magnitude() - 0.05).abs() < 0.001);
    }

    #[test]
    fn magnitude_correlation_both_some() {
        let d = StatisticsDelta::ColumnCorrelation {
            table: "t".to_string(),
            column: "c".to_string(),
            old: Some(0.9),
            new: Some(0.7),
        };
        assert!((d.magnitude() - 0.2).abs() < 0.001);
    }

    #[test]
    fn magnitude_correlation_none_none() {
        let d = StatisticsDelta::ColumnCorrelation {
            table: "t".to_string(),
            column: "c".to_string(),
            old: None,
            new: None,
        };
        assert!((d.magnitude()).abs() < f64::EPSILON);
    }

    #[test]
    fn magnitude_correlation_mixed_infinite() {
        let d = StatisticsDelta::ColumnCorrelation {
            table: "t".to_string(),
            column: "c".to_string(),
            old: None,
            new: Some(0.5),
        };
        assert!(d.magnitude().is_infinite());
    }

    #[test]
    fn magnitude_table_added_infinite() {
        let d = StatisticsDelta::TableAdded {
            table: "t".to_string(),
            row_count: 1000,
        };
        assert!(d.magnitude().is_infinite());
    }

    #[test]
    fn magnitude_table_removed_infinite() {
        let d = StatisticsDelta::TableRemoved {
            table: "t".to_string(),
            row_count: 1000,
        };
        assert!(d.magnitude().is_infinite());
    }

    #[test]
    fn magnitude_staleness_changed_infinite() {
        let d = StatisticsDelta::StalenessChanged {
            table: "t".to_string(),
            old: Staleness::Fresh,
            new: Staleness::VeryStale,
        };
        assert!(d.magnitude().is_infinite());
    }

    // -- is_structural --

    #[test]
    fn structural_table_added() {
        let d = StatisticsDelta::TableAdded {
            table: "t".to_string(),
            row_count: 1,
        };
        assert!(d.is_structural());
    }

    #[test]
    fn structural_row_count_not_structural() {
        let d = StatisticsDelta::TableRowCount {
            table: "t".to_string(),
            old: 100,
            new: 200,
        };
        assert!(!d.is_structural());
    }

    // -- DeltaSet methods --

    #[test]
    fn delta_set_new_empty() {
        let ds = DeltaSet::new(0, 60);
        assert!(ds.is_empty());
        assert_eq!(ds.len(), 0);
        assert_eq!(ds.from_time, 0);
        assert_eq!(ds.to_time, 60);
    }

    #[test]
    fn delta_set_push() {
        let mut ds = DeltaSet::new(0, 60);
        ds.push(StatisticsDelta::TableRowCount {
            table: "t".to_string(),
            old: 100,
            new: 200,
        });
        assert_eq!(ds.len(), 1);
    }

    #[test]
    fn delta_set_max_magnitude() {
        let a = snap_with_columns(0, 1000, 500, 0.0, Some(0.98));
        let b = snap_with_columns(60, 1500, 700, 0.02, Some(0.85));
        let ds = DeltaSet::compute(&a, &b);
        assert!(ds.max_magnitude() >= 0.4); // NDV changed by 40%
    }

    #[test]
    fn delta_set_total_magnitude() {
        let a = snap_with_columns(0, 1000, 500, 0.0, None);
        let b = snap_with_columns(60, 1500, 600, 0.0, None);
        let ds = DeltaSet::compute(&a, &b);
        // row_count 50% + ndv 20% = 0.7
        assert!(ds.total_magnitude() > 0.5);
    }

    #[test]
    fn delta_set_affected_tables() {
        let a = snap_two_tables(0, 1000, 5000);
        let b = snap_two_tables(60, 1200, 5500);
        let ds = DeltaSet::compute(&a, &b);
        let tables = ds.affected_tables();
        assert_eq!(tables.len(), 2);
        assert!(tables.contains(&"items".to_string()));
        assert!(tables.contains(&"orders".to_string()));
    }

    #[test]
    fn delta_set_row_count_change_pct() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1100);
        let ds = DeltaSet::compute(&a, &b);
        assert!((ds.row_count_change_pct() - 10.0).abs() < 0.1);
    }

    #[test]
    fn delta_set_row_count_change_pct_no_change() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1000);
        let ds = DeltaSet::compute(&a, &b);
        assert!((ds.row_count_change_pct()).abs() < f64::EPSILON);
    }

    // -- needs_full_reoptimization --

    #[test]
    fn small_change_incremental_ok() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1010);
        let ds = DeltaSet::compute(&a, &b);
        assert!(!ds.needs_full_reoptimization());
    }

    #[test]
    fn large_row_change_needs_full() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 2000);
        let ds = DeltaSet::compute(&a, &b);
        assert!(ds.needs_full_reoptimization());
    }

    #[test]
    fn structural_change_needs_full() {
        let a = snap_simple(0, 1000);
        let b = snap_two_tables(60, 1000, 5000);
        let ds = DeltaSet::compute(&a, &b);
        assert!(ds.needs_full_reoptimization());
    }

    // -- has_structural_changes --

    #[test]
    fn no_structural_changes() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1500);
        let ds = DeltaSet::compute(&a, &b);
        assert!(!ds.has_structural_changes());
    }

    #[test]
    fn has_structural_with_table_added() {
        let a = snap_simple(0, 1000);
        let b = snap_two_tables(60, 1000, 5000);
        let ds = DeltaSet::compute(&a, &b);
        assert!(ds.has_structural_changes());
    }

    // -- merge --

    #[test]
    fn merge_delta_sets() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1500);
        let c = snap_simple(120, 2000);
        let mut ds1 = DeltaSet::compute(&a, &b);
        let ds2 = DeltaSet::compute(&b, &c);
        ds1.merge(&ds2);
        assert_eq!(ds1.len(), 2);
        assert_eq!(ds1.to_time, 120);
    }

    // -- iterator --

    #[test]
    fn iterate_deltas() {
        let a = snap_with_columns(0, 1000, 500, 0.0, None);
        let b = snap_with_columns(60, 1500, 600, 0.0, None);
        let ds = DeltaSet::compute(&a, &b);
        let count = ds.iter().count();
        assert_eq!(count, ds.len());
    }

    #[test]
    fn into_iter_for_ref() {
        let a = snap_simple(0, 1000);
        let b = snap_simple(60, 1500);
        let ds = DeltaSet::compute(&a, &b);
        let mut count = 0;
        for _delta in &ds {
            count += 1;
        }
        assert_eq!(count, 1);
    }

    // -- table accessor --

    #[test]
    fn delta_table_name() {
        let d = StatisticsDelta::TableRowCount {
            table: "orders".to_string(),
            old: 100,
            new: 200,
        };
        assert_eq!(d.table(), "orders");
    }

    // -- serialization roundtrip --

    #[test]
    fn delta_set_json_roundtrip() {
        let a = snap_with_columns(0, 1000, 500, 0.0, Some(0.9));
        let b = snap_with_columns(60, 1500, 600, 0.05, Some(0.7));
        let ds = DeltaSet::compute(&a, &b);
        let json = serde_json::to_string(&ds).expect("serialize");
        let ds2: DeltaSet =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ds, ds2);
    }

    #[test]
    fn delta_json_roundtrip() {
        let d = StatisticsDelta::ColumnNDV {
            table: "t".to_string(),
            column: "c".to_string(),
            old: 100,
            new: 200,
        };
        let json = serde_json::to_string(&d).expect("serialize");
        let d2: StatisticsDelta =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(d, d2);
    }
}
