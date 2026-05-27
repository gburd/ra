#![expect(clippy::unwrap_used, reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests")]
//! Proptest-driven round-trip property: any AST that survives
//! `parse(render(a))` must round-trip to itself.

use proptest::prelude::*;

use ra_plan_advice::ast::{
    Advice, AdviceItem, AdviceTag, AdviceTarget,
    IndexTarget, RelationIdentifier,
};
use ra_plan_advice::{parse_advice, render_advice};

// ────────────────────────────────────────────────────────────────
// Strategies
// ────────────────────────────────────────────────────────────────

/// Identifiers safe to round-trip without quoting acrobatics.
/// Lowercase ASCII alphanumeric + underscore, leading non-digit.
/// We deliberately allow tag-keyword aliases here to exercise the
/// renderer's quoting logic, but disallow embedded `"` and other
/// awkward characters from the strategy because those expand the
/// search space without adding coverage we don't already have in
/// targeted unit tests.
fn arb_simple_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_filter(
        "skip empty",
        |s: &String| !s.is_empty(),
    )
}

fn arb_index_target() -> impl Strategy<Value = IndexTarget> {
    (
        prop::option::of(arb_simple_ident()),
        arb_simple_ident(),
    )
        .prop_map(|(schema, name)| IndexTarget { schema, name })
}

fn arb_rid() -> impl Strategy<Value = RelationIdentifier> {
    (
        arb_simple_ident(),
        1u32..5,
        prop::option::of(arb_simple_ident()),
        prop::option::of(arb_simple_ident()),
        prop::option::of(arb_simple_ident()),
    )
        .prop_map(
            |(alias_name, occurrence, partition_schema, partition_name, plan_name)| {
                RelationIdentifier {
                    alias_name,
                    occurrence,
                    // PG's grammar requires partition_schema to
                    // appear with partition_name (you can have
                    // partition_name alone, but not schema alone).
                    partition_schema: if partition_name.is_some() {
                        partition_schema
                    } else {
                        None
                    },
                    partition_name,
                    plan_name,
                }
            },
        )
}

/// A non-INDEX_SCAN target. INDEX targets are constructed by
/// `arb_index_scan_item` separately because they require a paired
/// `IndexTarget` value.
fn arb_simple_target() -> impl Strategy<Value = AdviceTarget> {
    arb_rid().prop_map(AdviceTarget::identifier)
}

/// A target that may optionally contain one level of sublist.
fn arb_generic_target() -> impl Strategy<Value = AdviceTarget> {
    prop_oneof![
        arb_simple_target(),
        prop::collection::vec(arb_simple_target(), 1..4)
            .prop_map(AdviceTarget::ordered),
    ]
}

/// `JOIN_ORDER` allows nested ordered sublists and one level of
/// curly-brace sublists. Use `prop_recursive` to bound depth.
fn arb_join_order_target() -> impl Strategy<Value = AdviceTarget> {
    arb_simple_target().prop_recursive(
        2,  // depth
        8,  // max nodes
        4,  // max children per node
        |inner| {
            prop_oneof![
                arb_simple_target(),
                prop::collection::vec(inner, 1..4)
                    .prop_map(AdviceTarget::ordered),
                // Curly-brace sublists themselves cannot contain
                // sublists per pgpa_parser.y::join_order_sublist;
                // populate with leaves only.
                prop::collection::vec(arb_simple_target(), 1..3)
                    .prop_map(AdviceTarget::unordered),
            ]
        },
    )
}

/// A complete `AdviceItem` with valid shape per PG's grammar.
fn arb_item() -> impl Strategy<Value = AdviceItem> {
    prop_oneof![
        // SEQ_SCAN, BITMAP_HEAP_SCAN, TID_SCAN, NO_GATHER:
        // simple target list (no sublists).
        (prop_oneof![
            Just(AdviceTag::SeqScan),
            Just(AdviceTag::BitmapHeapScan),
            Just(AdviceTag::TidScan),
            Just(AdviceTag::NoGather),
        ], prop::collection::vec(arb_simple_target(), 0..4))
            .prop_map(|(tag, targets)| AdviceItem { tag, targets }),
        // INDEX_SCAN / INDEX_ONLY_SCAN: each target carries an
        // IndexTarget.
        (prop_oneof![
            Just(AdviceTag::IndexScan),
            Just(AdviceTag::IndexOnlyScan),
        ], prop::collection::vec((arb_rid(), arb_index_target()), 0..3))
            .prop_map(|(tag, pairs)| {
                let targets = pairs
                    .into_iter()
                    .map(|(rid, idx)| {
                        let mut t = AdviceTarget::identifier(rid);
                        t.index = Some(idx);
                        t
                    })
                    .collect();
                AdviceItem { tag, targets }
            }),
        // JOIN_ORDER: at least one target, may contain sublists.
        prop::collection::vec(arb_join_order_target(), 1..4)
            .prop_map(|targets| AdviceItem {
                tag: AdviceTag::JoinOrder,
                targets,
            }),
        // Generic tags (HASH_JOIN, MERGE_JOIN_*, NESTED_LOOP_*,
        // GATHER, GATHER_MERGE, SEMIJOIN_*, PARTITIONWISE,
        // DO_NOT_SCAN): allow simple identifiers and one level of
        // ordered sublist.
        (prop_oneof![
            Just(AdviceTag::HashJoin),
            Just(AdviceTag::MergeJoinPlain),
            Just(AdviceTag::MergeJoinMaterialize),
            Just(AdviceTag::NestedLoopPlain),
            Just(AdviceTag::NestedLoopMaterialize),
            Just(AdviceTag::NestedLoopMemoize),
            Just(AdviceTag::Gather),
            Just(AdviceTag::GatherMerge),
            Just(AdviceTag::SemijoinUnique),
            Just(AdviceTag::SemijoinNonUnique),
            Just(AdviceTag::Partitionwise),
            Just(AdviceTag::DoNotScan),
        ], prop::collection::vec(arb_generic_target(), 0..3))
            .prop_map(|(tag, targets)| AdviceItem { tag, targets }),
        // FOREIGN_JOIN: each target must be a sublist of >= 2.
        prop::collection::vec(
            prop::collection::vec(arb_simple_target(), 2..4)
                .prop_map(AdviceTarget::ordered),
            1..3,
        )
        .prop_map(|targets| AdviceItem {
            tag: AdviceTag::ForeignJoin,
            targets,
        }),
    ]
}

fn arb_advice() -> impl Strategy<Value = Advice> {
    prop::collection::vec(arb_item(), 0..5)
}

// ────────────────────────────────────────────────────────────────
// Round-trip property
// ────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn render_then_parse_round_trips(advice in arb_advice()) {
        let rendered = render_advice(&advice);
        let reparsed = parse_advice(&rendered)
            .map_err(|e| TestCaseError::Fail(
                format!("parse({rendered:?}) failed: {e}").into(),
            ))?;
        prop_assert_eq!(advice, reparsed, "round-trip mismatch for {:?}", rendered);
    }
}

#[test]
fn empty_advice_round_trips() {
    let advice = Vec::new();
    let s = render_advice(&advice);
    assert_eq!(s, "");
    assert_eq!(parse_advice(&s).unwrap(), advice);
}
