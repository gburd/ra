//! Timeline data model for optimizer snapshots over time.
//!
//! A [`Timeline`] is a sequence of [`Snapshot`]s, each capturing the
//! optimizer state at a point in the optimization process. This is
//! what the TUI plays back and visualizes.
//!
//! Supports loading both JSON optimizer timelines (native format)
//! and TOML statistics timelines via [`Timeline::from_stats_timeline`].

use serde::{Deserialize, Serialize};

/// A single point-in-time snapshot of optimizer state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Human-readable label for this step (e.g., "Initial", "After predicate pushdown").
    pub label: String,
    /// Zero-based step index.
    pub step: usize,
    /// The plan tree as a display string (pre-formatted).
    pub plan_text: String,
    /// Estimated cost at this step.
    pub cost: f64,
    /// Rules applied in this step.
    pub rules_applied: Vec<String>,
    /// Table statistics at this step.
    pub table_stats: Vec<TableStatEntry>,
    /// Diagnostic messages.
    pub diagnostics: Vec<String>,
}

/// Summary statistics for a single table at a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStatEntry {
    /// Table name.
    pub table: String,
    /// Row count.
    pub row_count: u64,
    /// Staleness label.
    pub staleness: String,
    /// Confidence level (0.0 to 1.0).
    pub confidence: f64,
}

/// An ordered sequence of optimizer snapshots for playback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    /// SQL query being optimized.
    pub query: String,
    /// Hardware profile name used.
    pub hardware_profile: String,
    /// Ordered snapshots.
    pub snapshots: Vec<Snapshot>,
}

impl Timeline {
    /// Create a new empty timeline for a query.
    pub fn new(query: impl Into<String>, hardware: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            hardware_profile: hardware.into(),
            snapshots: Vec::new(),
        }
    }

    /// Add a snapshot to the timeline.
    pub fn push(&mut self, snapshot: Snapshot) {
        self.snapshots.push(snapshot);
    }

    /// Number of snapshots in the timeline.
    #[must_use]
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// Whether the timeline has no snapshots.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Load a timeline from TOML content.
    ///
    /// Parses a TOML statistics timeline (from ra-stats) and converts
    /// it into the TUI timeline format using [`from_stats_timeline`].
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is invalid or the stats timeline
    /// cannot be converted.
    pub fn from_toml(toml_content: &str) -> Result<Self, String> {
        let stats_tl =
            ra_stats::timeline::Timeline::from_toml(toml_content)
                .map_err(|e| format!("parsing TOML timeline: {e}"))?;
        Ok(Self::from_stats_timeline(&stats_tl))
    }

    /// Convert a ra-stats TOML timeline to a TUI timeline.
    ///
    /// Maps each statistics snapshot to a TUI snapshot with:
    /// - Label from the stats snapshot label or time offset
    /// - A synthetic plan text showing table scan operators
    /// - Cost derived from total row counts across tables
    /// - Diagnostics from events and feedback near each snapshot
    #[must_use]
    pub fn from_stats_timeline(
        stats: &ra_stats::timeline::Timeline,
    ) -> Self {
        let query = stats.feedback.first().map_or_else(
            || {
                format!(
                    "Statistics evolution: {}",
                    stats.metadata.name
                )
            },
            |f| f.query.clone(),
        );

        let mut tl = Self::new(&query, "auto");

        for (idx, snap) in stats.snapshots.iter().enumerate() {
            let label = snap.label.clone().unwrap_or_else(|| {
                format!("t={}s", snap.time_offset)
            });

            let plan_text = build_stats_plan_text(&snap.tables);

            let cost = snap
                .tables
                .iter()
                .map(|t| t.row_count as f64)
                .sum::<f64>();

            let table_stats = snap
                .tables
                .iter()
                .map(|t| {
                    let staleness =
                        infer_staleness(idx, stats);
                    let confidence =
                        infer_confidence(t, idx, stats);
                    TableStatEntry {
                        table: t.name.clone(),
                        row_count: t.row_count,
                        staleness,
                        confidence,
                    }
                })
                .collect();

            let diagnostics =
                collect_diagnostics(snap.time_offset, stats);

            tl.push(Snapshot {
                label,
                step: idx,
                plan_text,
                cost,
                rules_applied: Vec::new(),
                table_stats,
                diagnostics,
            });
        }

        tl
    }

    /// Build a demo timeline with synthetic data for testing.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn demo() -> Self {
        let mut tl = Self::new(
            "SELECT o.id, c.name, SUM(o.amount) \
             FROM orders o JOIN customers c ON o.customer_id = c.id \
             WHERE o.status = 'completed' \
             GROUP BY o.id, c.name \
             ORDER BY SUM(o.amount) DESC \
             LIMIT 10",
            "auto",
        );

        let stats = vec![
            TableStatEntry {
                table: "orders".into(),
                row_count: 1_000_000,
                staleness: "Fresh".into(),
                confidence: 0.95,
            },
            TableStatEntry {
                table: "customers".into(),
                row_count: 50_000,
                staleness: "Fresh".into(),
                confidence: 0.98,
            },
        ];

        tl.push(Snapshot {
            label: "Initial parse".into(),
            step: 0,
            plan_text: concat!(
                "Limit(count=10, offset=0)\n",
                "  Sort [SUM(o.amount) DESC]\n",
                "    Aggregate [o.id, c.name] SUM(o.amount)\n",
                "      Filter [o.status = 'completed']\n",
                "        Inner Join [o.customer_id = c.id]\n",
                "          Scan(orders AS o)\n",
                "          Scan(customers AS c)\n",
            )
            .into(),
            cost: 125_000.0,
            rules_applied: vec![],
            table_stats: stats.clone(),
            diagnostics: vec!["Parsed SQL to relational algebra".into()],
        });

        tl.push(Snapshot {
            label: "Predicate pushdown".into(),
            step: 1,
            plan_text: concat!(
                "Limit(count=10, offset=0)\n",
                "  Sort [SUM(o.amount) DESC]\n",
                "    Aggregate [o.id, c.name] SUM(o.amount)\n",
                "      Inner Join [o.customer_id = c.id]\n",
                "        Filter [o.status = 'completed']\n",
                "          Scan(orders AS o)\n",
                "        Scan(customers AS c)\n",
            )
            .into(),
            cost: 87_500.0,
            rules_applied: vec!["filter-join-push".into()],
            table_stats: stats.clone(),
            diagnostics: vec![
                "Pushed filter below join".into(),
                "Estimated selectivity: 0.15".into(),
            ],
        });

        tl.push(Snapshot {
            label: "Join reorder".into(),
            step: 2,
            plan_text: concat!(
                "Limit(count=10, offset=0)\n",
                "  Sort [SUM(o.amount) DESC]\n",
                "    Aggregate [o.id, c.name] SUM(o.amount)\n",
                "      Inner Join [c.id = o.customer_id]\n",
                "        Scan(customers AS c)\n",
                "        Filter [o.status = 'completed']\n",
                "          Scan(orders AS o)\n",
            )
            .into(),
            cost: 72_000.0,
            rules_applied: vec!["join-commute".into()],
            table_stats: stats.clone(),
            diagnostics: vec![
                "Reordered join: smaller table on build side".into(),
            ],
        });

        tl.push(Snapshot {
            label: "Index selection".into(),
            step: 3,
            plan_text: concat!(
                "Limit(count=10, offset=0)\n",
                "  Sort [SUM(o.amount) DESC]\n",
                "    Aggregate [o.id, c.name] SUM(o.amount)\n",
                "      Hash Join [c.id = o.customer_id]\n",
                "        Scan(customers AS c)\n",
                "        Index Scan(orders AS o, idx_status)\n",
                "          filter: o.status = 'completed'\n",
            )
            .into(),
            cost: 45_000.0,
            rules_applied: vec![
                "index-scan".into(),
                "hash-join-selection".into(),
            ],
            table_stats: stats.clone(),
            diagnostics: vec![
                "Selected hash join for equality predicate".into(),
                "Using index idx_status for filter".into(),
            ],
        });

        let stale_stats = vec![
            TableStatEntry {
                table: "orders".into(),
                row_count: 1_000_000,
                staleness: "SlightlyStale".into(),
                confidence: 0.88,
            },
            TableStatEntry {
                table: "customers".into(),
                row_count: 50_000,
                staleness: "Fresh".into(),
                confidence: 0.98,
            },
        ];

        tl.push(Snapshot {
            label: "Cost calibration".into(),
            step: 4,
            plan_text: concat!(
                "Limit(count=10, offset=0)\n",
                "  TopN [SUM(o.amount) DESC, n=10]\n",
                "    Aggregate [o.id, c.name] SUM(o.amount)\n",
                "      Hash Join [c.id = o.customer_id]\n",
                "        Scan(customers AS c)\n",
                "        Index Scan(orders AS o, idx_status)\n",
                "          filter: o.status = 'completed'\n",
            )
            .into(),
            cost: 38_000.0,
            rules_applied: vec!["sort-limit-to-topn".into()],
            table_stats: stale_stats,
            diagnostics: vec![
                "Merged Sort + Limit into TopN operator".into(),
                "Statistics slightly stale for orders table".into(),
            ],
        });

        tl
    }
}

/// Build a synthetic plan text from table snapshot data.
fn build_stats_plan_text(
    tables: &[ra_stats::timeline::TableSnapshot],
) -> String {
    use std::fmt::Write;
    let mut plan = String::new();
    if tables.len() > 1 {
        plan.push_str("Join\n");
        for t in tables {
            let _ = writeln!(
                plan,
                "  Scan({}, rows={})",
                t.name, t.row_count
            );
        }
    } else if let Some(t) = tables.first() {
        let _ = writeln!(
            plan,
            "Scan({}, rows={})",
            t.name, t.row_count
        );
    } else {
        plan.push_str("(empty)\n");
    }
    plan
}

/// Infer staleness label based on whether an ANALYZE event
/// occurred between the previous and current snapshot.
fn infer_staleness(
    snap_idx: usize,
    stats: &ra_stats::timeline::Timeline,
) -> String {
    if snap_idx == 0 {
        return "Fresh".into();
    }
    let prev_offset = stats.snapshots[snap_idx - 1].time_offset;
    let curr_offset = stats.snapshots[snap_idx].time_offset;

    let has_analyze = stats.events.iter().any(|e| {
        e.time_offset > prev_offset
            && e.time_offset <= curr_offset
            && e.kind == ra_stats::timeline::EventKind::Analyze
    });

    let has_dml = stats.events.iter().any(|e| {
        e.time_offset > prev_offset
            && e.time_offset <= curr_offset
            && matches!(
                e.kind,
                ra_stats::timeline::EventKind::Insert
                    | ra_stats::timeline::EventKind::Update
                    | ra_stats::timeline::EventKind::Delete
            )
    });

    if has_analyze {
        "Fresh".into()
    } else if has_dml {
        "Stale".into()
    } else {
        "Fresh".into()
    }
}

/// Infer confidence based on feedback accuracy near this snapshot.
fn infer_confidence(
    table: &ra_stats::timeline::TableSnapshot,
    snap_idx: usize,
    stats: &ra_stats::timeline::Timeline,
) -> f64 {
    let offset = stats.snapshots[snap_idx].time_offset;
    let nearby_feedback: Vec<&ra_stats::timeline::ExecutionFeedback> =
        stats
            .feedback
            .iter()
            .filter(|f| {
                f.time_offset.abs_diff(offset) <= 1800
            })
            .collect();

    if nearby_feedback.is_empty() {
        return 0.90;
    }

    let mut accuracy_sum = 0.0;
    for fb in &nearby_feedback {
        if fb.actual_rows > 0.0 {
            let ratio = fb.estimated_rows / fb.actual_rows;
            let accuracy = if ratio > 1.0 {
                1.0 / ratio
            } else {
                ratio
            };
            accuracy_sum += accuracy;
        }
    }

    let _ = table;
    let avg = accuracy_sum / nearby_feedback.len() as f64;
    avg.clamp(0.0, 1.0)
}

/// Collect diagnostic messages from events and feedback near a
/// snapshot's time offset.
fn collect_diagnostics(
    time_offset: u64,
    stats: &ra_stats::timeline::Timeline,
) -> Vec<String> {
    let mut diags = Vec::new();

    for ev in &stats.events {
        if ev.time_offset.abs_diff(time_offset) <= 900 {
            let desc = ev
                .description
                .clone()
                .unwrap_or_else(|| {
                    format!("{:?} on {}", ev.kind, ev.table)
                });
            diags.push(desc);
        }
    }

    for fb in &stats.feedback {
        if fb.time_offset == time_offset {
            let op = fb
                .operator
                .clone()
                .unwrap_or_else(|| "unknown".into());
            diags.push(format!(
                "{}: est={:.0} actual={:.0}",
                op, fb.estimated_rows, fb.actual_rows,
            ));
        }
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_timeline_not_empty() {
        let tl = Timeline::demo();
        assert!(!tl.is_empty());
        assert_eq!(tl.len(), 5);
    }

    #[test]
    fn new_timeline_is_empty() {
        let tl = Timeline::new("SELECT 1", "auto");
        assert!(tl.is_empty());
        assert_eq!(tl.len(), 0);
    }

    #[test]
    fn push_increments_len() {
        let mut tl = Timeline::new("SELECT 1", "auto");
        tl.push(Snapshot {
            label: "s1".into(),
            step: 0,
            plan_text: "Scan(t)".into(),
            cost: 100.0,
            rules_applied: vec![],
            table_stats: vec![],
            diagnostics: vec![],
        });
        assert_eq!(tl.len(), 1);
    }

    #[test]
    fn from_stats_timeline_converts_snapshots() {
        let stats = make_test_stats_timeline();
        let tl = Timeline::from_stats_timeline(&stats);
        assert_eq!(tl.snapshots.len(), 2);
        assert_eq!(tl.snapshots[0].label, "initial");
        assert_eq!(tl.snapshots[1].label, "after insert");
    }

    #[test]
    fn from_stats_timeline_uses_feedback_query() {
        let stats = make_test_stats_timeline();
        let tl = Timeline::from_stats_timeline(&stats);
        assert!(tl.query.contains("SELECT"));
    }

    #[test]
    fn from_stats_timeline_no_feedback_uses_name() {
        let mut stats = make_test_stats_timeline();
        stats.feedback.clear();
        let tl = Timeline::from_stats_timeline(&stats);
        assert!(tl.query.contains("test-timeline"));
    }

    #[test]
    fn from_stats_timeline_cost_is_row_sum() {
        let stats = make_test_stats_timeline();
        let tl = Timeline::from_stats_timeline(&stats);
        assert!((tl.snapshots[0].cost - 1000.0).abs() < 0.1);
        assert!((tl.snapshots[1].cost - 1500.0).abs() < 0.1);
    }

    #[test]
    fn from_stats_timeline_table_stats_populated() {
        let stats = make_test_stats_timeline();
        let tl = Timeline::from_stats_timeline(&stats);
        assert_eq!(tl.snapshots[0].table_stats.len(), 1);
        assert_eq!(
            tl.snapshots[0].table_stats[0].table,
            "test_table"
        );
    }

    #[test]
    fn from_stats_timeline_staleness_after_dml() {
        let stats = make_test_stats_timeline();
        let tl = Timeline::from_stats_timeline(&stats);
        assert_eq!(
            tl.snapshots[1].table_stats[0].staleness,
            "Stale"
        );
    }

    #[test]
    fn from_stats_timeline_diagnostics_from_events() {
        let stats = make_test_stats_timeline();
        let tl = Timeline::from_stats_timeline(&stats);
        assert!(!tl.snapshots[1].diagnostics.is_empty());
    }

    #[test]
    fn from_stats_timeline_plan_text_single_table() {
        let stats = make_test_stats_timeline();
        let tl = Timeline::from_stats_timeline(&stats);
        assert!(tl.snapshots[0].plan_text.contains("Scan("));
    }

    #[test]
    fn from_stats_timeline_plan_text_multi_table() {
        let mut stats = make_test_stats_timeline();
        stats.snapshots[0]
            .tables
            .push(ra_stats::timeline::TableSnapshot {
                name: "other".into(),
                row_count: 500,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![],
            });
        let tl = Timeline::from_stats_timeline(&stats);
        assert!(tl.snapshots[0].plan_text.contains("Join"));
    }

    #[test]
    fn from_toml_invalid_returns_error() {
        let result = Timeline::from_toml("not valid toml {{{");
        assert!(result.is_err());
    }

    #[test]
    fn from_stats_timeline_empty_snapshots() {
        let stats = ra_stats::timeline::Timeline {
            metadata: ra_stats::timeline::TimelineMetadata {
                name: "empty".into(),
                description: "empty".into(),
                database: None,
                schema: None,
                scale_factor: None,
                duration_seconds: None,
            },
            snapshots: vec![],
            events: vec![],
            feedback: vec![],
        };
        let tl = Timeline::from_stats_timeline(&stats);
        assert!(tl.is_empty());
    }

    #[test]
    fn infer_staleness_first_snapshot_is_fresh() {
        let stats = make_test_stats_timeline();
        let result = infer_staleness(0, &stats);
        assert_eq!(result, "Fresh");
    }

    #[test]
    fn infer_confidence_no_feedback() {
        let stats = ra_stats::timeline::Timeline {
            metadata: ra_stats::timeline::TimelineMetadata {
                name: "t".into(),
                description: "t".into(),
                database: None,
                schema: None,
                scale_factor: None,
                duration_seconds: None,
            },
            snapshots: vec![ra_stats::timeline::Snapshot {
                time_offset: 0,
                label: Some("s".into()),
                tables: vec![ra_stats::timeline::TableSnapshot {
                    name: "t".into(),
                    row_count: 100,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                }],
            }],
            events: vec![],
            feedback: vec![],
        };
        let table = &stats.snapshots[0].tables[0];
        let conf = infer_confidence(table, 0, &stats);
        assert!((conf - 0.90).abs() < 0.01);
    }

    #[test]
    fn build_stats_plan_text_empty_tables() {
        let result = build_stats_plan_text(&[]);
        assert!(result.contains("empty"));
    }

    fn make_test_stats_timeline(
    ) -> ra_stats::timeline::Timeline {
        ra_stats::timeline::Timeline {
            metadata: ra_stats::timeline::TimelineMetadata {
                name: "test-timeline".into(),
                description: "test".into(),
                database: Some("postgresql".into()),
                schema: None,
                scale_factor: None,
                duration_seconds: Some(3600),
            },
            snapshots: vec![
                ra_stats::timeline::Snapshot {
                    time_offset: 0,
                    label: Some("initial".into()),
                    tables: vec![
                        ra_stats::timeline::TableSnapshot {
                            name: "test_table".into(),
                            row_count: 1000,
                            page_count: Some(10),
                            avg_row_size: Some(100.0),
                            table_size_bytes: None,
                            columns: vec![],
                        },
                    ],
                },
                ra_stats::timeline::Snapshot {
                    time_offset: 1800,
                    label: Some("after insert".into()),
                    tables: vec![
                        ra_stats::timeline::TableSnapshot {
                            name: "test_table".into(),
                            row_count: 1500,
                            page_count: Some(15),
                            avg_row_size: Some(100.0),
                            table_size_bytes: None,
                            columns: vec![],
                        },
                    ],
                },
            ],
            events: vec![
                ra_stats::timeline::TimelineEvent {
                    time_offset: 900,
                    kind: ra_stats::timeline::EventKind::Insert,
                    table: "test_table".into(),
                    row_count: Some(500),
                    description: Some(
                        "Batch insert".into(),
                    ),
                },
            ],
            feedback: vec![
                ra_stats::timeline::ExecutionFeedback {
                    time_offset: 300,
                    query: "SELECT * FROM test_table".into(),
                    operator: Some("SeqScan".into()),
                    estimated_rows: 1000.0,
                    actual_rows: 1000.0,
                    estimated_cost: Some(100.0),
                    actual_time_ms: Some(50.0),
                },
            ],
        }
    }
}
