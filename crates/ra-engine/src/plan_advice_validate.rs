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

    // Two tags require special-case handling because their
    // semantics aren't "produce a plan that touches these
    // aliases":
    //
    // - DO_NOT_SCAN(t): a *negative* constraint. If the alias
    //   appears in the produced plan as a base scan, the
    //   advice has FAILED (the optimizer didn't manage to
    //   eliminate the scan). If the alias is absent the
    //   advice is fully matched.
    //
    // - FOREIGN_JOIN(left right): requires FDW pushdown,
    //   which Ra doesn't implement. The advice is always
    //   FAILED at this layer; the user gets honest feedback
    //   that the requested optimization isn't available.
    match item.tag {
        ra_plan_advice::AdviceTag::DoNotScan => {
            return classify_do_not_scan(&identifiers, aliases_in_plan);
        }
        ra_plan_advice::AdviceTag::ForeignJoin => {
            return classify_foreign_join(&identifiers, aliases_in_plan);
        }
        _ => {}
    }

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

/// Classify a `DO_NOT_SCAN(...)` item. Negative constraint:
/// success means none of the targets appear as base scans in
/// the plan; partial success means some appear and some don't;
/// any presence is FAILED.
fn classify_do_not_scan(
    identifiers: &[String],
    aliases_in_plan: &HashSet<String>,
) -> FeedbackFlags {
    if identifiers.is_empty() {
        return FeedbackFlags::empty();
    }
    let total = identifiers.len();
    let still_present = identifiers
        .iter()
        .filter(|id| aliases_in_plan.contains(id.as_str()))
        .count();

    if still_present == 0 {
        // Every targeted alias was eliminated from the plan —
        // the advice was honored.
        FeedbackFlags::empty()
            .with(FeedbackFlags::MATCH_PARTIAL)
            .with(FeedbackFlags::MATCH_FULL)
    } else if still_present == total {
        // All targeted aliases still in the plan. The advice
        // could not be honored at all. Mark FAILED so EXPLAIN
        // shows the failure clearly.
        FeedbackFlags::empty().with(FeedbackFlags::FAILED)
    } else {
        // Some eliminated, some remaining: partially honored.
        FeedbackFlags::empty()
            .with(FeedbackFlags::MATCH_PARTIAL)
            .with(FeedbackFlags::FAILED)
    }
}

/// Classify a `FOREIGN_JOIN(...)` item. Ra does not implement
/// FDW pushdown, so this advice is always FAILED at this
/// layer. We still set `MATCH_PARTIAL` when the alias is in the
/// plan so the user sees that we *recognize* the targets.
fn classify_foreign_join(
    identifiers: &[String],
    aliases_in_plan: &HashSet<String>,
) -> FeedbackFlags {
    if identifiers.is_empty() {
        return FeedbackFlags::empty().with(FeedbackFlags::FAILED);
    }
    let any_present = identifiers
        .iter()
        .any(|id| aliases_in_plan.contains(id.as_str()));
    if any_present {
        FeedbackFlags::empty()
            .with(FeedbackFlags::MATCH_PARTIAL)
            .with(FeedbackFlags::FAILED)
    } else {
        FeedbackFlags::empty().with(FeedbackFlags::FAILED)
    }
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

    #[test]
    fn do_not_scan_failed_when_alias_still_in_plan() {
        // DO_NOT_SCAN(t) with `t` reachable from the plan as
        // a base scan: the optimizer didn't manage to
        // eliminate it. FAILED should be set; MATCH_PARTIAL
        // and MATCH_FULL should NOT be set since the advice
        // wasn't honored.
        let plan = scan("t");
        let advice = parse_advice("DO_NOT_SCAN(t)").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert!(fb[0].flags.contains(FeedbackFlags::FAILED));
        assert!(!fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
        assert!(!fb[0].flags.contains(FeedbackFlags::MATCH_PARTIAL));
    }

    #[test]
    fn do_not_scan_matched_when_alias_eliminated() {
        // DO_NOT_SCAN(t) with `t` NOT in the plan: the
        // optimizer eliminated the scan (e.g. join elimination
        // or the user gave up the column). MATCH_FULL set,
        // FAILED clear.
        let plan = scan("u");
        let advice = parse_advice("DO_NOT_SCAN(t)").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
        assert!(!fb[0].flags.contains(FeedbackFlags::FAILED));
    }

    #[test]
    fn do_not_scan_partial_when_some_aliases_eliminated() {
        // DO_NOT_SCAN(s t) with only `s` eliminated. FAILED
        // and MATCH_PARTIAL set; MATCH_FULL clear.
        let plan = scan("t");
        let advice = parse_advice("DO_NOT_SCAN(s t)").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert!(fb[0].flags.contains(FeedbackFlags::FAILED));
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_PARTIAL));
        assert!(!fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
    }

    #[test]
    fn foreign_join_always_failed() {
        // FOREIGN_JOIN requires FDW pushdown which Ra doesn't
        // implement. The advice is always FAILED at this layer.
        // Syntax: FOREIGN_JOIN((a b)) — the sublist groups
        // the relations to be foreign-joined together.
        let plan = eq_join(scan("a"), scan("b"), "a", "b");
        let advice = parse_advice("FOREIGN_JOIN((a b))").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert!(fb[0].flags.contains(FeedbackFlags::FAILED));
        // Aliases are present so we mark MATCH_PARTIAL to
        // tell the user "we recognized the targets, just
        // can't honor the advice".
        assert!(fb[0].flags.contains(FeedbackFlags::MATCH_PARTIAL));
        assert!(!fb[0].flags.contains(FeedbackFlags::MATCH_FULL));
    }

    #[test]
    fn foreign_join_failed_with_unknown_aliases() {
        // FOREIGN_JOIN with aliases that don't exist in the
        // plan: still FAILED but no MATCH_PARTIAL.
        let plan = scan("c");
        let advice = parse_advice("FOREIGN_JOIN((a b))").unwrap();
        let fb = validate_advice(&advice, &plan);
        assert!(fb[0].flags.contains(FeedbackFlags::FAILED));
        assert!(!fb[0].flags.contains(FeedbackFlags::MATCH_PARTIAL));
    }
}
