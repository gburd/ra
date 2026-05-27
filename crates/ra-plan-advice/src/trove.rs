//! Lookup index over an [`Advice`] document.
//!
//! Mirrors `pgpa_trove.c`. Given a parsed [`Advice`] and a
//! [`RelationIdentifier`] (or a list of them), the trove returns
//! every advice item whose target list mentions that identifier.
//!
//! The Ra implementation is a `HashMap` keyed on `alias_name` with a
//! per-bucket linear scan to disambiguate by occurrence /
//! partition / plan. PG uses an open-addressing hash with the
//! same lookup contract; the choice of data structure does not
//! affect external behavior.

use std::collections::HashMap;

use crate::ast::{
    Advice, AdviceItem, AdviceTag, AdviceTarget, AdviceTargetKind,
    RelationIdentifier,
};

/// Owned lookup index built from a parsed [`Advice`].
///
/// A trove is cheap to query and intended to live for the
/// duration of one optimization. It owns nothing beyond the keys;
/// values are [`AdviceItem`] references back into the input.
#[derive(Debug, Clone)]
pub struct Trove<'a> {
    /// Alias-name → list of (item, target-position) pairs.
    /// Because the same alias may appear multiple times in the
    /// query (occurrence #2, #3, ...) and as a partition or in a
    /// different subplan, we store the full identifier on each
    /// entry and let lookup filter.
    by_alias: HashMap<String, Vec<TroveEntry<'a>>>,
}

/// A single entry in the trove.
#[derive(Debug, Clone, Copy)]
struct TroveEntry<'a> {
    /// The advice item this entry references.
    item: &'a AdviceItem,
    /// The target *within* `item` that holds the matched
    /// identifier. We index against the raw [`AdviceItem`] so
    /// callers can look at sibling targets when computing
    /// feedback (e.g. `JOIN_ORDER` requires every target to
    /// match in the same joinrel).
    target_index: usize,
    /// The matched identifier itself, copied as a reference into
    /// `item.targets`.
    identifier: &'a RelationIdentifier,
}

/// What a [`Trove::lookup`] consumer wants to know about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TroveLookup {
    /// Items whose tag is a scan-method advice
    /// (`SEQ_SCAN`, `INDEX_SCAN`, `INDEX_ONLY_SCAN`,
    /// `BITMAP_HEAP_SCAN`, `TID_SCAN`, `FOREIGN_JOIN`,
    /// `DO_NOT_SCAN`). These are the items the scan-side of the
    /// planner consults.
    Scan,
    /// Items whose tag governs general per-rel behavior
    /// (`NO_GATHER`, `GATHER`, `GATHER_MERGE`, `PARTITIONWISE`).
    Rel,
    /// Items whose tag governs join behavior (`JOIN_ORDER`, the
    /// six join-method tags, `SEMIJOIN_*`).
    Join,
}

/// One match returned by [`Trove::lookup`].
#[derive(Debug, Clone, Copy)]
pub struct TroveMatch<'a> {
    pub item: &'a AdviceItem,
    pub identifier: &'a RelationIdentifier,
    /// Position within `item.targets` of the target that contained
    /// the matched identifier. The same identifier can appear
    /// inside a sublist; in that case `target_index` points at
    /// the enclosing top-level target and the caller can walk
    /// `item.targets[target_index].children` if needed.
    pub target_index: usize,
}

impl<'a> Trove<'a> {
    /// Build a trove from parsed advice.
    #[must_use]
    pub fn build(advice: &'a Advice) -> Self {
        let mut by_alias: HashMap<String, Vec<TroveEntry<'a>>> = HashMap::new();
        for item in advice {
            for (idx, target) in item.targets.iter().enumerate() {
                Self::collect_identifiers(item, idx, target, &mut by_alias);
            }
        }
        Self { by_alias }
    }

    fn collect_identifiers(
        item: &'a AdviceItem,
        target_index: usize,
        target: &'a AdviceTarget,
        by_alias: &mut HashMap<String, Vec<TroveEntry<'a>>>,
    ) {
        match target.kind {
            AdviceTargetKind::Identifier => {
                if let Some(rid) = &target.identifier {
                    by_alias
                        .entry(rid.alias_name.clone())
                        .or_default()
                        .push(TroveEntry {
                            item,
                            target_index,
                            identifier: rid,
                        });
                }
            }
            AdviceTargetKind::OrderedList | AdviceTargetKind::UnorderedList => {
                for child in &target.children {
                    Self::collect_identifiers(item, target_index, child, by_alias);
                }
            }
        }
    }

    /// Look up advice items mentioning `id`, filtered by the
    /// given category.
    ///
    /// Multiple identifiers are matched conjunctively: an
    /// [`AdviceItem`] is returned only if every entry in `ids`
    /// resolves to a target in that item. This mirrors PG's
    /// `PGPA_TROVE_LOOKUP_JOIN` behavior.
    #[must_use]
    pub fn lookup(&self, ids: &[&RelationIdentifier], lookup: TroveLookup) -> Vec<TroveMatch<'a>> {
        if ids.is_empty() {
            return Vec::new();
        }
        // Find candidate items via the first identifier's bucket,
        // then verify the remaining identifiers match.
        let first = ids[0];
        let Some(bucket) = self.by_alias.get(&first.alias_name) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        'next_entry: for entry in bucket {
            if !category_matches(lookup, entry.item.tag) {
                continue;
            }
            if !identifier_matches(entry.identifier, first) {
                continue;
            }
            for &id in &ids[1..] {
                let mut found = false;
                for target in &entry.item.targets {
                    if target_contains_identifier(target, id) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    continue 'next_entry;
                }
            }
            out.push(TroveMatch {
                item: entry.item,
                identifier: entry.identifier,
                target_index: entry.target_index,
            });
        }
        out
    }

    /// Convenience for the common "scan-method advice for this
    /// table" query.
    #[must_use]
    pub fn lookup_scan(&self, id: &RelationIdentifier) -> Vec<TroveMatch<'a>> {
        self.lookup(&[id], TroveLookup::Scan)
    }

    /// Convenience for the common "per-rel advice for this table"
    /// query.
    #[must_use]
    pub fn lookup_rel(&self, id: &RelationIdentifier) -> Vec<TroveMatch<'a>> {
        self.lookup(&[id], TroveLookup::Rel)
    }

    /// Convenience for the join lookup, which expects multiple
    /// identifiers to resolve in the same item.
    #[must_use]
    pub fn lookup_join(&self, ids: &[&RelationIdentifier]) -> Vec<TroveMatch<'a>> {
        self.lookup(ids, TroveLookup::Join)
    }
}

/// True when `tag` belongs to the given lookup category.
fn category_matches(lookup: TroveLookup, tag: AdviceTag) -> bool {
    match lookup {
        TroveLookup::Scan => matches!(
            tag,
            AdviceTag::SeqScan
                | AdviceTag::IndexScan
                | AdviceTag::IndexOnlyScan
                | AdviceTag::BitmapHeapScan
                | AdviceTag::TidScan
                | AdviceTag::ForeignJoin
                | AdviceTag::DoNotScan,
        ),
        TroveLookup::Rel => matches!(
            tag,
            AdviceTag::NoGather
                | AdviceTag::Gather
                | AdviceTag::GatherMerge
                | AdviceTag::Partitionwise,
        ),
        TroveLookup::Join => matches!(
            tag,
            AdviceTag::JoinOrder
                | AdviceTag::HashJoin
                | AdviceTag::MergeJoinPlain
                | AdviceTag::MergeJoinMaterialize
                | AdviceTag::NestedLoopPlain
                | AdviceTag::NestedLoopMaterialize
                | AdviceTag::NestedLoopMemoize
                | AdviceTag::SemijoinUnique
                | AdviceTag::SemijoinNonUnique,
        ),
    }
}

/// Strict identifier equality: two identifiers match iff every
/// component is equal. `None` matches `None`, `Some(x)` matches
/// `Some(x)`. (PG's lookup is also strict; partition/plan
/// ambiguity is resolved at advice-generation time, not
/// lookup time.)
fn identifier_matches(stored: &RelationIdentifier, query: &RelationIdentifier) -> bool {
    stored.alias_name == query.alias_name
        && stored.occurrence == query.occurrence
        && stored.partition_schema == query.partition_schema
        && stored.partition_name == query.partition_name
        && stored.plan_name == query.plan_name
}

/// Recursive search for an identifier inside a target tree.
fn target_contains_identifier(target: &AdviceTarget, id: &RelationIdentifier) -> bool {
    match target.kind {
        AdviceTargetKind::Identifier => target
            .identifier
            .as_ref()
            .is_some_and(|stored| identifier_matches(stored, id)),
        AdviceTargetKind::OrderedList | AdviceTargetKind::UnorderedList => {
            target.children.iter().any(|c| target_contains_identifier(c, id))
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests")]
mod tests {
    use super::*;
    use crate::parse_advice;

    fn rid(s: &str) -> RelationIdentifier {
        RelationIdentifier::simple(s)
    }

    #[test]
    fn lookup_scan_finds_seq_scan() {
        let advice = parse_advice("SEQ_SCAN(a b) HASH_JOIN(b)").unwrap();
        let trove = Trove::build(&advice);

        let matches = trove.lookup_scan(&rid("a"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].item.tag, AdviceTag::SeqScan);

        // 'b' appears in both items, but lookup_scan filters out
        // the hash-join item.
        let matches = trove.lookup_scan(&rid("b"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].item.tag, AdviceTag::SeqScan);
    }

    #[test]
    fn lookup_join_requires_all_ids_to_match() {
        let advice = parse_advice("JOIN_ORDER(a b c) HASH_JOIN(c)").unwrap();
        let trove = Trove::build(&advice);

        // (a, b) appears together in JOIN_ORDER.
        let matches = trove.lookup_join(&[&rid("a"), &rid("b")]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].item.tag, AdviceTag::JoinOrder);

        // (a, x) — x is not in any join item.
        let matches = trove.lookup_join(&[&rid("a"), &rid("x")]);
        assert!(matches.is_empty());
    }

    #[test]
    fn unknown_alias_returns_empty() {
        let advice = parse_advice("SEQ_SCAN(a)").unwrap();
        let trove = Trove::build(&advice);
        assert!(trove.lookup_scan(&rid("z")).is_empty());
        assert!(trove.lookup_rel(&rid("z")).is_empty());
        assert!(trove.lookup_join(&[&rid("z")]).is_empty());
    }

    #[test]
    fn lookup_rel_finds_no_gather() {
        let advice = parse_advice("NO_GATHER(a) SEQ_SCAN(a)").unwrap();
        let trove = Trove::build(&advice);
        let matches = trove.lookup_rel(&rid("a"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].item.tag, AdviceTag::NoGather);
    }

    #[test]
    fn nested_sublist_identifiers_are_indexed() {
        let advice = parse_advice("JOIN_ORDER(a (b c) d)").unwrap();
        let trove = Trove::build(&advice);
        // All four identifiers resolve.
        for name in ["a", "b", "c", "d"] {
            assert!(
                !trove.lookup_join(&[&rid(name)]).is_empty(),
                "{name} should match",
            );
        }
    }

    #[test]
    fn identifier_with_occurrence_disambiguates() {
        let advice = parse_advice("SEQ_SCAN(t#1 t#2)").unwrap();
        let trove = Trove::build(&advice);
        let mut t1 = rid("t");
        t1.occurrence = 1;
        let mut t2 = rid("t");
        t2.occurrence = 2;
        assert_eq!(trove.lookup_scan(&t1).len(), 1);
        assert_eq!(trove.lookup_scan(&t2).len(), 1);
    }
}
