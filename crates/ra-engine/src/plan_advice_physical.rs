//! Per-relation and per-joinrel physical-strategy preferences
//! derived from supplied plan advice.
//!
//! Ra's [`RelExpr`] is a logical algebra tree; it has no
//! `IndexScan` vs `SeqScan` distinction at the type level. So
//! when supplied advice says `INDEX_SCAN(t i)` or
//! `HASH_JOIN(b)`, the optimizer can't pick a different
//! `RelExpr` variant — that decision lives downstream in the
//! plan-builder layer (Ra-as-PG-extension translates `RelExpr`
//! to `pg_sys::Plan` nodes; that's where physical strategies
//! are chosen).
//!
//! This module provides a typed map that the honor pass
//! populates from supplied advice. Downstream consumers (the
//! PG plan builder, future Ra physical optimizers) read the
//! map at translation time and prefer the requested strategy
//! when feasible.
//!
//! The map is exposed on [`crate::OptimizationResult`] as
//! `physical_choices` so callers don't need to re-parse the
//! advice string themselves.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use ra_plan_advice::ast::{
    Advice, AdviceTag, AdviceTarget, AdviceTargetKind, IndexTarget,
};

/// Preferred scan strategy for a single base relation.
///
/// Mirrors PG's scan-method advice tags. `Auto` means no
/// preference (the consumer is free to pick).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScanStrategy {
    /// `SEQ_SCAN(t)`
    Seq,
    /// `INDEX_SCAN(t idx)` — carries the requested index name
    /// (and optional schema).
    Index {
        schema: Option<String>,
        name: String,
    },
    /// `INDEX_ONLY_SCAN(t idx)`
    IndexOnly {
        schema: Option<String>,
        name: String,
    },
    /// `BITMAP_HEAP_SCAN(t)`
    BitmapHeap,
    /// `TID_SCAN(t)`
    Tid,
    /// `DO_NOT_SCAN(t)` — caller should produce an empty-or-
    /// degenerate plan for this relation if possible. PG uses
    /// this for `AlternativeSubPlan` disambiguation.
    DoNotScan,
}

/// Preferred join strategy for a relation when it appears on
/// the inner side of a join.
///
/// PG's join-method advice tags target the inner side: e.g.
/// `HASH_JOIN(b)` means "in any join touching `b`, put `b` on
/// the inner side and use a hash join."
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JoinInnerStrategy {
    /// `HASH_JOIN(...)`
    Hash,
    /// `MERGE_JOIN_PLAIN(...)`
    MergeJoinPlain,
    /// `MERGE_JOIN_MATERIALIZE(...)`
    MergeJoinMaterialize,
    /// `NESTED_LOOP_PLAIN(...)`
    NestedLoopPlain,
    /// `NESTED_LOOP_MATERIALIZE(...)`
    NestedLoopMaterialize,
    /// `NESTED_LOOP_MEMOIZE(...)`
    NestedLoopMemoize,
    /// `FOREIGN_JOIN((...))` — push the named join down to a
    /// foreign-data-wrapper. The inner relation, by itself,
    /// can't carry `FOREIGN_JOIN` advice (it requires multiple
    /// targets); represented here so callers walking individual
    /// inner-side relations can see "push down if foreign."
    ForeignJoin,
}

/// Per-relation parallelism preference.
///
/// Mirrors PG's `GATHER` / `GATHER_MERGE` / `NO_GATHER` advice
/// tags. `NoGather` is the most informative for planners that
/// need to forbid parallelism for a particular subquery branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParallelStrategy {
    /// `GATHER(t)`
    Gather,
    /// `GATHER_MERGE(t)`
    GatherMerge,
    /// `NO_GATHER(t)`
    NoGather,
}

/// Map from supplied advice to per-relation physical-strategy
/// preferences. Each key is an alias name (matches
/// `RelationIdentifier::alias_name`); each value is the
/// requested strategy.
///
/// Construct with [`PhysicalChoices::from_advice`] and read with
/// the public accessors. The map is small (at most a handful of
/// entries per query) so a `HashMap` is overkill but matches
/// the expected access pattern.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalChoices {
    scans: HashMap<String, ScanStrategy>,
    joins: HashMap<String, JoinInnerStrategy>,
    parallel: HashMap<String, ParallelStrategy>,
}

impl PhysicalChoices {
    /// Empty choice map. Equivalent to "no preferences."
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Compile a parsed [`Advice`] document into per-alias
    /// preferences.
    ///
    /// Each scan-method tag like `SEQ_SCAN(a b c)` produces three
    /// entries (`a → Seq`, `b → Seq`, `c → Seq`). When the same
    /// alias appears in multiple conflicting tags (e.g. both
    /// `SEQ_SCAN(t)` and `INDEX_SCAN(t i)`), the *last* tag in
    /// document order wins, mirroring PG's "later advice
    /// overrides" semantics. The optimizer's honor pass also
    /// flags such conflicts as `FeedbackFlags::CONFLICTING` so
    /// users see the disagreement in EXPLAIN output.
    #[must_use]
    pub fn from_advice(advice: &Advice) -> Self {
        let mut out = Self::new();
        for item in advice {
            match item.tag {
                AdviceTag::SeqScan => apply_scan(&mut out, &item.targets, |_| ScanStrategy::Seq),
                AdviceTag::BitmapHeapScan => {
                    apply_scan(&mut out, &item.targets, |_| ScanStrategy::BitmapHeap);
                }
                AdviceTag::TidScan => apply_scan(&mut out, &item.targets, |_| ScanStrategy::Tid),
                AdviceTag::DoNotScan => {
                    apply_scan(&mut out, &item.targets, |_| ScanStrategy::DoNotScan);
                }
                AdviceTag::IndexScan => apply_scan(&mut out, &item.targets, |idx| {
                    let IndexTarget { schema, name } = idx
                        .cloned()
                        .unwrap_or_else(|| IndexTarget {
                            schema: None,
                            name: String::new(),
                        });
                    ScanStrategy::Index { schema, name }
                }),
                AdviceTag::IndexOnlyScan => apply_scan(&mut out, &item.targets, |idx| {
                    let IndexTarget { schema, name } = idx
                        .cloned()
                        .unwrap_or_else(|| IndexTarget {
                            schema: None,
                            name: String::new(),
                        });
                    ScanStrategy::IndexOnly { schema, name }
                }),
                AdviceTag::HashJoin => apply_join(&mut out, &item.targets, &JoinInnerStrategy::Hash),
                AdviceTag::MergeJoinPlain => {
                    apply_join(&mut out, &item.targets, &JoinInnerStrategy::MergeJoinPlain);
                }
                AdviceTag::MergeJoinMaterialize => {
                    apply_join(
                        &mut out,
                        &item.targets,
                        &JoinInnerStrategy::MergeJoinMaterialize,
                    );
                }
                AdviceTag::NestedLoopPlain => {
                    apply_join(&mut out, &item.targets, &JoinInnerStrategy::NestedLoopPlain);
                }
                AdviceTag::NestedLoopMaterialize => {
                    apply_join(
                        &mut out,
                        &item.targets,
                        &JoinInnerStrategy::NestedLoopMaterialize,
                    );
                }
                AdviceTag::NestedLoopMemoize => {
                    apply_join(
                        &mut out,
                        &item.targets,
                        &JoinInnerStrategy::NestedLoopMemoize,
                    );
                }
                AdviceTag::ForeignJoin => {
                    // FOREIGN_JOIN's targets are sublists; flatten
                    // and assign each member ForeignJoin so the
                    // builder can push the join down.
                    for t in &item.targets {
                        for child in flatten_aliases(t) {
                            out.joins.insert(child, JoinInnerStrategy::ForeignJoin);
                        }
                    }
                }
                AdviceTag::Gather => apply_parallel(&mut out, &item.targets, ParallelStrategy::Gather),
                AdviceTag::GatherMerge => {
                    apply_parallel(&mut out, &item.targets, ParallelStrategy::GatherMerge);
                }
                AdviceTag::NoGather => {
                    apply_parallel(&mut out, &item.targets, ParallelStrategy::NoGather);
                }
                // JOIN_ORDER, PARTITIONWISE, SEMIJOIN_*: not
                // per-relation physical strategies; honored
                // elsewhere (rule advisor + extraction).
                AdviceTag::JoinOrder
                | AdviceTag::Partitionwise
                | AdviceTag::SemijoinUnique
                | AdviceTag::SemijoinNonUnique => {}
            }
        }
        out
    }

    /// Look up the preferred scan strategy for `alias`.
    #[must_use]
    pub fn scan_for(&self, alias: &str) -> Option<&ScanStrategy> {
        self.scans.get(alias)
    }

    /// Look up the preferred inner-join strategy for `alias`.
    #[must_use]
    pub fn join_for(&self, alias: &str) -> Option<&JoinInnerStrategy> {
        self.joins.get(alias)
    }

    /// Look up the parallelism preference for `alias`.
    #[must_use]
    pub fn parallel_for(&self, alias: &str) -> Option<ParallelStrategy> {
        self.parallel.get(alias).copied()
    }

    /// Whether the map contains any preferences. Convenient for
    /// fast-path skipping in consumers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scans.is_empty() && self.joins.is_empty() && self.parallel.is_empty()
    }

    /// Number of recorded preferences across all categories.
    #[must_use]
    pub fn len(&self) -> usize {
        self.scans.len() + self.joins.len() + self.parallel.len()
    }

    /// Augment the choice map with cost-driven defaults for any
    /// alias the supplied advice didn't already cover.
    ///
    /// Walks the optimized [`RelExpr`] and, for every base scan
    /// without a recorded `ScanStrategy`, picks one based on the
    /// table's statistics. For every join without a recorded
    /// `JoinInnerStrategy`, picks `Hash` for equi-joins and
    /// `NestedLoopPlain` for non-equi-joins, mirroring PG's
    /// path-costing defaults.
    ///
    /// User-supplied advice always wins: this method only adds
    /// entries for aliases that are not already in the map. So
    /// `SET ra_planner.plan_advice = 'INDEX_SCAN(t i)'` followed
    /// by `augment_from_stats` produces a map where `t` keeps
    /// its `Index` strategy and any other tables get
    /// cost-driven choices.
    ///
    /// Conservative by design: when the cost story is ambiguous
    /// (e.g. table without statistics, predicate not covered by
    /// any index), defaults to `SeqScan` / `NestedLoopPlain` so
    /// behavior matches PG's defaults rather than introducing
    /// novel decisions.
    pub fn augment_from_stats(
        &mut self,
        expr: &ra_core::algebra::RelExpr,
        table_stats: &std::collections::HashMap<String, ra_core::statistics::Statistics>,
    ) {
        cost_driven::walk_for_scans(expr, table_stats, self);
        cost_driven::walk_for_joins(expr, table_stats, self);
    }

    /// Populate cost-based join strategies (hash vs nested-loop) from table
    /// statistics, leaving scan/parallel choices untouched. Used on the
    /// production planner path, where scan-strategy augmentation is
    /// intentionally withheld (a concrete PG `IndexScan` with a fixed
    /// selectivity guess bypasses PG's own path-costing). Existing entries
    /// (from supplied advice) are preserved.
    pub fn augment_join_strategies_from_stats(
        &mut self,
        expr: &ra_core::algebra::RelExpr,
        table_stats: &std::collections::HashMap<String, ra_core::statistics::Statistics>,
    ) {
        cost_driven::walk_for_joins(expr, table_stats, self);
    }
}

/// Build [`PhysicalChoices`] from a cost-extracted `RecExpr` that may contain
/// physical join variants (RFC 0090 Phase 3 chunk 4). For each physical join
/// node, the inner side's relation alias is mapped to the chosen strategy, so
/// the plan-builder can honor the e-graph's cost-driven physical choice instead
/// of the `augment_from_stats` heuristic. The physical method survives even
/// though `from_rec` collapses the variants back to a logical `RelExpr::Join`.
#[must_use]
pub fn physical_choices_from_recexpr(rec: &egg::RecExpr<crate::egraph::RelLang>) -> PhysicalChoices {
    use crate::egraph::RelLang;
    let nodes = rec.as_ref();
    let mut out = PhysicalChoices::new();
    for node in nodes {
        let (strategy, right) = match node {
            RelLang::HashJoinOp([_, _, _, r]) => (JoinInnerStrategy::Hash, *r),
            RelLang::MergeJoinOp([_, _, _, r]) => (JoinInnerStrategy::MergeJoinPlain, *r),
            RelLang::NestLoopOp([_, _, _, r]) | RelLang::IndexNestLoopOp([_, _, _, r]) => {
                (JoinInnerStrategy::NestedLoopPlain, *r)
            }
            _ => continue,
        };
        if let Some(alias) = first_relation_alias(nodes, right) {
            out.joins.insert(alias, strategy);
        }
    }
    out
}

/// First relation alias/name reachable from `id` in the extracted expr (the
/// inner relation of a join), descending through pass-through operators and,
/// for a nested inner join, its own inner side.
fn first_relation_alias(nodes: &[crate::egraph::RelLang], id: egg::Id) -> Option<String> {
    use crate::egraph::RelLang;
    let symbol_at = |sid: egg::Id| match nodes.get(usize::from(sid)) {
        Some(RelLang::Symbol(s)) => Some(s.to_string()),
        _ => None,
    };
    match nodes.get(usize::from(id))? {
        RelLang::Scan([t]) | RelLang::IndexScan([t, _]) => symbol_at(*t),
        RelLang::ScanAlias([_, a]) => symbol_at(*a),
        RelLang::Filter([_, c]) | RelLang::Project([_, c]) | RelLang::Sort([_, c]) => {
            first_relation_alias(nodes, *c)
        }
        RelLang::Limit([_, _, c]) | RelLang::Aggregate([_, _, c]) => first_relation_alias(nodes, *c),
        RelLang::Join([_, _, _, r])
        | RelLang::HashJoinOp([_, _, _, r])
        | RelLang::MergeJoinOp([_, _, _, r])
        | RelLang::NestLoopOp([_, _, _, r])
        | RelLang::IndexNestLoopOp([_, _, _, r]) => first_relation_alias(nodes, *r),
        _ => None,
    }
}

/// Cost-driven helpers used by [`PhysicalChoices::augment_from_stats`].
/// Kept in a private submodule so the heuristics can evolve
/// without touching the public API.
mod cost_driven {
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, Expr};
    use ra_core::statistics::Statistics;
    use std::collections::HashMap;

    use super::{JoinInnerStrategy, PhysicalChoices, ScanStrategy};

    /// Threshold below which we never pick an index scan even
    /// if one is available. PG's default `random_page_cost = 4.0`
    /// vs `seq_page_cost = 1.0` means the crossover for tiny
    /// tables is around hundreds of rows; we pick the lower end
    /// to be conservative (don't go behind PG's back to use an
    /// index when seq-scan is the cheaper choice).
    const SMALL_TABLE_ROW_THRESHOLD: f64 = 200.0;

    /// Walk `expr` and assign `ScanStrategy` to every base scan
    /// alias not already present in `choices.scans`.
    pub(super) fn walk_for_scans(
        expr: &RelExpr,
        table_stats: &HashMap<String, Statistics>,
        choices: &mut PhysicalChoices,
    ) {
        // Collect the list of (alias, table, predicate?) we'll
        // need to make scan decisions for.
        let mut scans: Vec<(String, String, Option<&Expr>)> = Vec::new();
        collect_scans(expr, None, &mut scans);

        for (alias, table, pred) in scans {
            if choices.scans.contains_key(&alias) {
                continue;
            }
            let strategy = pick_scan_strategy(&table, pred, table_stats);
            choices.scans.insert(alias, strategy);
        }
    }

    /// Walk `expr` and assign `JoinInnerStrategy` to every join's
    /// inner-side alias not already present in `choices.joins`.
    pub(super) fn walk_for_joins(
        expr: &RelExpr,
        table_stats: &HashMap<String, Statistics>,
        choices: &mut PhysicalChoices,
    ) {
        let mut joins: Vec<(String, JoinInnerStrategy)> = Vec::new();
        collect_joins(expr, table_stats, &mut joins);
        for (inner_alias, strategy) in joins {
            // Advice (already-present keys) wins over cost-based defaults.
            choices.joins.entry(inner_alias).or_insert(strategy);
        }
    }

    /// Pick the cost-driven scan strategy for a base relation.
    ///
    /// Heuristic:
    /// - If the table has fewer than `SMALL_TABLE_ROW_THRESHOLD`
    ///   rows, sequential scan wins (no index can beat a small
    ///   sequential read).
    /// - If a predicate has equality conditions on a prefix of
    ///   an index's columns, that index is a candidate. The
    ///   index with the longest matching prefix wins (more
    ///   columns matched → better selectivity). Ties are broken
    ///   by primary-key > unique > regular, matching PG's
    ///   path-cost ordering for index selectivity heuristics.
    /// - Otherwise sequential scan.
    fn pick_scan_strategy(
        table: &str,
        pred: Option<&Expr>,
        table_stats: &HashMap<String, Statistics>,
    ) -> ScanStrategy {
        let stats_key = table.to_lowercase();
        let Some(stats) = table_stats.get(&stats_key) else {
            // No stats → can't reason about index utility.
            // Default to seq-scan to match PG when stats are
            // missing.
            return ScanStrategy::Seq;
        };
        if stats.row_count < SMALL_TABLE_ROW_THRESHOLD {
            return ScanStrategy::Seq;
        }
        let Some(pred) = pred else {
            // No filter → seq-scan all rows is the only sensible
            // choice for now. Future work: covered queries
            // with index-only scans.
            return ScanStrategy::Seq;
        };

        // Collect all columns the predicate has an equality
        // test on; an index column-prefix is "covered" if every
        // column in the prefix appears here.
        let mut eq_columns: Vec<String> = Vec::new();
        collect_eq_columns(pred, &mut eq_columns);
        if eq_columns.is_empty() {
            return ScanStrategy::Seq;
        }

        // Find the index with the longest covered prefix.
        // Each candidate is (index_name, prefix_len, tie_break,
        // selectivity). Ranking:
        //   1. larger prefix_len wins
        //   2. higher tie_break wins (primary > unique > regular)
        //   3. smaller selectivity wins (more selective →
        //      fewer rows per probe)
        // Selectivity is computed from column NDV when stats
        // are available; absent stats default to a neutral
        // selectivity of 1.0 so we don't bias against
        // un-analyzed indexes.
        let mut best: Option<(&String, usize, u8, f64)> = None;
        for (idx_name, idx_stats) in &stats.indexes {
            // Only B-tree and Hash indexes match equality
            // predicates correctly. GIN/GiST/BRIN/SP-GiST/RUM
            // are operator-class-specific (text search,
            // spatial overlap, range queries) and must not
            // be selected for plain `=` predicates — choosing
            // them would produce a plan that PG can't execute
            // or that's executor-correct but pessimal.
            // RFC 0039 documents the full operator-class-aware
            // path; this MVP gates on index_type so the
            // wrong index is never picked, then defers
            // GIN/GiST etc. to explicit `INDEX_SCAN` advice
            // until predicate-matching is wired through.
            if !is_btree_or_hash(idx_stats.index_type) {
                continue;
            }
            let prefix_len = covered_prefix_len(&idx_stats.columns, &eq_columns);
            if prefix_len == 0 {
                continue;
            }
            let tie_break: u8 = if idx_stats.is_primary {
                2
            } else {
                u8::from(idx_stats.is_unique)
            };
            let selectivity = covered_prefix_selectivity(
                &idx_stats.columns[..prefix_len],
                stats,
            );
            let candidate = (idx_name, prefix_len, tie_break, selectivity);
            best = match best {
                None => Some(candidate),
                Some((_, cur_len, cur_tie, cur_sel)) => {
                    let strictly_better = prefix_len > cur_len
                        || (prefix_len == cur_len && tie_break > cur_tie)
                        || (prefix_len == cur_len
                            && tie_break == cur_tie
                            && selectivity < cur_sel);
                    if strictly_better {
                        Some(candidate)
                    } else {
                        best
                    }
                }
            };
        }

        match best {
            Some((name, _, _, _)) => ScanStrategy::Index {
                schema: None,
                name: name.clone(),
            },
            None => ScanStrategy::Seq,
        }
    }

    /// True if `kind` indexes equality on B-tree-style ordering
    /// or hash-style equality lookups. False for specialized
    /// operator-class indexes (GIN/GiST/SP-GiST/BRIN/RUM/IVFFlat
    /// /HNSW) which need predicate types this layer doesn't yet
    /// recognize.
    fn is_btree_or_hash(kind: ra_core::facts::IndexType) -> bool {
        use ra_core::facts::IndexType;
        matches!(kind, IndexType::BTree | IndexType::Hash)
    }

    /// Estimate the combined equality selectivity of every
    /// column in `prefix`, using NDV from [`Statistics::columns`]
    /// when present and assuming column independence (`AND` of
    /// per-column selectivities is the product of selectivities).
    /// Returns `1.0` for any column with missing or zero NDV
    /// — neutral, so absent statistics never bias the
    /// comparison incorrectly.
    fn covered_prefix_selectivity(
        prefix: &[String],
        stats: &Statistics,
    ) -> f64 {
        let mut acc = 1.0_f64;
        for col in prefix {
            let col_sel = stats
                .columns
                .iter()
                .find_map(|(name, cs)| {
                    if name.eq_ignore_ascii_case(col)
                        && cs.distinct_count > 0.0
                    {
                        // Equality selectivity ~ 1 / NDV
                        // (adjusted for NULL fraction).
                        let live = (1.0 - cs.null_fraction).max(0.0);
                        Some(live / cs.distinct_count.max(1.0))
                    } else {
                        None
                    }
                })
                .unwrap_or(1.0);
            acc *= col_sel;
        }
        acc
    }

    /// Number of columns from the start of `index_columns` that
    /// every appear in `eq_columns` (case-insensitive). Returns 0
    /// when the leading column isn't covered.
    fn covered_prefix_len(index_columns: &[String], eq_columns: &[String]) -> usize {
        let mut covered = 0usize;
        for idx_col in index_columns {
            let hit = eq_columns
                .iter()
                .any(|c| c.eq_ignore_ascii_case(idx_col));
            if hit {
                covered += 1;
            } else {
                break;
            }
        }
        covered
    }

    /// Walk `pred` collecting column names that appear in
    /// equality tests against a non-column operand. Only
    /// AND-conjunctions are descended — OR cannot guarantee
    /// every disjunct constrains the same column. Equality of
    /// two columns (`a.x = b.x`) is excluded since join
    /// conditions don't constrain index access.
    fn collect_eq_columns(pred: &Expr, out: &mut Vec<String>) {
        match pred {
            Expr::BinOp { op: BinOp::Eq, left, right } => {
                let l_col = as_column_name(left);
                let r_col = as_column_name(right);
                match (l_col, r_col) {
                    // Column = const: index-restricting. Either
                    // orientation produces the same constraint.
                    (Some(c), None) | (None, Some(c)) => out.push(c.to_string()),
                    // Two columns or two non-columns: not an
                    // index-restricting equality.
                    _ => {}
                }
            }
            Expr::BinOp { op: BinOp::And, left, right } => {
                collect_eq_columns(left, out);
                collect_eq_columns(right, out);
            }
            _ => {}
        }
    }

    fn as_column_name(e: &Expr) -> Option<&str> {
        if let Expr::Column(c) = e {
            Some(&c.column)
        } else {
            None
        }
    }

    /// Walk `expr` collecting `(alias, table, applicable_predicate)`
    /// per base scan. The predicate is the parent Filter's
    /// predicate when it's directly above the scan; otherwise
    /// `None`.
    fn collect_scans<'a>(
        expr: &'a RelExpr,
        parent_pred: Option<&'a Expr>,
        out: &mut Vec<(String, String, Option<&'a Expr>)>,
    ) {
        match expr {
            RelExpr::Scan { table, alias } => {
                let alias_name = alias.clone().unwrap_or_else(|| table.clone());
                out.push((alias_name, table.clone(), parent_pred));
            }
            RelExpr::Filter { predicate, input } => {
                collect_scans(input, Some(predicate), out);
            }
            RelExpr::Project { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Aggregate { input, .. }
            | RelExpr::Window { input, .. }
            | RelExpr::Distinct { input } => collect_scans(input, parent_pred, out),
            RelExpr::Join { left, right, .. } => {
                // Filter doesn't pass through joins for column-
                // scoping purposes; reset.
                collect_scans(left, None, out);
                collect_scans(right, None, out);
            }
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                collect_scans(left, None, out);
                collect_scans(right, None, out);
            }
            RelExpr::CTE { definition, body, .. } => {
                collect_scans(definition, None, out);
                collect_scans(body, None, out);
            }
            other => {
                for child in other.children() {
                    collect_scans(child, None, out);
                }
            }
        }
    }

    /// Walk `expr` collecting `(inner_alias, is_equi_join)` per
    /// join.
    fn collect_joins(
        expr: &RelExpr,
        table_stats: &HashMap<String, Statistics>,
        out: &mut Vec<(String, JoinInnerStrategy)>,
    ) {
        if let RelExpr::Join { condition, left, right, .. } = expr {
            if let Some(inner_alias) = super::inner_join_alias(right) {
                let outer_rows = leaf_table(left).and_then(|t| row_count(table_stats, &t));
                let inner_rows = leaf_table(right).and_then(|t| row_count(table_stats, &t));
                let strategy =
                    decide_join_strategy(outer_rows, inner_rows, is_equi_join(condition));
                out.push((inner_alias, strategy));
            }
        }
        for child in expr.children() {
            collect_joins(child, table_stats, out);
        }
    }

    /// Cost-based hash-vs-nested-loop choice for a join's inner side.
    ///
    /// A non-equi join can only nested-loop. For an equi-join, a plain
    /// nested loop rescans the inner relation once per outer row
    /// (cost ~ `outer*inner`) while a hash join builds the inner once and
    /// probes (cost ~ `outer+inner`); hash wins for all but degenerate
    /// (<=1 row) inputs. Cardinality-driven, no magic constants. When row
    /// counts are unknown, defaults to `Hash` for equi-joins — the safe
    /// choice that avoids an O(n*m) nested loop on large inputs.
    fn decide_join_strategy(
        outer_rows: Option<f64>,
        inner_rows: Option<f64>,
        is_equi: bool,
    ) -> JoinInnerStrategy {
        if !is_equi {
            return JoinInnerStrategy::NestedLoopPlain;
        }
        match (outer_rows, inner_rows) {
            (Some(o), Some(i)) if o * i < o + i => JoinInnerStrategy::NestedLoopPlain,
            _ => JoinInnerStrategy::Hash,
        }
    }

    /// True if `cond` is or contains an equality of two column
    /// references — the standard equi-join shape that hash
    /// joins handle directly.
    fn is_equi_join(cond: &Expr) -> bool {
        match cond {
            Expr::BinOp { op: BinOp::Eq, left, right } => {
                matches!(left.as_ref(), Expr::Column(_))
                    && matches!(right.as_ref(), Expr::Column(_))
            }
            Expr::BinOp { op: BinOp::And, left, right } => {
                is_equi_join(left) || is_equi_join(right)
            }
            _ => false,
        }
    }

    /// Base-table name of a relation, descending through pass-through
    /// nodes. Used to look up the relation's row count for cost decisions.
    fn leaf_table(expr: &RelExpr) -> Option<String> {
        match expr {
            RelExpr::Scan { table, .. } => Some(table.clone()),
            RelExpr::Filter { input, .. }
            | RelExpr::Project { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Distinct { input } => leaf_table(input),
            _ => None,
        }
    }

    fn row_count(table_stats: &HashMap<String, Statistics>, table: &str) -> Option<f64> {
        table_stats.get(&table.to_lowercase()).map(|s| s.row_count)
    }
}

/// The alias used to key a join's inner (right) relation in
/// [`PhysicalChoices`], descending through pass-through nodes to the base
/// scan. Shared by the cost-driven join walk and the plan builder so both
/// agree on the lookup key.
#[must_use]
pub fn inner_join_alias(expr: &ra_core::algebra::RelExpr) -> Option<String> {
    use ra_core::algebra::RelExpr;
    match expr {
        RelExpr::Scan { table, alias } => Some(alias.clone().unwrap_or_else(|| table.clone())),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => inner_join_alias(input),
        _ => None,
    }
}

/// Walk a list of advice targets and apply a scan-strategy
/// builder per identifier. `INDEX_SCAN`'s index target is
/// extracted from the per-target `index` field; non-index tags
/// pass `None`.
fn apply_scan(
    out: &mut PhysicalChoices,
    targets: &[AdviceTarget],
    make: impl Fn(Option<&IndexTarget>) -> ScanStrategy,
) {
    for t in targets {
        for alias in flatten_aliases(t) {
            out.scans.insert(alias, make(t.index.as_ref()));
        }
    }
}

/// Walk a list of advice targets and apply a join-inner
/// strategy per identifier.
fn apply_join(
    out: &mut PhysicalChoices,
    targets: &[AdviceTarget],
    strategy: &JoinInnerStrategy,
) {
    for t in targets {
        for alias in flatten_aliases(t) {
            out.joins.insert(alias, strategy.clone());
        }
    }
}

/// Walk a list of advice targets and apply a parallel-strategy
/// per identifier.
fn apply_parallel(
    out: &mut PhysicalChoices,
    targets: &[AdviceTarget],
    strategy: ParallelStrategy,
) {
    for t in targets {
        for alias in flatten_aliases(t) {
            out.parallel.insert(alias, strategy);
        }
    }
}

/// Recursively pull out the alias names from an [`AdviceTarget`],
/// flattening sublists. Used because most physical-strategy tags
/// allow either a flat target list or one-level sublists.
fn flatten_aliases(target: &AdviceTarget) -> Vec<String> {
    let mut out = Vec::new();
    walk(target, &mut out);
    out
}

fn walk(target: &AdviceTarget, out: &mut Vec<String>) {
    match target.kind {
        AdviceTargetKind::Identifier => {
            if let Some(id) = &target.identifier {
                out.push(id.alias_name.clone());
            }
        }
        AdviceTargetKind::OrderedList | AdviceTargetKind::UnorderedList => {
            for c in &target.children {
                walk(c, out);
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
#[expect(
    clippy::panic,
    reason = "test code; panic is how we report a failed expectation"
)]
mod tests {
    use super::*;

    #[test]
    fn physical_choices_derived_from_extracted_recexpr() {
        use crate::egraph::RelLang;
        // A cost-extracted expr where the optimizer chose hash-join for the
        // inner relation `c`. The bridge maps inner alias -> strategy.
        let rec: egg::RecExpr<RelLang> =
            "(hash-join inner (eq (qcol o a) (qcol c b)) (scan o) (scan c))"
                .parse()
                .expect("valid expr");
        let choices = physical_choices_from_recexpr(&rec);
        assert_eq!(choices.join_for("c"), Some(&JoinInnerStrategy::Hash));

        let rec_nl: egg::RecExpr<RelLang> =
            "(nest-loop inner (eq (qcol o a) (qcol c b)) (scan o) (scan c))"
                .parse()
                .expect("valid expr");
        let nl = physical_choices_from_recexpr(&rec_nl);
        assert_eq!(nl.join_for("c"), Some(&JoinInnerStrategy::NestedLoopPlain));

        // A purely logical join produces no physical choice.
        let rec_logical: egg::RecExpr<RelLang> =
            "(join inner (eq (qcol o a) (qcol c b)) (scan o) (scan c))"
                .parse()
                .expect("valid expr");
        assert!(physical_choices_from_recexpr(&rec_logical).is_empty());
    }
    use ra_plan_advice::parse_advice;

    #[test]
    fn empty_advice_yields_empty_choices() {
        let advice = parse_advice("").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert!(pc.is_empty());
    }

    #[test]
    fn seq_scan_records_seq_strategy() {
        let advice = parse_advice("SEQ_SCAN(a b)").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert_eq!(pc.scan_for("a"), Some(&ScanStrategy::Seq));
        assert_eq!(pc.scan_for("b"), Some(&ScanStrategy::Seq));
        assert_eq!(pc.scan_for("c"), None);
    }

    #[test]
    fn index_scan_records_index_name() {
        let advice = parse_advice("INDEX_SCAN(orders orders_pkey)").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        match pc.scan_for("orders") {
            Some(ScanStrategy::Index { schema, name }) => {
                assert_eq!(schema, &None);
                assert_eq!(name, "orders_pkey");
            }
            other => panic!("expected Index strategy, got {other:?}"),
        }
    }

    #[test]
    fn index_scan_with_schema_qualified_index() {
        let advice = parse_advice("INDEX_ONLY_SCAN(t public.t_pkey)").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        match pc.scan_for("t") {
            Some(ScanStrategy::IndexOnly { schema, name }) => {
                assert_eq!(schema, &Some("public".into()));
                assert_eq!(name, "t_pkey");
            }
            other => panic!("expected IndexOnly strategy, got {other:?}"),
        }
    }

    #[test]
    fn hash_join_records_inner_strategy() {
        let advice = parse_advice("HASH_JOIN(b c)").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert_eq!(pc.join_for("b"), Some(&JoinInnerStrategy::Hash));
        assert_eq!(pc.join_for("c"), Some(&JoinInnerStrategy::Hash));
    }

    #[test]
    fn no_gather_records_parallel_preference() {
        let advice = parse_advice("NO_GATHER(t)").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert_eq!(pc.parallel_for("t"), Some(ParallelStrategy::NoGather));
    }

    #[test]
    fn later_advice_overrides_earlier() {
        // SEQ_SCAN(t) followed by INDEX_SCAN(t idx) — the
        // INDEX wins.
        let advice = parse_advice("SEQ_SCAN(t) INDEX_SCAN(t idx)").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert!(matches!(
            pc.scan_for("t"),
            Some(ScanStrategy::Index { .. })
        ));
    }

    #[test]
    fn scan_advice_inside_sublist_is_flattened() {
        // GATHER((a b)) — the sublist contains two identifiers.
        let advice = parse_advice("GATHER((a b))").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert_eq!(pc.parallel_for("a"), Some(ParallelStrategy::Gather));
        assert_eq!(pc.parallel_for("b"), Some(ParallelStrategy::Gather));
    }

    #[test]
    fn join_order_advice_does_not_populate_physical_choices() {
        // JOIN_ORDER is honored by rule demotion, not by the
        // physical-choice map.
        let advice = parse_advice("JOIN_ORDER(a b c)").unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert!(pc.is_empty());
    }

    #[test]
    fn mixed_advice_populates_each_category() {
        let advice = parse_advice(
            "SEQ_SCAN(a) INDEX_SCAN(b b_idx) HASH_JOIN(c) NO_GATHER(d)"
        ).unwrap();
        let pc = PhysicalChoices::from_advice(&advice);
        assert_eq!(pc.len(), 4);
        assert!(matches!(pc.scan_for("a"), Some(ScanStrategy::Seq)));
        assert!(matches!(pc.scan_for("b"), Some(ScanStrategy::Index { .. })));
        assert!(matches!(pc.join_for("c"), Some(JoinInnerStrategy::Hash)));
        assert_eq!(pc.parallel_for("d"), Some(ParallelStrategy::NoGather));
    }
}
