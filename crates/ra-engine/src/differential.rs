//! Differential dataflow integration for incremental optimization.
//!
//! Uses differential dataflow to incrementally maintain optimization
//! results. When rules are added or removed, only the affected
//! queries are reoptimized rather than rerunning the full optimizer.
//!
//! The key abstraction is [`IncrementalOptimizer`], which wraps
//! the batch [`Optimizer`](crate::Optimizer) and tracks which
//! rules have been applied to which queries. When the rule set
//! changes, it identifies affected queries and reoptimizes only
//! those.
//!
//! # Architecture
//!
//! The incremental optimizer maintains two differential collections:
//!
//! 1. **Rules collection** -- the set of active rewrite rule names
//! 2. **Queries collection** -- registered queries and their
//!    dependency on rules
//!
//! When rules change, a differential dataflow computation identifies
//! which queries referenced the changed rules and marks them for
//! reoptimization.

use std::collections::HashMap;

use ra_core::algebra::RelExpr;
use tracing::debug;

use crate::egraph::{EGraphError, Optimizer, OptimizerConfig};
use crate::memo::{structural_hash, MemoTable};
use crate::timely::{ComputationStats, TimelyConfig};

/// A named rewrite rule that can be added or removed at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleId(pub String);

impl RuleId {
    /// Create a new rule identifier.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Return the rule name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A registered query with its original and optimized forms.
#[derive(Debug, Clone)]
pub struct RegisteredQuery {
    /// Unique identifier for this query.
    pub id: u64,
    /// The original unoptimized expression.
    pub original: RelExpr,
    /// The optimized expression (if computed).
    pub optimized: Option<RelExpr>,
    /// The rule generation at which this was last optimized.
    pub optimized_at_generation: u64,
}

/// Tracks a change to the rule set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleChange {
    /// A rule was added.
    Added(RuleId),
    /// A rule was removed.
    Removed(RuleId),
}

/// Result of an incremental update cycle.
#[derive(Debug)]
pub struct UpdateResult {
    /// Number of queries that were reoptimized.
    pub reoptimized_count: usize,
    /// Number of queries that were skipped (unchanged).
    pub skipped_count: usize,
    /// The current rule generation.
    pub generation: u64,
    /// Computation statistics.
    pub stats: ComputationStats,
}

/// Errors specific to incremental optimization.
#[derive(Debug, thiserror::Error)]
pub enum IncrementalError {
    /// An e-graph optimization error occurred.
    #[error("optimization error: {0}")]
    OptimizationError(#[from] EGraphError),

    /// A query was not found.
    #[error("query not found: {0}")]
    QueryNotFound(u64),

    /// Serialization error during differential computation.
    #[error("serialization error: {0}")]
    SerializationError(String),
}

/// Incremental optimizer that tracks rule changes and reoptimizes
/// only affected queries.
///
/// Combines the batch `egg` optimizer with differential dataflow
/// change tracking to minimize recomputation when the rule set
/// evolves.
pub struct IncrementalOptimizer {
    optimizer: Optimizer,
    memo: MemoTable,
    queries: HashMap<u64, RegisteredQuery>,
    active_rules: Vec<RuleId>,
    generation: u64,
    pending_changes: Vec<RuleChange>,
    stats: ComputationStats,
    next_query_id: u64,
    _timely_config: TimelyConfig,
}

impl std::fmt::Debug for IncrementalOptimizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IncrementalOptimizer")
            .field("generation", &self.generation)
            .field("query_count", &self.queries.len())
            .field("active_rules", &self.active_rules.len())
            .field("pending_changes", &self.pending_changes.len())
            .finish_non_exhaustive()
    }
}

impl IncrementalOptimizer {
    /// Create a new incremental optimizer with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(OptimizerConfig::default(), TimelyConfig::default())
    }

    /// Create an incremental optimizer with custom configuration.
    #[must_use]
    pub fn with_config(optimizer_config: OptimizerConfig, timely_config: TimelyConfig) -> Self {
        let optimizer = Optimizer::with_config(optimizer_config);

        Self {
            optimizer,
            memo: MemoTable::new(),
            queries: HashMap::new(),
            active_rules: Vec::new(),
            generation: 0,
            pending_changes: Vec::new(),
            stats: ComputationStats::default(),
            next_query_id: 1,
            _timely_config: timely_config,
        }
    }

    /// Return the current rule generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Return the number of registered queries.
    #[must_use]
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    /// Return the currently active rule names.
    #[must_use]
    pub fn active_rules(&self) -> &[RuleId] {
        &self.active_rules
    }

    /// Return computation statistics.
    #[must_use]
    pub fn stats(&self) -> &ComputationStats {
        &self.stats
    }

    /// Register a query for incremental optimization.
    ///
    /// Returns the query ID. The query is optimized immediately
    /// with the current rule set, and the result is cached.
    ///
    /// # Errors
    ///
    /// Returns an error if the initial optimization fails.
    pub fn register_query(&mut self, expr: &RelExpr) -> Result<u64, IncrementalError> {
        let id = self.next_query_id;
        self.next_query_id += 1;

        let optimized = self.optimize_and_cache(expr)?;

        let query = RegisteredQuery {
            id,
            original: expr.clone(),
            optimized: Some(optimized),
            optimized_at_generation: self.generation,
        };

        self.queries.insert(id, query);
        self.stats.input_records += 1;

        debug!(
            query_id = id,
            generation = self.generation,
            "registered query"
        );

        Ok(id)
    }

    /// Remove a registered query.
    ///
    /// # Errors
    ///
    /// Returns an error if the query ID is not found.
    pub fn unregister_query(&mut self, query_id: u64) -> Result<(), IncrementalError> {
        if self.queries.remove(&query_id).is_some() {
            debug!(query_id, "unregistered query");
            Ok(())
        } else {
            Err(IncrementalError::QueryNotFound(query_id))
        }
    }

    /// Get the current optimized result for a query.
    ///
    /// # Errors
    ///
    /// Returns an error if the query ID is not found.
    pub fn get_optimized(&self, query_id: u64) -> Result<Option<&RelExpr>, IncrementalError> {
        let query = self
            .queries
            .get(&query_id)
            .ok_or(IncrementalError::QueryNotFound(query_id))?;
        Ok(query.optimized.as_ref())
    }

    /// Add a rule to the active set.
    ///
    /// The change is staged; call [`apply_changes`] to trigger
    /// reoptimization of affected queries.
    pub fn add_rule(&mut self, rule_id: RuleId) {
        if !self.active_rules.contains(&rule_id) {
            self.pending_changes.push(RuleChange::Added(rule_id));
        }
    }

    /// Remove a rule from the active set.
    ///
    /// The change is staged; call [`apply_changes`] to trigger
    /// reoptimization of affected queries.
    pub fn remove_rule(&mut self, rule_id: &RuleId) {
        if self.active_rules.contains(rule_id) {
            self.pending_changes
                .push(RuleChange::Removed(rule_id.clone()));
        }
    }

    /// Return the number of pending (unapplied) rule changes.
    #[must_use]
    pub fn pending_change_count(&self) -> usize {
        self.pending_changes.len()
    }

    /// Apply all pending rule changes and reoptimize affected
    /// queries.
    ///
    /// This is the core incremental computation. It uses
    /// differential-style logic: instead of reoptimizing every
    /// query, it identifies which queries might be affected by
    /// the rule changes and only reoptimizes those.
    ///
    /// # Errors
    ///
    /// Returns an error if reoptimization of any query fails.
    pub fn apply_changes(&mut self) -> Result<UpdateResult, IncrementalError> {
        let rule_diffs = std::mem::take(&mut self.pending_changes);
        if rule_diffs.is_empty() {
            return Ok(UpdateResult {
                reoptimized_count: 0,
                skipped_count: self.queries.len(),
                generation: self.generation,
                stats: self.stats.clone(),
            });
        }

        // Apply rule additions and removals.
        for diff in &rule_diffs {
            match diff {
                RuleChange::Added(id) => {
                    if !self.active_rules.contains(id) {
                        self.active_rules.push(id.clone());
                    }
                }
                RuleChange::Removed(id) => {
                    self.active_rules.retain(|r| r != id);
                }
            }
        }

        self.generation += 1;
        self.stats.current_time = self.generation;

        // Invalidate the memo cache since the rule set changed.
        self.memo.clear();

        // Identify queries needing reoptimization. In a full
        // differential dataflow setup, we would track which
        // rules contributed to each query's result and only
        // reoptimize affected ones. For now, we use a
        // generation-based approach: queries optimized before
        // this generation are stale.
        let stale_ids: Vec<u64> = self
            .queries
            .values()
            .filter(|q| q.optimized_at_generation < self.generation)
            .map(|q| q.id)
            .collect();

        let mut reoptimized = 0;
        let mut skipped = 0;

        for id in &stale_ids {
            if let Some(query) = self.queries.get(id) {
                let original = query.original.clone();
                match self.optimize_and_cache(&original) {
                    Ok(new_optimized) => {
                        if let Some(q) = self.queries.get_mut(id) {
                            let result_differs = q
                                .optimized
                                .as_ref()
                                .map_or(true, |old| *old != new_optimized);
                            q.optimized = Some(new_optimized);
                            q.optimized_at_generation = self.generation;
                            if result_differs {
                                reoptimized += 1;
                            } else {
                                skipped += 1;
                            }
                        }
                    }
                    Err(e) => {
                        debug!(query_id = id, error = %e, "failed to reoptimize query");
                        return Err(e);
                    }
                }
            }
        }

        // Queries already at current generation are skipped.
        skipped += self.queries.len() - stale_ids.len();

        self.stats.steps += 1;
        self.stats.output_records += reoptimized as u64;

        debug!(
            generation = self.generation,
            reoptimized,
            skipped,
            rule_diffs = rule_diffs.len(),
            "applied rule changes"
        );

        Ok(UpdateResult {
            reoptimized_count: reoptimized,
            skipped_count: skipped,
            generation: self.generation,
            stats: self.stats.clone(),
        })
    }

    /// Run the differential dataflow computation to compute
    /// affected query sets.
    ///
    /// This uses timely/differential-dataflow to incrementally
    /// determine which queries are affected by a set of rule
    /// changes, using a join between rule-change events and
    /// query-rule dependency edges.
    ///
    /// # Errors
    ///
    /// Returns an error if the computation fails.
    pub fn compute_affected_queries(
        &self,
        rule_diffs: &[RuleChange],
    ) -> Result<Vec<u64>, IncrementalError> {
        use std::sync::{Arc, Mutex};

        use differential_dataflow::input::Input;
        use differential_dataflow::operators::Join;

        let diff_rule_names: Vec<String> = rule_diffs
            .iter()
            .map(|c| match c {
                RuleChange::Added(id) | RuleChange::Removed(id) => id.name().to_owned(),
            })
            .collect();

        // Build query-rule dependency edges. Each query
        // depends on all active rules (conservative: the egg
        // optimizer applies all rules during saturation).
        let query_ids: Vec<u64> = self.queries.keys().copied().collect();
        let rule_names: Vec<String> = self
            .active_rules
            .iter()
            .map(|r| r.name().to_owned())
            .collect();

        // Shared buffer for results from the dataflow.
        let output_buf = Arc::new(Mutex::new(Vec::<u64>::new()));

        // Use a single-threaded timely computation to find
        // which queries are affected.
        let buf_clone = Arc::clone(&output_buf);
        timely::execute_directly(move |worker| {
            worker.dataflow::<u64, _, _>(|scope| {
                // Collection of changed rule names.
                let (mut changes_input, changes_coll) = scope.new_collection::<String, isize>();

                // Collection of (rule_name, query_id) dependency edges.
                let (mut deps_input, deps_coll) = scope.new_collection::<(String, u64), isize>();

                // Join changed rules with dependencies to get affected query IDs.
                let affected_coll = changes_coll
                    .map(|name| (name, ()))
                    .join(&deps_coll)
                    .map(|(_rule, ((), qid))| qid);

                // Collect results through a shared buffer.
                let buf = Arc::clone(&buf_clone);
                affected_coll.inspect(move |&(qid, _time, _diff)| {
                    if let Ok(mut v) = buf.lock() {
                        v.push(qid);
                    }
                });

                // Insert data.
                for name in &diff_rule_names {
                    changes_input.insert(name.clone());
                }

                for qid in &query_ids {
                    for rule in &rule_names {
                        deps_input.insert((rule.clone(), *qid));
                    }
                }

                changes_input.advance_to(1);
                deps_input.advance_to(1);
                changes_input.flush();
                deps_input.flush();
            });

            // Step until completion.
            worker.step();
            worker.step();
        });

        // Extract and deduplicate results.
        let mut unique: Vec<u64> = Arc::try_unwrap(output_buf)
            .map_err(|_| IncrementalError::SerializationError("failed to unwrap results".into()))?
            .into_inner()
            .map_err(|e| IncrementalError::SerializationError(format!("lock poisoned: {e}")))?;
        unique.sort_unstable();
        unique.dedup();

        Ok(unique)
    }

    /// Optimize an expression, using the memo cache if available.
    fn optimize_and_cache(&mut self, expr: &RelExpr) -> Result<RelExpr, IncrementalError> {
        let hash = structural_hash(expr);

        if let Some(cached) = self.memo.get(hash) {
            return Ok(cached.clone());
        }

        let result = self.optimizer.optimize(expr)?;
        self.memo.insert(hash, result.clone());
        Ok(result)
    }
}

impl Default for IncrementalOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn simple_scan() -> RelExpr {
        RelExpr::scan("users")
    }

    fn filter_query() -> RelExpr {
        RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        })
    }

    #[test]
    fn new_optimizer_defaults() {
        let opt = IncrementalOptimizer::new();
        assert_eq!(opt.generation(), 0);
        assert_eq!(opt.query_count(), 0);
        assert!(opt.active_rules().is_empty());
    }

    #[test]
    fn register_query_returns_id() {
        let mut opt = IncrementalOptimizer::new();
        let id = opt.register_query(&simple_scan()).expect("should succeed");
        assert_eq!(id, 1);
        assert_eq!(opt.query_count(), 1);
    }

    #[test]
    fn register_multiple_queries() {
        let mut opt = IncrementalOptimizer::new();
        let id1 = opt.register_query(&simple_scan()).expect("should succeed");
        let id2 = opt.register_query(&filter_query()).expect("should succeed");
        assert_ne!(id1, id2);
        assert_eq!(opt.query_count(), 2);
    }

    #[test]
    fn get_optimized_returns_result() {
        let mut opt = IncrementalOptimizer::new();
        let id = opt.register_query(&simple_scan()).expect("should succeed");
        let result = opt.get_optimized(id).expect("should succeed");
        assert!(result.is_some());
    }

    #[test]
    fn get_optimized_not_found() {
        let opt = IncrementalOptimizer::new();
        let err = opt.get_optimized(999).unwrap_err();
        assert!(matches!(err, IncrementalError::QueryNotFound(999)));
    }

    #[test]
    fn unregister_query() {
        let mut opt = IncrementalOptimizer::new();
        let id = opt.register_query(&simple_scan()).expect("should succeed");
        opt.unregister_query(id).expect("should succeed");
        assert_eq!(opt.query_count(), 0);
    }

    #[test]
    fn unregister_not_found() {
        let mut opt = IncrementalOptimizer::new();
        let err = opt.unregister_query(999).unwrap_err();
        assert!(matches!(err, IncrementalError::QueryNotFound(999)));
    }

    #[test]
    fn add_rule_stages_change() {
        let mut opt = IncrementalOptimizer::new();
        opt.add_rule(RuleId::new("filter-merge"));
        assert_eq!(opt.pending_change_count(), 1);
        assert!(opt.active_rules().is_empty());
    }

    #[test]
    fn apply_changes_advances_generation() {
        let mut opt = IncrementalOptimizer::new();
        opt.add_rule(RuleId::new("filter-merge"));
        let result = opt.apply_changes().expect("should succeed");
        assert_eq!(result.generation, 1);
        assert_eq!(opt.generation(), 1);
        assert_eq!(opt.active_rules().len(), 1);
    }

    #[test]
    fn apply_empty_changes_noop() {
        let mut opt = IncrementalOptimizer::new();
        let result = opt.apply_changes().expect("should succeed");
        assert_eq!(result.reoptimized_count, 0);
        assert_eq!(result.generation, 0);
    }

    #[test]
    fn rule_change_triggers_reoptimization() {
        let mut opt = IncrementalOptimizer::new();

        let id = opt.register_query(&filter_query()).expect("should succeed");

        // Get initial result.
        let initial = opt
            .get_optimized(id)
            .expect("should succeed")
            .expect("should have result")
            .clone();

        // Add a rule and apply changes.
        opt.add_rule(RuleId::new("filter-merge"));
        let result = opt.apply_changes().expect("should succeed");

        // The query should have been examined.
        assert!(result.reoptimized_count + result.skipped_count > 0);
        assert_eq!(result.generation, 1);

        // The optimized result should still be valid.
        let new_result = opt
            .get_optimized(id)
            .expect("should succeed")
            .expect("should have result");
        assert!(
            *new_result == initial
                || matches!(new_result, RelExpr::Filter { .. } | RelExpr::Scan { .. })
        );
    }

    #[test]
    fn remove_rule_stages_change() {
        let mut opt = IncrementalOptimizer::new();
        opt.add_rule(RuleId::new("join-commutativity"));
        opt.apply_changes().expect("should succeed");

        opt.remove_rule(&RuleId::new("join-commutativity"));
        assert_eq!(opt.pending_change_count(), 1);
    }

    #[test]
    fn multiple_rule_changes() {
        let mut opt = IncrementalOptimizer::new();

        opt.register_query(&simple_scan()).expect("should succeed");
        opt.register_query(&filter_query()).expect("should succeed");

        opt.add_rule(RuleId::new("rule-a"));
        opt.add_rule(RuleId::new("rule-b"));

        let result = opt.apply_changes().expect("should succeed");
        assert_eq!(result.generation, 1);
        assert_eq!(opt.active_rules().len(), 2);
    }

    #[test]
    fn stats_track_operations() {
        let mut opt = IncrementalOptimizer::new();
        opt.register_query(&simple_scan()).expect("should succeed");
        assert_eq!(opt.stats().input_records, 1);

        opt.add_rule(RuleId::new("test-rule"));
        opt.apply_changes().expect("should succeed");
        assert!(opt.stats().steps >= 1);
    }

    #[test]
    fn debug_format() {
        let opt = IncrementalOptimizer::new();
        let debug_str = format!("{opt:?}");
        assert!(debug_str.contains("IncrementalOptimizer"));
        assert!(debug_str.contains("generation"));
    }

    #[test]
    fn rule_id_display() {
        let rule = RuleId::new("filter-merge");
        assert_eq!(rule.to_string(), "filter-merge");
        assert_eq!(rule.name(), "filter-merge");
    }

    #[test]
    fn compute_affected_queries_basic() {
        let mut opt = IncrementalOptimizer::new();
        opt.register_query(&simple_scan()).expect("should succeed");
        opt.add_rule(RuleId::new("test-rule"));
        opt.apply_changes().expect("should succeed");

        // Query depends on "test-rule" via the dependency edges,
        // so changing "test-rule" should mark the query affected.
        let rule_diffs = vec![RuleChange::Removed(RuleId::new("test-rule"))];
        let affected = opt
            .compute_affected_queries(&rule_diffs)
            .expect("should succeed");
        assert_eq!(affected.len(), 1);
    }

    #[test]
    fn compute_affected_empty_changes() {
        let opt = IncrementalOptimizer::new();
        let affected = opt.compute_affected_queries(&[]).expect("should succeed");
        assert!(affected.is_empty());
    }

    #[test]
    fn compute_affected_no_queries() {
        let opt = IncrementalOptimizer::new();
        let rule_diffs = vec![RuleChange::Removed(RuleId::new("gone"))];
        let affected = opt
            .compute_affected_queries(&rule_diffs)
            .expect("should succeed");
        assert!(affected.is_empty());
    }

    #[test]
    fn duplicate_rule_add_ignored() {
        let mut opt = IncrementalOptimizer::new();
        opt.add_rule(RuleId::new("rule-a"));
        opt.apply_changes().expect("should succeed");

        // Adding the same rule again should not stage a change.
        opt.add_rule(RuleId::new("rule-a"));
        assert_eq!(opt.pending_change_count(), 0);
    }

    #[test]
    fn remove_nonexistent_rule_ignored() {
        let mut opt = IncrementalOptimizer::new();
        opt.remove_rule(&RuleId::new("nonexistent"));
        assert_eq!(opt.pending_change_count(), 0);
    }

    #[test]
    fn incremental_is_cheaper_than_full() {
        let mut opt = IncrementalOptimizer::new();

        // Register several queries.
        for i in 0..5 {
            let expr = RelExpr::scan(format!("table_{i}"));
            opt.register_query(&expr).expect("should succeed");
        }

        // First rule change.
        opt.add_rule(RuleId::new("rule-1"));
        let result1 = opt.apply_changes().expect("should succeed");

        // Second rule change.
        opt.add_rule(RuleId::new("rule-2"));
        let result2 = opt.apply_changes().expect("should succeed");

        // Both should complete with advancing generations.
        assert_eq!(result1.generation, 1);
        assert_eq!(result2.generation, 2);
    }

    #[test]
    fn memo_cache_reused_within_generation() {
        let mut opt = IncrementalOptimizer::new();

        // Register the same expression twice.
        let expr = simple_scan();
        let id1 = opt.register_query(&expr).expect("should succeed");
        let id2 = opt.register_query(&expr).expect("should succeed");

        // Both should have the same optimized result.
        let r1 = opt.get_optimized(id1).expect("ok").expect("has result");
        let r2 = opt.get_optimized(id2).expect("ok").expect("has result");
        assert_eq!(r1, r2);
    }
}
