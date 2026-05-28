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
