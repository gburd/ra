//! Cached plan entry.

use std::collections::HashMap;
use std::time::Instant;

use ra_core::algebra::RelExpr;
use ra_core::cost::Cost;
use ra_core::statistics::Statistics;

/// A cached optimized plan with its metadata.
#[derive(Debug, Clone)]
pub struct CachedPlan {
    /// The optimized relational expression.
    pub plan: RelExpr,
    /// The estimated cost at optimization time.
    pub cost: Cost,
    /// Statistics snapshot for referenced tables at optimization time.
    pub statistics_snapshot: HashMap<String, Statistics>,
    /// The original SQL text used to produce this plan.
    pub original_sql: String,
    /// When this plan was first cached.
    pub created_at: Instant,
    /// When this plan was last optimized (or reoptimized).
    pub optimized_at: Instant,
    /// When this plan was last accessed (for LRU).
    pub last_accessed: Instant,
    /// Number of times this plan has been retrieved (for LFU).
    pub use_count: u64,
    /// Number of times this plan has been reoptimized.
    pub reoptimization_count: u32,
}

impl CachedPlan {
    /// Create a new cached plan entry.
    #[must_use]
    pub fn new(
        plan: RelExpr,
        cost: Cost,
        statistics_snapshot: HashMap<String, Statistics>,
        original_sql: String,
    ) -> Self {
        let now = Instant::now();
        Self {
            plan,
            cost,
            statistics_snapshot,
            original_sql,
            created_at: now,
            optimized_at: now,
            last_accessed: now,
            use_count: 0,
            reoptimization_count: 0,
        }
    }

    /// Check whether this plan references a table by name.
    #[must_use]
    pub fn references_table(&self, table: &str) -> bool {
        self.statistics_snapshot.contains_key(table)
            || plan_scans_table(&self.plan, table)
    }

    /// Return the list of table names referenced by this plan.
    #[must_use]
    pub fn referenced_tables(&self) -> Vec<String> {
        let mut tables: Vec<String> =
            self.statistics_snapshot.keys().cloned().collect();
        tables.sort();
        tables.dedup();
        tables
    }

    /// Age since creation.
    #[must_use]
    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Time since last optimization.
    #[must_use]
    pub fn staleness(&self) -> std::time::Duration {
        self.optimized_at.elapsed()
    }
}

/// Walk the plan tree to check if it scans a specific table.
fn plan_scans_table(plan: &RelExpr, target: &str) -> bool {
    match plan {
        RelExpr::Scan { table, .. } => table == target,
        other => other
            .children()
            .iter()
            .any(|child| plan_scans_table(child, target)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan(table: &str) -> CachedPlan {
        let mut snapshot = HashMap::new();
        snapshot
            .insert(table.to_owned(), Statistics::new(1000.0));
        CachedPlan::new(
            RelExpr::scan(table),
            Cost::ZERO,
            snapshot,
            format!("SELECT * FROM {table}"),
        )
    }

    #[test]
    fn references_table_true() {
        let plan = make_plan("users");
        assert!(plan.references_table("users"));
    }

    #[test]
    fn references_table_false() {
        let plan = make_plan("users");
        assert!(!plan.references_table("orders"));
    }

    #[test]
    fn referenced_tables_sorted() {
        let mut snapshot = HashMap::new();
        snapshot
            .insert("orders".to_owned(), Statistics::new(500.0));
        snapshot
            .insert("users".to_owned(), Statistics::new(1000.0));
        let plan = CachedPlan::new(
            RelExpr::scan("users"),
            Cost::ZERO,
            snapshot,
            "SELECT * FROM users JOIN orders".to_owned(),
        );
        let tables = plan.referenced_tables();
        assert_eq!(tables, vec!["orders", "users"]);
    }

    #[test]
    fn use_count_starts_zero() {
        let plan = make_plan("t");
        assert_eq!(plan.use_count, 0);
    }

    #[test]
    fn reoptimization_count_starts_zero() {
        let plan = make_plan("t");
        assert_eq!(plan.reoptimization_count, 0);
    }
}
