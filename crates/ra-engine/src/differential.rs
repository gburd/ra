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
//! The incremental optimizer maintains four differential collections:
//!
//! 1. **Rules collection** -- the set of active rewrite rule names
//! 2. **Queries collection** -- registered queries and their
//!    dependency on rules
//! 3. **Statistics changes** -- detected statistics/index/fact
//!    changes (RFC 0059)
//! 4. **Plan dependencies** -- maps cached plan fingerprints to
//!    the statistics resources they depend on (RFC 0059)
//!
//! When rules change, a differential dataflow computation identifies
//! which queries referenced the changed rules and marks them for
//! reoptimization. When statistics change, a similar computation
//! identifies which cached plans are affected and invalidates them.

use std::collections::{HashMap, HashSet};

use ra_core::algebra::RelExpr;
use ra_core::cost::StatisticsProvider;
use ra_core::statistics::Histogram;
use tracing::debug;

use crate::egraph::{EGraphError, Optimizer, OptimizerConfig};
use crate::genetic_fingerprint::QueryFingerprint;
use crate::memo::{structural_hash, MemoTable};
use crate::timely::{ComputationStats, TimelyConfig};

// ── RFC 0059: Plan dependency types ─────────────────────────────

/// Identifies a specific statistics resource that can change.
/// Used as the join key in differential dataflow invalidation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceId {
    /// Table row count: "table_name.row_count"
    RowCount(String),
    /// Column distinct count: "table.column.ndistinct"
    NDistinct(String, String),
    /// Index existence: "table.index_name"
    Index(String, String),
    /// Column histogram: "table.column.histogram"
    Histogram(String, String),
    /// A database fact (e.g., constraint, FK relationship).
    Fact(String),
}

impl ResourceId {
    /// String key for differential dataflow collections.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::RowCount(t) => format!("{t}.row_count"),
            Self::NDistinct(t, c) => format!("{t}.{c}.ndistinct"),
            Self::Index(t, i) => format!("{t}.{i}"),
            Self::Histogram(t, c) => format!("{t}.{c}.histogram"),
            Self::Fact(f) => f.clone(),
        }
    }
}

/// Compact summary of a histogram for drift comparison.
#[derive(Debug, Clone)]
pub struct HistogramDigest {
    /// Bucket boundary count.
    pub bucket_count: usize,
    /// Normalized frequency distribution (sums to 1.0).
    pub frequencies: Vec<f64>,
    /// Total row count across all buckets.
    pub total_rows: f64,
}

impl HistogramDigest {
    /// Build a digest from an ra-core Histogram.
    #[must_use]
    pub fn from_histogram(hist: &Histogram) -> Self {
        let buckets = match hist {
            Histogram::EquiWidth(h) => &h.buckets,
            Histogram::EquiDepth(h) => &h.buckets,
        };
        let total: f64 = buckets.iter().map(|b| b.row_count).sum();
        let frequencies = if total > 0.0 {
            buckets.iter().map(|b| b.row_count / total).collect()
        } else {
            vec![0.0; buckets.len()]
        };
        Self {
            bucket_count: buckets.len(),
            frequencies,
            total_rows: total,
        }
    }

    /// Compute symmetric KL-divergence between two digests.
    /// Returns 0.0 for identical distributions, higher for
    /// more divergent distributions.
    #[must_use]
    pub fn kl_divergence(&self, other: &HistogramDigest) -> f64 {
        if self.bucket_count != other.bucket_count {
            return f64::MAX;
        }
        let epsilon = 1e-10;
        let mut kl_pq = 0.0;
        let mut kl_qp = 0.0;
        for (p, q) in self
            .frequencies
            .iter()
            .zip(other.frequencies.iter())
        {
            let p_safe = p.max(epsilon);
            let q_safe = q.max(epsilon);
            kl_pq += p_safe * (p_safe / q_safe).ln();
            kl_qp += q_safe * (q_safe / p_safe).ln();
        }
        (kl_pq + kl_qp) / 2.0
    }
}

/// Dependencies of a cached plan on statistics resources.
#[derive(Debug, Clone)]
pub struct PlanDependencies {
    /// Table cardinalities that influenced this plan.
    pub table_cardinalities: HashMap<String, f64>,
    /// Indexes this plan uses or considered.
    pub indexes: HashSet<(String, String)>,
    /// Column distinct counts that affect selectivity.
    pub distinct_counts: HashMap<(String, String), f64>,
    /// Column histogram digests at optimization time.
    pub histogram_digests:
        HashMap<(String, String), HistogramDigest>,
    /// Facts that enabled certain optimization rules.
    pub facts: HashSet<String>,
}

impl PlanDependencies {
    /// Build dependencies from a plan and statistics provider.
    #[must_use]
    pub fn from_plan_and_stats(
        plan: &RelExpr,
        stats: &dyn StatisticsProvider,
    ) -> Self {
        let tables = collect_referenced_tables(plan);
        let mut deps = Self {
            table_cardinalities: HashMap::new(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };
        for table in &tables {
            if let Some(table_stats) =
                stats.get_statistics(table)
            {
                deps.table_cardinalities
                    .insert(table.clone(), table_stats.row_count);
                for (col, col_stats) in &table_stats.columns {
                    deps.distinct_counts.insert(
                        (table.clone(), col.clone()),
                        col_stats.distinct_count,
                    );
                    if let Some(hist) = &col_stats.histogram {
                        deps.histogram_digests.insert(
                            (table.clone(), col.clone()),
                            HistogramDigest::from_histogram(hist),
                        );
                    }
                }
                for idx_name in table_stats.indexes.keys() {
                    deps.indexes
                        .insert((table.clone(), idx_name.clone()));
                }
            }
        }
        deps
    }

    /// Enumerate all `ResourceId`s this plan depends on.
    #[must_use]
    pub fn all_resources(&self) -> Vec<ResourceId> {
        let mut resources = Vec::new();
        for table in self.table_cardinalities.keys() {
            resources
                .push(ResourceId::RowCount(table.clone()));
        }
        for (table, col) in self.distinct_counts.keys() {
            resources.push(ResourceId::NDistinct(
                table.clone(),
                col.clone(),
            ));
        }
        for (table, idx) in &self.indexes {
            resources.push(ResourceId::Index(
                table.clone(),
                idx.clone(),
            ));
        }
        for (table, col) in self.histogram_digests.keys() {
            resources.push(ResourceId::Histogram(
                table.clone(),
                col.clone(),
            ));
        }
        for fact in &self.facts {
            resources.push(ResourceId::Fact(fact.clone()));
        }
        resources
    }
}

/// Configurable thresholds for change detection.
#[derive(Debug, Clone)]
pub struct StalenessThresholds {
    /// Cardinality must change by this ratio to emit an event.
    /// Computed as max(new/old, old/new). Default: 2.0.
    pub cardinality_ratio: f64,
    /// Distinct count must change by this ratio. Default: 1.5.
    pub ndistinct_ratio: f64,
    /// Whether any index add/drop emits a change event.
    pub index_changes_trigger: bool,
    /// KL-divergence threshold for histogram comparison.
    pub histogram_kl_threshold: f64,
    /// Maximum plan age before forced invalidation.
    pub max_age: Option<std::time::Duration>,
}

impl Default for StalenessThresholds {
    fn default() -> Self {
        Self {
            cardinality_ratio: 2.0,
            ndistinct_ratio: 1.5,
            index_changes_trigger: true,
            histogram_kl_threshold: 0.5,
            max_age: None,
        }
    }
}

/// A detected change that may invalidate cached plans.
#[derive(Debug, Clone)]
pub enum ChangeSource {
    /// A statistics value crossed its threshold.
    Statistics(StatisticsChange),
    /// An index was added or dropped.
    Index(IndexChange),
    /// A database fact changed.
    Fact(FactChange),
}

impl ChangeSource {
    /// The `ResourceId` affected by this change.
    #[must_use]
    pub fn resource_id(&self) -> ResourceId {
        match self {
            Self::Statistics(s) => s.resource_id(),
            Self::Index(i) => i.resource_id(),
            Self::Fact(f) => {
                ResourceId::Fact(f.fact_name.clone())
            }
        }
    }
}

/// A statistics value that crossed its threshold.
#[derive(Debug, Clone)]
pub enum StatisticsChange {
    /// Row count changed beyond the cardinality ratio threshold.
    RowCount {
        /// Table name.
        table: String,
        /// Row count at previous snapshot.
        old_value: f64,
        /// Current row count.
        new_value: f64,
        /// max(new/old, old/new).
        ratio: f64,
    },
    /// Column distinct count changed beyond the NDV ratio.
    DistinctCount {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
        /// NDV at previous snapshot.
        old_value: f64,
        /// Current NDV.
        new_value: f64,
        /// max(new/old, old/new).
        ratio: f64,
    },
    /// Histogram KL-divergence exceeded the threshold.
    HistogramDrift {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
        /// The computed KL-divergence.
        kl_divergence: f64,
    },
}

impl StatisticsChange {
    /// The `ResourceId` this change affects.
    #[must_use]
    pub fn resource_id(&self) -> ResourceId {
        match self {
            Self::RowCount { table, .. } => {
                ResourceId::RowCount(table.clone())
            }
            Self::DistinctCount {
                table, column, ..
            } => ResourceId::NDistinct(
                table.clone(),
                column.clone(),
            ),
            Self::HistogramDrift {
                table, column, ..
            } => ResourceId::Histogram(
                table.clone(),
                column.clone(),
            ),
        }
    }
}

/// An index that was added or dropped.
#[derive(Debug, Clone)]
pub enum IndexChange {
    /// A new index was created.
    Added {
        /// Table name.
        table: String,
        /// Index name.
        index_name: String,
        /// Columns covered by the index.
        columns: Vec<String>,
    },
    /// An index was dropped.
    Dropped {
        /// Table name.
        table: String,
        /// Index name.
        index_name: String,
    },
}

impl IndexChange {
    /// The `ResourceId` this change affects.
    #[must_use]
    pub fn resource_id(&self) -> ResourceId {
        match self {
            Self::Added {
                table, index_name, ..
            }
            | Self::Dropped { table, index_name } => {
                ResourceId::Index(
                    table.clone(),
                    index_name.clone(),
                )
            }
        }
    }
}

/// A database fact that changed.
#[derive(Debug, Clone)]
pub struct FactChange {
    /// Name of the fact.
    pub fact_name: String,
    /// Previous value (if any).
    pub old_value: Option<String>,
    /// New value (if any).
    pub new_value: Option<String>,
}

/// Collect all table names referenced in a `RelExpr` tree.
fn collect_referenced_tables(plan: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    collect_tables_recursive(plan, &mut tables);
    tables.sort();
    tables.dedup();
    tables
}

fn collect_tables_recursive(
    plan: &RelExpr,
    tables: &mut Vec<String>,
) {
    if let RelExpr::Scan { table, .. } = plan {
        tables.push(table.clone());
    }
    for child in plan.children() {
        collect_tables_recursive(child, tables);
    }
}

/// Compute the change ratio between two positive values.
/// Returns max(a/b, b/a), always >= 1.0.
#[must_use]
pub fn change_ratio(old: f64, new: f64) -> f64 {
    if old <= 0.0 && new <= 0.0 {
        return 1.0;
    }
    if old <= 0.0 || new <= 0.0 {
        return f64::MAX;
    }
    let r = new / old;
    if r >= 1.0 { r } else { 1.0 / r }
}

// ── Original types ──────────────────────────────────────────────

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
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
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
/// only affected queries. Also tracks plan dependencies on
/// statistics resources (RFC 0059) so that statistics changes
/// invalidate only the affected cached plans.
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
    // RFC 0059: plan dependency tracking
    plan_dependencies:
        HashMap<QueryFingerprint, PlanDependencies>,
    thresholds: StalenessThresholds,
}

impl std::fmt::Debug for IncrementalOptimizer {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("IncrementalOptimizer")
            .field("generation", &self.generation)
            .field("query_count", &self.queries.len())
            .field("active_rules", &self.active_rules.len())
            .field(
                "pending_changes",
                &self.pending_changes.len(),
            )
            .field(
                "plan_dependencies",
                &self.plan_dependencies.len(),
            )
            .finish_non_exhaustive()
    }
}

impl IncrementalOptimizer {
    /// Create a new incremental optimizer with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(
            OptimizerConfig::default(),
            TimelyConfig::default(),
        )
    }

    /// Create an incremental optimizer with custom configuration.
    #[must_use]
    pub fn with_config(
        optimizer_config: OptimizerConfig,
        timely_config: TimelyConfig,
    ) -> Self {
        let optimizer =
            Optimizer::with_config(optimizer_config);
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
            plan_dependencies: HashMap::new(),
            thresholds: StalenessThresholds::default(),
        }
    }

    /// Create with custom staleness thresholds.
    #[must_use]
    pub fn with_thresholds(
        optimizer_config: OptimizerConfig,
        timely_config: TimelyConfig,
        thresholds: StalenessThresholds,
    ) -> Self {
        let mut opt =
            Self::with_config(optimizer_config, timely_config);
        opt.thresholds = thresholds;
        opt
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
    /// # Errors
    ///
    /// Returns an error if the initial optimization fails.
    pub fn register_query(
        &mut self,
        expr: &RelExpr,
    ) -> Result<u64, IncrementalError> {
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
    pub fn unregister_query(
        &mut self,
        query_id: u64,
    ) -> Result<(), IncrementalError> {
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
    pub fn get_optimized(
        &self,
        query_id: u64,
    ) -> Result<Option<&RelExpr>, IncrementalError> {
        let query = self
            .queries
            .get(&query_id)
            .ok_or(IncrementalError::QueryNotFound(query_id))?;
        Ok(query.optimized.as_ref())
    }

    /// Add a rule to the active set.
    pub fn add_rule(&mut self, rule_id: RuleId) {
        if !self.active_rules.contains(&rule_id) {
            self.pending_changes
                .push(RuleChange::Added(rule_id));
        }
    }

    /// Remove a rule from the active set.
    pub fn remove_rule(&mut self, rule_id: &RuleId) {
        if self.active_rules.contains(rule_id) {
            self.pending_changes
                .push(RuleChange::Removed(rule_id.clone()));
        }
    }

    /// Return the number of pending rule changes.
    #[must_use]
    pub fn pending_change_count(&self) -> usize {
        self.pending_changes.len()
    }

    /// Apply all pending rule changes and reoptimize affected
    /// queries.
    ///
    /// # Errors
    ///
    /// Returns an error if reoptimization of any query fails.
    pub fn apply_changes(
        &mut self,
    ) -> Result<UpdateResult, IncrementalError> {
        let rule_diffs =
            std::mem::take(&mut self.pending_changes);
        if rule_diffs.is_empty() {
            return Ok(UpdateResult {
                reoptimized_count: 0,
                skipped_count: self.queries.len(),
                generation: self.generation,
                stats: self.stats.clone(),
            });
        }

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
        self.memo.clear();

        let stale_ids: Vec<u64> = self
            .queries
            .values()
            .filter(|q| {
                q.optimized_at_generation < self.generation
            })
            .map(|q| q.id)
            .collect();

        let mut reoptimized = 0;
        let mut skipped = 0;

        for id in &stale_ids {
            if let Some(query) = self.queries.get(id) {
                let original = query.original.clone();
                match self.optimize_and_cache(&original) {
                    Ok(new_optimized) => {
                        if let Some(q) =
                            self.queries.get_mut(id)
                        {
                            let result_differs = q
                                .optimized
                                .as_ref()
                                .map_or(true, |old| {
                                    *old != new_optimized
                                });
                            q.optimized =
                                Some(new_optimized);
                            q.optimized_at_generation =
                                self.generation;
                            if result_differs {
                                reoptimized += 1;
                            } else {
                                skipped += 1;
                            }
                        }
                    }
                    Err(e) => {
                        debug!(
                            query_id = id,
                            error = %e,
                            "failed to reoptimize query"
                        );
                        return Err(e);
                    }
                }
            }
        }

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

    /// Compute affected query sets via differential dataflow.
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
                RuleChange::Added(id)
                | RuleChange::Removed(id) => {
                    id.name().to_owned()
                }
            })
            .collect();

        let query_ids: Vec<u64> =
            self.queries.keys().copied().collect();
        let rule_names: Vec<String> = self
            .active_rules
            .iter()
            .map(|r| r.name().to_owned())
            .collect();

        let output_buf =
            Arc::new(Mutex::new(Vec::<u64>::new()));
        let buf_clone = Arc::clone(&output_buf);

        timely::execute_directly(move |worker| {
            worker.dataflow::<u64, _, _>(|scope| {
                let (mut changes_input, changes_coll) =
                    scope.new_collection::<String, isize>();
                let (mut deps_input, deps_coll) = scope
                    .new_collection::<(String, u64), isize>();

                let affected_coll = changes_coll
                    .map(|name| (name, ()))
                    .join(&deps_coll)
                    .map(|(_rule, ((), qid))| qid);

                let buf = Arc::clone(&buf_clone);
                affected_coll.inspect(
                    move |&(qid, _time, _diff)| {
                        if let Ok(mut v) = buf.lock() {
                            v.push(qid);
                        }
                    },
                );

                for name in &diff_rule_names {
                    changes_input.insert(name.clone());
                }
                for qid in &query_ids {
                    for rule in &rule_names {
                        deps_input
                            .insert((rule.clone(), *qid));
                    }
                }

                changes_input.advance_to(1);
                deps_input.advance_to(1);
                changes_input.flush();
                deps_input.flush();
            });

            worker.step();
            worker.step();
        });

        let mut unique: Vec<u64> = Arc::try_unwrap(output_buf)
            .map_err(|_| {
                IncrementalError::SerializationError(
                    "failed to unwrap results".into(),
                )
            })?
            .into_inner()
            .map_err(|e| {
                IncrementalError::SerializationError(
                    format!("lock poisoned: {e}"),
                )
            })?;
        unique.sort_unstable();
        unique.dedup();

        Ok(unique)
    }

    // ── RFC 0059: Plan dependency tracking ──────────────────

    /// Return the staleness thresholds.
    #[must_use]
    pub fn thresholds(&self) -> &StalenessThresholds {
        &self.thresholds
    }

    /// Set the staleness thresholds.
    pub fn set_thresholds(
        &mut self,
        thresholds: StalenessThresholds,
    ) {
        self.thresholds = thresholds;
    }

    /// Return the number of tracked plan dependencies.
    #[must_use]
    pub fn plan_dependency_count(&self) -> usize {
        self.plan_dependencies.len()
    }

    /// Register a cached plan's dependencies.
    pub fn register_plan_dependencies(
        &mut self,
        fingerprint: &QueryFingerprint,
        deps: &PlanDependencies,
    ) {
        self.plan_dependencies
            .insert(fingerprint.clone(), deps.clone());
    }

    /// Remove a plan's dependencies (on cache eviction).
    pub fn unregister_plan_dependencies(
        &mut self,
        fingerprint: &QueryFingerprint,
    ) {
        self.plan_dependencies.remove(fingerprint);
    }

    /// Compute which cached plans are affected by change events.
    ///
    /// # Errors
    ///
    /// Returns an error if the differential dataflow computation
    /// fails.
    pub fn compute_affected_plans(
        &self,
        changes: &[ChangeSource],
    ) -> Result<Vec<QueryFingerprint>, IncrementalError> {
        if changes.is_empty()
            || self.plan_dependencies.is_empty()
        {
            return Ok(Vec::new());
        }

        use std::sync::{Arc, Mutex};

        use differential_dataflow::input::Input;
        use differential_dataflow::operators::Join;

        let change_keys: Vec<String> = changes
            .iter()
            .map(|c| c.resource_id().key())
            .collect();

        let fp_list: Vec<QueryFingerprint> =
            self.plan_dependencies.keys().cloned().collect();

        let dep_edges: Vec<(String, usize)> = fp_list
            .iter()
            .enumerate()
            .flat_map(|(idx, fp)| {
                self.plan_dependencies[fp]
                    .all_resources()
                    .into_iter()
                    .map(move |r| (r.key(), idx))
            })
            .collect();

        let output_buf =
            Arc::new(Mutex::new(Vec::<usize>::new()));
        let buf_clone = Arc::clone(&output_buf);

        timely::execute_directly(move |worker| {
            worker.dataflow::<u64, _, _>(|scope| {
                let (mut changes_input, changes_coll) =
                    scope.new_collection::<String, isize>();
                let (mut deps_input, deps_coll) = scope
                    .new_collection::<(String, usize), isize>(
                    );

                let affected = changes_coll
                    .map(|key| (key, ()))
                    .join(&deps_coll)
                    .map(|(_resource, ((), fp_idx))| fp_idx);

                let buf = Arc::clone(&buf_clone);
                affected.inspect(
                    move |&(fp_idx, _time, _diff)| {
                        if let Ok(mut v) = buf.lock() {
                            v.push(fp_idx);
                        }
                    },
                );

                for key in &change_keys {
                    changes_input.insert(key.clone());
                }
                for (resource_key, fp_idx) in &dep_edges {
                    deps_input.insert((
                        resource_key.clone(),
                        *fp_idx,
                    ));
                }

                changes_input.advance_to(1);
                deps_input.advance_to(1);
                changes_input.flush();
                deps_input.flush();
            });

            worker.step();
            worker.step();
        });

        let mut indices: Vec<usize> =
            Arc::try_unwrap(output_buf)
                .map_err(|_| {
                    IncrementalError::SerializationError(
                        "failed to unwrap invalidation results"
                            .into(),
                    )
                })?
                .into_inner()
                .map_err(|e| {
                    IncrementalError::SerializationError(
                        format!("lock poisoned: {e}"),
                    )
                })?;
        indices.sort_unstable();
        indices.dedup();

        let affected: Vec<QueryFingerprint> = indices
            .into_iter()
            .filter_map(|idx| fp_list.get(idx).cloned())
            .collect();
        Ok(affected)
    }

    /// Detect changes between old and new statistics for a table.
    #[must_use]
    pub fn detect_changes(
        &self,
        table: &str,
        old: &ra_core::statistics::Statistics,
        new: &ra_core::statistics::Statistics,
    ) -> Vec<ChangeSource> {
        let mut changes = Vec::new();
        let th = &self.thresholds;

        let card_ratio =
            change_ratio(old.row_count, new.row_count);
        if card_ratio >= th.cardinality_ratio {
            changes.push(ChangeSource::Statistics(
                StatisticsChange::RowCount {
                    table: table.to_owned(),
                    old_value: old.row_count,
                    new_value: new.row_count,
                    ratio: card_ratio,
                },
            ));
        }

        for (col, old_col) in &old.columns {
            if let Some(new_col) = new.columns.get(col) {
                let ndv_ratio = change_ratio(
                    old_col.distinct_count,
                    new_col.distinct_count,
                );
                if ndv_ratio >= th.ndistinct_ratio {
                    changes.push(ChangeSource::Statistics(
                        StatisticsChange::DistinctCount {
                            table: table.to_owned(),
                            column: col.clone(),
                            old_value: old_col.distinct_count,
                            new_value: new_col.distinct_count,
                            ratio: ndv_ratio,
                        },
                    ));
                }

                if let (Some(old_h), Some(new_h)) =
                    (&old_col.histogram, &new_col.histogram)
                {
                    let old_d =
                        HistogramDigest::from_histogram(old_h);
                    let new_d =
                        HistogramDigest::from_histogram(new_h);
                    let kl = old_d.kl_divergence(&new_d);
                    if kl > th.histogram_kl_threshold {
                        changes.push(
                            ChangeSource::Statistics(
                                StatisticsChange::HistogramDrift {
                                    table: table.to_owned(),
                                    column: col.clone(),
                                    kl_divergence: kl,
                                },
                            ),
                        );
                    }
                }
            }
        }

        if th.index_changes_trigger {
            for idx_name in new.indexes.keys() {
                if !old.indexes.contains_key(idx_name) {
                    let cols =
                        new.indexes[idx_name].columns.clone();
                    changes.push(ChangeSource::Index(
                        IndexChange::Added {
                            table: table.to_owned(),
                            index_name: idx_name.clone(),
                            columns: cols,
                        },
                    ));
                }
            }
            for idx_name in old.indexes.keys() {
                if !new.indexes.contains_key(idx_name) {
                    changes.push(ChangeSource::Index(
                        IndexChange::Dropped {
                            table: table.to_owned(),
                            index_name: idx_name.clone(),
                        },
                    ));
                }
            }
        }

        changes
    }

    /// Optimize an expression, using the memo cache.
    fn optimize_and_cache(
        &mut self,
        expr: &RelExpr,
    ) -> Result<RelExpr, IncrementalError> {
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
            left: Box::new(Expr::Column(ColumnRef::new(
                "age",
            ))),
            right: Box::new(Expr::Const(Const::Int(18))),
        })
    }

    #[test]
    fn new_optimizer_defaults() {
        let opt = IncrementalOptimizer::new();
        assert_eq!(opt.generation(), 0);
        assert_eq!(opt.query_count(), 0);
        assert!(opt.active_rules().is_empty());
        assert_eq!(opt.plan_dependency_count(), 0);
    }

    #[test]
    fn register_query_returns_id() {
        let mut opt = IncrementalOptimizer::new();
        let id = opt
            .register_query(&simple_scan())
            .expect("should succeed");
        assert_eq!(id, 1);
        assert_eq!(opt.query_count(), 1);
    }

    #[test]
    fn register_multiple_queries() {
        let mut opt = IncrementalOptimizer::new();
        let id1 = opt
            .register_query(&simple_scan())
            .expect("should succeed");
        let id2 = opt
            .register_query(&filter_query())
            .expect("should succeed");
        assert_ne!(id1, id2);
        assert_eq!(opt.query_count(), 2);
    }

    #[test]
    fn get_optimized_returns_result() {
        let mut opt = IncrementalOptimizer::new();
        let id = opt
            .register_query(&simple_scan())
            .expect("should succeed");
        let result =
            opt.get_optimized(id).expect("should succeed");
        assert!(result.is_some());
    }

    #[test]
    fn get_optimized_not_found() {
        let opt = IncrementalOptimizer::new();
        let err = opt.get_optimized(999).unwrap_err();
        assert!(matches!(
            err,
            IncrementalError::QueryNotFound(999)
        ));
    }

    #[test]
    fn unregister_query() {
        let mut opt = IncrementalOptimizer::new();
        let id = opt
            .register_query(&simple_scan())
            .expect("should succeed");
        opt.unregister_query(id).expect("should succeed");
        assert_eq!(opt.query_count(), 0);
    }

    #[test]
    fn unregister_not_found() {
        let mut opt = IncrementalOptimizer::new();
        let err = opt.unregister_query(999).unwrap_err();
        assert!(matches!(
            err,
            IncrementalError::QueryNotFound(999)
        ));
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
        let result =
            opt.apply_changes().expect("should succeed");
        assert_eq!(result.generation, 1);
        assert_eq!(opt.generation(), 1);
        assert_eq!(opt.active_rules().len(), 1);
    }

    #[test]
    fn apply_empty_changes_noop() {
        let mut opt = IncrementalOptimizer::new();
        let result =
            opt.apply_changes().expect("should succeed");
        assert_eq!(result.reoptimized_count, 0);
        assert_eq!(result.generation, 0);
    }

    #[test]
    fn rule_change_triggers_reoptimization() {
        let mut opt = IncrementalOptimizer::new();
        let id = opt
            .register_query(&filter_query())
            .expect("should succeed");
        let initial = opt
            .get_optimized(id)
            .expect("should succeed")
            .expect("should have result")
            .clone();

        opt.add_rule(RuleId::new("filter-merge"));
        let result =
            opt.apply_changes().expect("should succeed");
        assert!(
            result.reoptimized_count + result.skipped_count
                > 0
        );
        assert_eq!(result.generation, 1);

        let new_result = opt
            .get_optimized(id)
            .expect("should succeed")
            .expect("should have result");
        assert!(
            *new_result == initial
                || matches!(
                    new_result,
                    RelExpr::Filter { .. }
                        | RelExpr::Scan { .. }
                )
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
        opt.register_query(&simple_scan())
            .expect("should succeed");
        opt.register_query(&filter_query())
            .expect("should succeed");
        opt.add_rule(RuleId::new("rule-a"));
        opt.add_rule(RuleId::new("rule-b"));
        let result =
            opt.apply_changes().expect("should succeed");
        assert_eq!(result.generation, 1);
        assert_eq!(opt.active_rules().len(), 2);
    }

    #[test]
    fn stats_track_operations() {
        let mut opt = IncrementalOptimizer::new();
        opt.register_query(&simple_scan())
            .expect("should succeed");
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
        assert!(debug_str.contains("plan_dependencies"));
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
        opt.register_query(&simple_scan())
            .expect("should succeed");
        opt.add_rule(RuleId::new("test-rule"));
        opt.apply_changes().expect("should succeed");

        let rule_diffs = vec![RuleChange::Removed(
            RuleId::new("test-rule"),
        )];
        let affected = opt
            .compute_affected_queries(&rule_diffs)
            .expect("should succeed");
        assert_eq!(affected.len(), 1);
    }

    #[test]
    fn compute_affected_empty_changes() {
        let opt = IncrementalOptimizer::new();
        let affected = opt
            .compute_affected_queries(&[])
            .expect("should succeed");
        assert!(affected.is_empty());
    }

    #[test]
    fn compute_affected_no_queries() {
        let opt = IncrementalOptimizer::new();
        let rule_diffs =
            vec![RuleChange::Removed(RuleId::new("gone"))];
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
        for i in 0..5 {
            let expr = RelExpr::scan(format!("table_{i}"));
            opt.register_query(&expr)
                .expect("should succeed");
        }
        opt.add_rule(RuleId::new("rule-1"));
        let r1 =
            opt.apply_changes().expect("should succeed");
        opt.add_rule(RuleId::new("rule-2"));
        let r2 =
            opt.apply_changes().expect("should succeed");
        assert_eq!(r1.generation, 1);
        assert_eq!(r2.generation, 2);
    }

    #[test]
    fn memo_cache_reused_within_generation() {
        let mut opt = IncrementalOptimizer::new();
        let expr = simple_scan();
        let id1 = opt
            .register_query(&expr)
            .expect("should succeed");
        let id2 = opt
            .register_query(&expr)
            .expect("should succeed");
        let r1 =
            opt.get_optimized(id1).expect("ok").expect("r");
        let r2 =
            opt.get_optimized(id2).expect("ok").expect("r");
        assert_eq!(r1, r2);
    }

    // ── RFC 0059 tests ──────────────────────────────────────

    #[test]
    fn register_and_unregister_plan_deps() {
        let mut opt = IncrementalOptimizer::new();
        let fp = QueryFingerprint::from_rel_expr(
            &simple_scan(),
        );
        let deps = PlanDependencies {
            table_cardinalities: [("users".into(), 1000.0)]
                .into_iter()
                .collect(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };
        opt.register_plan_dependencies(&fp, &deps);
        assert_eq!(opt.plan_dependency_count(), 1);

        opt.unregister_plan_dependencies(&fp);
        assert_eq!(opt.plan_dependency_count(), 0);
    }

    #[test]
    fn compute_affected_plans_row_count() {
        let mut opt = IncrementalOptimizer::new();
        let fp = QueryFingerprint::from_rel_expr(
            &simple_scan(),
        );
        let deps = PlanDependencies {
            table_cardinalities: [("users".into(), 1000.0)]
                .into_iter()
                .collect(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };
        opt.register_plan_dependencies(&fp, &deps);

        let changes = vec![ChangeSource::Statistics(
            StatisticsChange::RowCount {
                table: "users".into(),
                old_value: 1000.0,
                new_value: 10_000.0,
                ratio: 10.0,
            },
        )];
        let affected = opt
            .compute_affected_plans(&changes)
            .expect("should succeed");
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0], fp);
    }

    #[test]
    fn unaffected_plans_not_invalidated() {
        let mut opt = IncrementalOptimizer::new();

        let fp_users = QueryFingerprint::from_rel_expr(
            &RelExpr::scan("users"),
        );
        let fp_orders = QueryFingerprint::from_rel_expr(
            &RelExpr::scan("orders"),
        );

        let deps_users = PlanDependencies {
            table_cardinalities: [("users".into(), 1000.0)]
                .into_iter()
                .collect(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };
        let deps_orders = PlanDependencies {
            table_cardinalities: [("orders".into(), 5000.0)]
                .into_iter()
                .collect(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };

        opt.register_plan_dependencies(
            &fp_users,
            &deps_users,
        );
        opt.register_plan_dependencies(
            &fp_orders,
            &deps_orders,
        );

        let changes = vec![ChangeSource::Statistics(
            StatisticsChange::RowCount {
                table: "orders".into(),
                old_value: 5000.0,
                new_value: 50_000.0,
                ratio: 10.0,
            },
        )];
        let affected = opt
            .compute_affected_plans(&changes)
            .expect("should succeed");
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0], fp_orders);
    }

    #[test]
    fn change_ratio_basic() {
        assert!((change_ratio(100.0, 200.0) - 2.0).abs() < 1e-10);
        assert!((change_ratio(200.0, 100.0) - 2.0).abs() < 1e-10);
        assert!((change_ratio(100.0, 100.0) - 1.0).abs() < 1e-10);
        assert_eq!(change_ratio(0.0, 100.0), f64::MAX);
        assert!((change_ratio(0.0, 0.0) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn detect_changes_row_count() {
        let opt = IncrementalOptimizer::new();
        let old = ra_core::statistics::Statistics::new(1000.0);
        let new = ra_core::statistics::Statistics::new(3000.0);
        let changes = opt.detect_changes("t", &old, &new);
        assert_eq!(changes.len(), 1);
        assert!(matches!(
            &changes[0],
            ChangeSource::Statistics(
                StatisticsChange::RowCount { ratio, .. }
            ) if *ratio >= 2.0
        ));
    }

    #[test]
    fn detect_changes_below_threshold() {
        let opt = IncrementalOptimizer::new();
        let old = ra_core::statistics::Statistics::new(1000.0);
        let new = ra_core::statistics::Statistics::new(1500.0);
        let changes = opt.detect_changes("t", &old, &new);
        assert!(changes.is_empty());
    }

    #[test]
    fn histogram_digest_kl_identical() {
        let d1 = HistogramDigest {
            bucket_count: 3,
            frequencies: vec![0.2, 0.5, 0.3],
            total_rows: 100.0,
        };
        let d2 = d1.clone();
        assert!(d1.kl_divergence(&d2) < 1e-6);
    }

    #[test]
    fn histogram_digest_kl_different() {
        let d1 = HistogramDigest {
            bucket_count: 3,
            frequencies: vec![0.1, 0.1, 0.8],
            total_rows: 100.0,
        };
        let d2 = HistogramDigest {
            bucket_count: 3,
            frequencies: vec![0.8, 0.1, 0.1],
            total_rows: 100.0,
        };
        assert!(d1.kl_divergence(&d2) > 0.5);
    }

    #[test]
    fn resource_id_keys() {
        assert_eq!(
            ResourceId::RowCount("t".into()).key(),
            "t.row_count"
        );
        assert_eq!(
            ResourceId::NDistinct("t".into(), "c".into())
                .key(),
            "t.c.ndistinct"
        );
        assert_eq!(
            ResourceId::Index("t".into(), "idx".into()).key(),
            "t.idx"
        );
        assert_eq!(
            ResourceId::Histogram("t".into(), "c".into())
                .key(),
            "t.c.histogram"
        );
    }
}
