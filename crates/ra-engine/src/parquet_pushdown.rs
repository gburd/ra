//! Predicate pushdown to Parquet row groups using metadata.
//!
//! Provides rewrite rules that transform a filter over a scan into
//! a parquet-aware scan that skips row groups whose min/max column
//! statistics prove the predicate cannot match.
//!
//! # Architecture
//!
//! The optimizer represents a Parquet-aware scan as:
//! ```text
//! (parquet-scan ?table ?pred ?row_groups)
//! ```
//! where `?row_groups` encodes which row groups survived filtering.
//! The cost model rewards skipping row groups by reducing scan cost
//! proportionally.
//!
//! # Row Group Filtering
//!
//! For each row group, the predicate is evaluated against the column
//! min/max statistics. A row group is skipped only when the
//! statistics *prove* no rows can match. When stats are missing,
//! the row group is conservatively included.

use std::cmp::Ordering;
use std::collections::HashMap;

use ra_core::formats::{FileColumnStats, FileMetadata, RowGroupMeta, ScalarValue};

/// Result of evaluating a predicate against row group statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowGroupMatch {
    /// Statistics prove no rows in this group can match.
    Pruned,
    /// The group may contain matching rows (or stats are missing).
    MayMatch,
}

/// A comparison predicate that can be evaluated against row group
/// min/max statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct PushdownPredicate {
    /// Column name referenced in the predicate.
    pub column: String,
    /// Comparison operator.
    pub op: CompareOp,
    /// Literal value on the right-hand side.
    pub value: ScalarValue,
}

/// Comparison operators supported for predicate pushdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Column = value
    Eq,
    /// Column != value
    Ne,
    /// Column < value
    Lt,
    /// Column <= value
    Le,
    /// Column > value
    Gt,
    /// Column >= value
    Ge,
}

impl std::fmt::Display for CompareOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eq => write!(f, "="),
            Self::Ne => write!(f, "!="),
            Self::Lt => write!(f, "<"),
            Self::Le => write!(f, "<="),
            Self::Gt => write!(f, ">"),
            Self::Ge => write!(f, ">="),
        }
    }
}

/// Evaluate a predicate against a row group's column statistics.
///
/// Returns `Pruned` when the statistics prove no rows can match,
/// `MayMatch` otherwise (including when stats are absent).
#[must_use]
pub fn evaluate_predicate(pred: &PushdownPredicate, rg: &RowGroupMeta) -> RowGroupMatch {
    let Some(stats) = rg.column_stats.get(&pred.column) else {
        return RowGroupMatch::MayMatch;
    };
    evaluate_against_stats(pred, stats)
}

/// Evaluate a comparison predicate against min/max column stats.
///
/// The logic for each operator:
/// - `col > val`:  prune if `max <= val`
/// - `col >= val`: prune if `max < val`
/// - `col < val`:  prune if `min >= val`
/// - `col <= val`: prune if `min > val`
/// - `col = val`:  prune if `val < min` or `val > max`
/// - `col != val`: prune if `min == max == val` (all rows equal)
#[must_use]
fn evaluate_against_stats(pred: &PushdownPredicate, stats: &FileColumnStats) -> RowGroupMatch {
    let val = &pred.value;

    match pred.op {
        CompareOp::Gt => {
            // col > val: prune when max <= val
            if let Some(max) = &stats.max {
                if let Some(Ordering::Less | Ordering::Equal) = max.partial_cmp_value(val) {
                    return RowGroupMatch::Pruned;
                }
            }
        }
        CompareOp::Ge => {
            // col >= val: prune when max < val
            if let Some(max) = &stats.max {
                if let Some(Ordering::Less) = max.partial_cmp_value(val) {
                    return RowGroupMatch::Pruned;
                }
            }
        }
        CompareOp::Lt => {
            // col < val: prune when min >= val
            if let Some(min) = &stats.min {
                if let Some(Ordering::Greater | Ordering::Equal) = min.partial_cmp_value(val) {
                    return RowGroupMatch::Pruned;
                }
            }
        }
        CompareOp::Le => {
            // col <= val: prune when min > val
            if let Some(min) = &stats.min {
                if let Some(Ordering::Greater) = min.partial_cmp_value(val) {
                    return RowGroupMatch::Pruned;
                }
            }
        }
        CompareOp::Eq => {
            // col = val: prune when val < min or val > max
            if let Some(min) = &stats.min {
                if let Some(Ordering::Less) = val.partial_cmp_value(min) {
                    return RowGroupMatch::Pruned;
                }
            }
            if let Some(max) = &stats.max {
                if let Some(Ordering::Greater) = val.partial_cmp_value(max) {
                    return RowGroupMatch::Pruned;
                }
            }
        }
        CompareOp::Ne => {
            // col != val: prune only when min == max == val
            if let (Some(min), Some(max)) = (&stats.min, &stats.max) {
                let min_eq = min.partial_cmp_value(val) == Some(Ordering::Equal);
                let max_eq = max.partial_cmp_value(val) == Some(Ordering::Equal);
                if min_eq && max_eq {
                    return RowGroupMatch::Pruned;
                }
            }
        }
    }

    RowGroupMatch::MayMatch
}

/// Filter row groups from file metadata, returning indices of groups
/// that may contain matching rows.
///
/// Row groups are pruned when *all* predicates prove the group
/// cannot match. If any predicate cannot be evaluated (missing
/// stats), the group is conservatively included.
#[must_use]
pub fn filter_row_groups(predicates: &[PushdownPredicate], metadata: &FileMetadata) -> Vec<usize> {
    if predicates.is_empty() {
        return (0..metadata.row_groups.len()).collect();
    }

    let mut surviving = Vec::new();
    for rg in &metadata.row_groups {
        let pruned = predicates
            .iter()
            .any(|pred| evaluate_predicate(pred, rg) == RowGroupMatch::Pruned);
        if !pruned {
            surviving.push(rg.index);
        }
    }
    surviving
}

/// Estimate the cost reduction from row group pruning.
///
/// Returns the fraction of data that will be scanned (0.0 = all
/// pruned, 1.0 = nothing pruned). The cost model uses this to
/// discount scan costs.
#[must_use]
pub fn pruning_selectivity(total_row_groups: usize, surviving_row_groups: usize) -> f64 {
    if total_row_groups == 0 {
        return 1.0;
    }
    surviving_row_groups as f64 / total_row_groups as f64
}

/// Build a metadata registry for files and their metadata.
///
/// The optimizer queries this to check whether predicate pushdown
/// is applicable for a given scan target.
#[derive(Debug, Default)]
pub struct ParquetMetadataRegistry {
    files: HashMap<String, FileMetadata>,
}

impl ParquetMetadataRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register file metadata for a table/file name.
    pub fn register(&mut self, name: impl Into<String>, metadata: FileMetadata) {
        self.files.insert(name.into(), metadata);
    }

    /// Look up metadata for a table/file name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&FileMetadata> {
        self.files.get(name)
    }

    /// Check if a table has Parquet metadata registered.
    #[must_use]
    pub fn has_metadata(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }

    /// Compute surviving row group indices for a table given
    /// predicates.
    #[must_use]
    pub fn filter_for_table(
        &self,
        table: &str,
        predicates: &[PushdownPredicate],
    ) -> Option<Vec<usize>> {
        let meta = self.files.get(table)?;
        Some(filter_row_groups(predicates, meta))
    }

    /// Estimate the scan selectivity after pushdown for a table.
    #[must_use]
    pub fn estimate_selectivity(&self, table: &str, predicates: &[PushdownPredicate]) -> f64 {
        let Some(meta) = self.files.get(table) else {
            return 1.0;
        };
        let surviving = filter_row_groups(predicates, meta);
        pruning_selectivity(meta.row_groups.len(), surviving.len())
    }
}

/// Check if a table uses Parquet storage format.
///
/// This function is used as a precondition check for Parquet-specific rules.
/// It queries the `FactsProvider` to get the table's schema information and
/// checks the storage format.
///
/// # Returns
///
/// - `true` if the table uses `StorageFormat::Parquet`
/// - `true` if the storage format is `StorageFormat::Unknown` (conservative)
/// - `false` for all other formats (`RowBased`, Columnar, Orc, `ArrowIpc`, etc.)
///
/// If the table cannot be found in the schema, returns `true` (conservative).
pub fn is_parquet_storage(table_name: &str, facts: &dyn ra_core::facts::FactsProvider) -> bool {
    if let Some(table_info) = facts.get_schema(table_name) {
        use ra_core::facts::StorageFormat;
        matches!(
            table_info.storage_format,
            StorageFormat::Parquet | StorageFormat::Unknown
        )
    } else {
        // Conservative: if we don't have schema info, allow the rule
        true
    }
}

/// Rewrite rules for parquet predicate pushdown.
///
/// These rules operate within the egg equality saturation framework.
/// They transform `(filter ?pred (scan ?table))` into a
/// representation that the cost model can reward when Parquet
/// metadata is available.
///
/// The actual row group filtering happens during plan extraction
/// (not during rewrite), since the rewrite rules operate on
/// symbolic patterns. The cost function uses the
/// [`ParquetMetadataRegistry`] to estimate how many row groups
/// would be pruned.
///
/// # Storage Format Preconditions
///
/// These rules should only apply to tables using Parquet storage.
/// The filtering is done via the rule metadata system (see
/// `rule_metadata.rs`) which evaluates preconditions before applying
/// rules. This prevents Parquet-specific optimizations from being
/// applied to non-Parquet tables.
///
/// For runtime filtering, the optimizer should call
/// [`is_parquet_storage`] to check each table before applying these
/// rules.
#[must_use]
pub fn parquet_pushdown_rules(
) -> Vec<egg::Rewrite<crate::egraph::RelLang, crate::analysis::RelAnalysis>> {
    use egg::rewrite;
    vec![
        // When a filter sits on top of a scan, the cost model can
        // check if the scan target has Parquet metadata and discount
        // the cost accordingly. This rule pushes the filter into the
        // scan by keeping the pattern as-is but marking it for the
        // cost model via the existing filter-through-project and
        // filter-merge rules.
        //
        // The key insight: we don't need a new RelLang variant.
        // Instead, the cost model checks for the pattern
        // (filter ?pred (scan ?table)) and applies the pushdown
        // discount when Parquet metadata is available.

        // Split conjunctive predicates so each can be evaluated
        // individually against row group stats. This enables
        // partial pushdown where only some conjuncts use stats.
        //
        // NOTE: This rule should be filtered by storage format at
        // rule selection time. See docs above.
        rewrite!("parquet-filter-split-for-pushdown";
            "(filter (and ?p1 ?p2) (scan ?table))" =>
            "(filter ?p1 (filter ?p2 (scan ?table)))"
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::formats::{FileColumnStats, ScalarValue};

    fn make_row_group(index: usize, stats: HashMap<String, FileColumnStats>) -> RowGroupMeta {
        RowGroupMeta {
            index,
            offset: 0,
            num_rows: 1000,
            column_stats: stats,
            compressed_size: 4096,
            uncompressed_size: 8192,
            column_encodings: HashMap::new(),
        }
    }

    fn make_stats(min: i64, max: i64) -> FileColumnStats {
        FileColumnStats {
            min: Some(ScalarValue::Int64(min)),
            max: Some(ScalarValue::Int64(max)),
            null_count: 0,
            distinct_count: None,
        }
    }

    fn make_metadata(row_groups: Vec<RowGroupMeta>) -> FileMetadata {
        let total_rows = row_groups.iter().map(|rg| rg.num_rows).sum();
        FileMetadata {
            schema: ra_core::formats::Schema::default(),
            num_rows: total_rows,
            row_groups,
            file_stats: HashMap::new(),
            mtime: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    // -- evaluate_predicate tests --

    #[test]
    fn gt_prunes_when_max_le_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(1, 100))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);
    }

    #[test]
    fn gt_keeps_when_max_gt_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(1, 200))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn ge_prunes_when_max_lt_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(1, 99))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Ge,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);
    }

    #[test]
    fn ge_keeps_when_max_eq_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(1, 100))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Ge,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn lt_prunes_when_min_ge_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(100, 200))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Lt,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);
    }

    #[test]
    fn lt_keeps_when_min_lt_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(50, 200))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Lt,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn le_prunes_when_min_gt_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(101, 200))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Le,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);
    }

    #[test]
    fn le_keeps_when_min_eq_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(100, 200))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Le,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn eq_prunes_when_val_below_min() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(50, 100))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Eq,
            value: ScalarValue::Int64(10),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);
    }

    #[test]
    fn eq_prunes_when_val_above_max() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(50, 100))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Eq,
            value: ScalarValue::Int64(200),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);
    }

    #[test]
    fn eq_keeps_when_val_in_range() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(50, 100))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Eq,
            value: ScalarValue::Int64(75),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn ne_prunes_when_all_equal_to_val() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(42, 42))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Ne,
            value: ScalarValue::Int64(42),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);
    }

    #[test]
    fn ne_keeps_when_range_wider() {
        let rg = make_row_group(0, HashMap::from([("a".into(), make_stats(40, 50))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Ne,
            value: ScalarValue::Int64(42),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn missing_stats_returns_may_match() {
        let rg = make_row_group(0, HashMap::new());
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(100),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn missing_column_returns_may_match() {
        let rg = make_row_group(0, HashMap::from([("b".into(), make_stats(1, 100))]));
        let pred = PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(50),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::MayMatch,);
    }

    // -- filter_row_groups tests --

    #[test]
    fn filter_partial_scan() {
        let metadata = make_metadata(vec![
            make_row_group(0, HashMap::from([("a".into(), make_stats(1, 100))])),
            make_row_group(1, HashMap::from([("a".into(), make_stats(101, 200))])),
            make_row_group(2, HashMap::from([("a".into(), make_stats(201, 300))])),
        ]);

        // a > 150: should keep groups 1 (max=200>150) and 2
        let predicates = vec![PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(150),
        }];

        let surviving = filter_row_groups(&predicates, &metadata);
        assert_eq!(surviving, vec![1, 2]);
    }

    #[test]
    fn filter_no_match() {
        let metadata = make_metadata(vec![
            make_row_group(0, HashMap::from([("a".into(), make_stats(1, 100))])),
            make_row_group(1, HashMap::from([("a".into(), make_stats(101, 200))])),
        ]);

        // a > 300: all groups pruned
        let predicates = vec![PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(300),
        }];

        let surviving = filter_row_groups(&predicates, &metadata);
        assert!(surviving.is_empty());
    }

    #[test]
    fn filter_all_match() {
        let metadata = make_metadata(vec![
            make_row_group(0, HashMap::from([("a".into(), make_stats(1, 100))])),
            make_row_group(1, HashMap::from([("a".into(), make_stats(101, 200))])),
        ]);

        // a > 0: all groups match
        let predicates = vec![PushdownPredicate {
            column: "a".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(0),
        }];

        let surviving = filter_row_groups(&predicates, &metadata);
        assert_eq!(surviving, vec![0, 1]);
    }

    #[test]
    fn filter_empty_predicates_returns_all() {
        let metadata = make_metadata(vec![
            make_row_group(0, HashMap::from([("a".into(), make_stats(1, 100))])),
            make_row_group(1, HashMap::from([("a".into(), make_stats(101, 200))])),
        ]);

        let surviving = filter_row_groups(&[], &metadata);
        assert_eq!(surviving, vec![0, 1]);
    }

    #[test]
    fn filter_multiple_predicates_any_prunes() {
        let metadata = make_metadata(vec![
            make_row_group(
                0,
                HashMap::from([
                    ("a".into(), make_stats(1, 100)),
                    ("b".into(), make_stats(1, 50)),
                ]),
            ),
            make_row_group(
                1,
                HashMap::from([
                    ("a".into(), make_stats(101, 200)),
                    ("b".into(), make_stats(51, 100)),
                ]),
            ),
        ]);

        // a > 50 prunes nothing; b > 80 prunes group 0 (max=50)
        let predicates = vec![
            PushdownPredicate {
                column: "a".into(),
                op: CompareOp::Gt,
                value: ScalarValue::Int64(50),
            },
            PushdownPredicate {
                column: "b".into(),
                op: CompareOp::Gt,
                value: ScalarValue::Int64(80),
            },
        ];

        let surviving = filter_row_groups(&predicates, &metadata);
        assert_eq!(surviving, vec![1]);
    }

    // -- pruning_selectivity tests --

    #[test]
    fn selectivity_all_pruned() {
        assert!((pruning_selectivity(10, 0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn selectivity_none_pruned() {
        assert!((pruning_selectivity(10, 10) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn selectivity_half_pruned() {
        assert!((pruning_selectivity(10, 5) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn selectivity_empty_metadata() {
        assert!((pruning_selectivity(0, 0) - 1.0).abs() < f64::EPSILON);
    }

    // -- ParquetMetadataRegistry tests --

    #[test]
    fn registry_basic_operations() {
        let mut reg = ParquetMetadataRegistry::new();
        assert!(!reg.has_metadata("events"));

        reg.register(
            "events",
            make_metadata(vec![make_row_group(
                0,
                HashMap::from([("ts".into(), make_stats(1, 100))]),
            )]),
        );

        assert!(reg.has_metadata("events"));
        assert!(reg.get("events").is_some());
        assert!(!reg.has_metadata("other"));
    }

    #[test]
    fn registry_filter_for_table() {
        let mut reg = ParquetMetadataRegistry::new();
        reg.register(
            "events",
            make_metadata(vec![
                make_row_group(0, HashMap::from([("ts".into(), make_stats(1, 100))])),
                make_row_group(1, HashMap::from([("ts".into(), make_stats(101, 200))])),
            ]),
        );

        let preds = vec![PushdownPredicate {
            column: "ts".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(150),
        }];

        let result = reg.filter_for_table("events", &preds);
        assert_eq!(result, Some(vec![1]));

        assert!(reg.filter_for_table("missing", &preds).is_none());
    }

    #[test]
    fn registry_estimate_selectivity() {
        let mut reg = ParquetMetadataRegistry::new();
        reg.register(
            "events",
            make_metadata(vec![
                make_row_group(0, HashMap::from([("ts".into(), make_stats(1, 100))])),
                make_row_group(1, HashMap::from([("ts".into(), make_stats(101, 200))])),
                make_row_group(2, HashMap::from([("ts".into(), make_stats(201, 300))])),
            ]),
        );

        // ts > 150: prunes group 0, keeps groups 1 and 2
        let preds = vec![PushdownPredicate {
            column: "ts".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(150),
        }];

        let sel = reg.estimate_selectivity("events", &preds);
        assert!((sel - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn registry_missing_table_selectivity_is_one() {
        let reg = ParquetMetadataRegistry::new();
        let preds = vec![PushdownPredicate {
            column: "x".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Int64(0),
        }];
        assert!((reg.estimate_selectivity("missing", &preds) - 1.0).abs() < f64::EPSILON);
    }

    // -- String predicate tests --

    #[test]
    fn string_predicate_gt() {
        let rg = make_row_group(
            0,
            HashMap::from([(
                "name".into(),
                FileColumnStats {
                    min: Some(ScalarValue::Utf8("alice".into())),
                    max: Some(ScalarValue::Utf8("charlie".into())),
                    null_count: 0,
                    distinct_count: None,
                },
            )]),
        );

        // name > "delta": max="charlie" <= "delta", prune
        let pred = PushdownPredicate {
            column: "name".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Utf8("delta".into()),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);

        // name > "bob": max="charlie" > "bob", keep
        let pred2 = PushdownPredicate {
            column: "name".into(),
            op: CompareOp::Gt,
            value: ScalarValue::Utf8("bob".into()),
        };
        assert_eq!(evaluate_predicate(&pred2, &rg), RowGroupMatch::MayMatch,);
    }

    #[test]
    fn float_predicate_eq() {
        let rg = make_row_group(
            0,
            HashMap::from([(
                "price".into(),
                FileColumnStats {
                    min: Some(ScalarValue::Float64(10.0)),
                    max: Some(ScalarValue::Float64(50.0)),
                    null_count: 0,
                    distinct_count: None,
                },
            )]),
        );

        // price = 5.0: below min, prune
        let pred = PushdownPredicate {
            column: "price".into(),
            op: CompareOp::Eq,
            value: ScalarValue::Float64(5.0),
        };
        assert_eq!(evaluate_predicate(&pred, &rg), RowGroupMatch::Pruned);

        // price = 25.0: in range, keep
        let pred2 = PushdownPredicate {
            column: "price".into(),
            op: CompareOp::Eq,
            value: ScalarValue::Float64(25.0),
        };
        assert_eq!(evaluate_predicate(&pred2, &rg), RowGroupMatch::MayMatch,);
    }

    // -- Rewrite rule integration test --

    #[test]
    fn pushdown_rules_are_valid() {
        let rules = parquet_pushdown_rules();
        assert!(!rules.is_empty());
    }
}
