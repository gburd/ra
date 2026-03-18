//! Timeline data model for optimizer snapshots over time.
//!
//! A [`Timeline`] is a sequence of [`Snapshot`]s, each capturing the
//! optimizer state at a point in the optimization process. This is
//! what the TUI plays back and visualizes.

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
