//! Validate an extracted plan against supplied plan advice and
//! report per-item [`FeedbackFlags`].
//!
//! This is the post-extraction half of `pg_plan_advice`'s
//! "supplied advice" path. After the optimizer produces a
//! `RelExpr`, this module walks both the supplied advice and
//! the produced plan, classifies each advice item according to
//! what was matched, and returns a list of
//! [`AdviceItemFeedback`] entries that EXPLAIN-rendering code
//! can format with the same wording PG uses
//! (`matched`, `partially matched`, `not matched`, ...).
//!
//! # What we actually validate
//!
//! Ra's `RelExpr` is logical. The validations we can perform
//! today match the dimensions [`crate::plan_advice_emit`]
//! already covers:
//!
//! - **`JOIN_ORDER(...)`** — matches if the same set of base
//!   tables appears in the plan in the same outer-deep order;
//!   partially matches if some but not all targets resolve.
//! - **`SEQ_SCAN(...)`** — matches every target whose alias is
//!   present in the plan as a base scan.
//!
//! Other tags are flagged as `MATCH_PARTIAL` whenever any
//! target alias is present in the plan, with no further
//! validation. The reasoning matches PG's own self-documented
//! limitation: scan-method, parallelism, partitionwise, and
//! semijoin-uniqueness all describe physical-plan choices that
//! a pure-logical extractor doesn't make.

use std::collections::HashSet;

use ra_core::algebra::RelExpr;
use ra_plan_advice::ast::{Advice, AdviceItem, AdviceTarget, AdviceTargetKind};
use ra_plan_advice::feedback::FeedbackFlags;

/// Per-item validation result.
///
/// Mirrors PG's `pgpa_planner_append_feedback` output element:
/// the original advice item plus a [`FeedbackFlags`] bitmask.
#[derive(Debug, Clone)]
pub struct AdviceItemFeedback<'a> {
    pub item: &'a AdviceItem,
    pub flags: FeedbackFlags,
}

/// Validate every item in `advice` against the produced `plan`.
///
/// Returns one [`AdviceItemFeedback`] per item, in the same
/// order as `advice`. Empty `advice` returns an empty Vec.
#[must_use]
pub fn validate_advice<'a>(advice: &'a Advice, plan: &RelExpr) -> Vec<AdviceItemFeedback<'a>> {
    let aliases_in_plan = collect_aliases(plan);
    advice
        .iter()
        .map(|item| AdviceItemFeedback {
            item,
            flags: classify_item(item, plan, &aliases_in_plan),
        })
        .collect()
}

/// Walk an [`AdviceItem`] and produce its [`FeedbackFlags`].
///
/// The decision tree:
/// - count distinct identifiers in the item's target list (across
///   any sublist nesting).
/// - count how many of them appear as base-scan aliases in the
///   plan.
/// - none → `not_matched` (flags = 0).
/// - some, not all → `MATCH_PARTIAL`.
/// - all → `MATCH_PARTIAL | MATCH_FULL` (PG's invariant: full
///   implies partial).
/// - `JOIN_ORDER` additionally checks that the order matches the
///   plan's outer-deep traversal; mismatches set `FAILED`.
fn classify_item(
    item: &AdviceItem,
    plan: &RelExpr,
    aliases_in_plan: &HashSet<String>,
) -> FeedbackFlags {
    let identifiers = collect_target_identifiers(&item.targets);
    if identifiers.is_empty() {
        // Empty target list (legal for everything except
        // JOIN_ORDER) — neither matches nor not-matches.
        return FeedbackFlags::empty();
    }
    let total = identifiers.len();
    let matched = identifiers
        .iter()
        .filter(|id| aliases_in_plan.contains(id.as_str()))
        .count();

    let mut flags = FeedbackFlags::empty();
    if matched == 0 {
        return flags;
    }
    flags = flags.with(FeedbackFlags::MATCH_PARTIAL);
    if matched == total {
        flags = flags.with(FeedbackFlags::MATCH_FULL);
    }

    // JOIN_ORDER additionally checks that the plan's outer-deep
    // join traversal matches the supplied order.
    if item.tag == ra_plan_advice::AdviceTag::JoinOrder {
        let supplied_order: Vec<&str> = identifiers
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let plan_order = collect_outer_deep_order(plan);
        // Compare prefix: the supplied advice may be a prefix of
        // the plan's full order (in which case it matches), but
        // never the other way around.
        let n = supplied_order.len();
        let plan_prefix: Vec<&str> = plan_order.iter().take(n).map(std::string::String::as_str).collect();
        if plan_prefix != supplied_order {
            flags = flags.with(FeedbackFlags::FAILED);
        }
    }
    flags
}

/// Collect every base-scan alias reachable from `expr`.
fn collect_aliases(expr: &RelExpr) -> HashSet<String> {
    fn walk(e: &RelExpr, out: &mut HashSet<String>) {
        if let RelExpr::Scan { table, alias } = e {
            out.insert(alias.clone().unwrap_or_else(|| table.clone()));
        } else {
            for child in e.children() {
                walk(child, out);
            }
        }
    }
    let mut out = HashSet::new();
    walk(expr, &mut out);
    out
}

/// Collect every distinct alias name mentioned in a target list,
/// recursing into ordered/unordered sublists. Preserves left-to-right
/// order of first occurrence.
fn collect_target_identifiers(targets: &[AdviceTarget]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for t in targets {
        walk_target(t, &mut out, &mut seen);
    }
    out
}

fn walk_target(
    target: &AdviceTarget,
    out: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    match target.kind {
        AdviceTargetKind::Identifier => {
            if let Some(id) = &target.identifier {
                if seen.insert(id.alias_name.clone()) {
                    out.push(id.alias_name.clone());
                }
            }
        }
        AdviceTargetKind::OrderedList | AdviceTargetKind::UnorderedList => {
            for child in &target.children {
                walk_target(child, out, seen);
            }
        }
    }
}

/// Walk `expr` collecting base-table aliases in outer-deep order
/// (left-most leaf first). For a chain `((a JOIN b) JOIN c)`
/// returns `[a, b, c]`.
fn collect_outer_deep_order(expr: &RelExpr) -> Vec<String> {
    fn walk(e: &RelExpr, out: &mut Vec<String>) {
        match e {
            RelExpr::Scan { table, alias } => {
                out.push(alias.clone().unwrap_or_else(|| table.clone()));
            }
            RelExpr::Join { left, right, .. } => {
                walk(left, out);
                walk(right, out);
            }
            // Pass-through wrappers: descend.
            RelExpr::Filter { input, .. }
            | RelExpr::Project { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Aggregate { input, .. }
            | RelExpr::Window { input, .. }
            | RelExpr::Distinct { input } => walk(input, out),
            other => {
                for child in other.children() {
                    walk(child, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(expr, &mut out);
    out
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
mod tests {
    use super::*;
    use ra_core::algebra::JoinType;
    use ra_core::expr::{BinOp, ColumnRef, Expr};
    use ra_plan_advice::parse_advice;

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan { table: name.into(), alias: None }
    }

    fn eq_join(left: RelExpr, right: RelExpr, l: &str, r: &str) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified(l, "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified(r, "id"))),
            },
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn empty_advice_yields_empty_feedback() {
        let plan = scan("t");
        let advice: Advice = Vec::new();
        let fb = validate_advice(&advice, &plan);
        assert!(fb.is_empty());
    }

    #[test]
    fn full_match_seq_scan() {
        let plan = eq_join(scan("a"), scan("b"), "a", "b");
        let advice = parse_advice("SEQ_SCAN(a b)").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert_eq!(fb.len(), 1);
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_PARTIAL));
    }

    #[test]
    fn partial_match_seq_scan() {
        let plan = scan("a");
        let advice = parse_advice("SEQ_SCAN(a missing)").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_PARTIAL));
        assert!(!fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
    }

    #[test]
    fn not_matched_when_no_alias_present() {
        let plan = scan("a");
        let advice = parse_advice("SEQ_SCAN(missing)").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert_eq!(fb[0].flags.0, 0, "no flags should be set");
    }

    #[test]
    fn join_order_failed_when_plan_order_differs() {
        // Plan: ((a JOIN b) JOIN c) — outer-deep order [a, b, c]
        let plan = eq_join(
            eq_join(scan("a"), scan("b"), "a", "b"),
            scan("c"),
            "a",
            "c",
        );
        let advice = parse_advice("JOIN_ORDER(c b a)").unwrap();
        let fb = validate_advice(&advice, &plan);
        // All three identifiers exist in plan: full match.
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
        // But the order doesn't match: FAILED set.
        assert!(fb[0].flags.contains(FeedbackFlags::FAILED));
    }

    #[test]
    fn join_order_matched_when_plan_order_agrees() {
        let plan = eq_join(
            eq_join(scan("a"), scan("b"), "a", "b"),
            scan("c"),
            "a",
            "c",
        );
        let advice = parse_advice("JOIN_ORDER(a b c)").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
        assert!(!fb[0].flags.contains(FeedbackFlags::FAILED));
    }
}
