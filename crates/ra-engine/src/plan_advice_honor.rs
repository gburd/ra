//! Honor supplied plan advice during optimization.
//!
//! Given a parsed [`Advice`], compute which rule groups the
//! optimizer should *demote* (remove from the saturation rule
//! set) so the produced plan respects the user's request. This
//! is the first half of "honor": the rule advisor consults this
//! module to filter rules. Future work will add a cost-extraction
//! penalty layer that makes plans which still violate the advice
//! after rule filtering more expensive than compliant ones.
//!
//! The mapping from advice tags to rule-group labels is
//! deliberately conservative — when in doubt, allow the rule to
//! fire and let extraction-time checks (a future addition) decide.
//! Demoting too aggressively risks blocking transformations that
//! aren't related to the supplied advice.
//!
//! # Tag → rule-group demotions (current coverage)
//!
//! | Advice tag | Demoted rule groups |
//! |---|---|
//! | `JOIN_ORDER(...)` (any) | `join-reordering` |
//!
//! Other tags (scan-method, parallelism, partitionwise, etc.)
//! map to physical-plan choices that Ra's `RelExpr` doesn't
//! distinguish, so demoting rules wouldn't honor them. Those
//! tags are still parsed and stored in `OptimizationResult.
//! provenance` for diagnosis; the optimizer just doesn't gate
//! on them yet.

use std::collections::HashSet;

use ra_plan_advice::ast::{Advice, AdviceTag};

/// Compute the set of rule-group labels the rule advisor should
/// demote, given the supplied advice. Returns an empty set when
/// `advice` is empty or doesn't contain any tags Ra knows how to
/// honor.
#[must_use]
pub fn demoted_rule_groups(advice: &Advice) -> HashSet<&'static str> {
    let mut out = HashSet::new();
    for item in advice {
        // Any JOIN_ORDER advice forbids join reordering at the
        // rule level — the optimizer must produce the exact tree
        // shape supplied.
        if item.tag == AdviceTag::JoinOrder {
            out.insert("join-reordering");
        }
        // Scan-method, parallelism, partitionwise, semijoin
        // uniqueness, foreign-join — Ra's RelExpr has no notion
        // of these; demoting rules wouldn't help. They're parsed
        // and stored on PlanProvenance for diagnosis but the
        // optimizer doesn't gate on them.
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use ra_plan_advice::ast::{
        AdviceItem, AdviceTarget, RelationIdentifier,
    };

    fn item(tag: AdviceTag, names: &[&str]) -> AdviceItem {
        AdviceItem {
            tag,
            targets: names
                .iter()
                .map(|n| {
                    AdviceTarget::identifier(RelationIdentifier::simple(*n))
                })
                .collect(),
        }
    }

    #[test]
    fn empty_advice_demotes_nothing() {
        let empty: Advice = Vec::new();
        assert!(demoted_rule_groups(&empty).is_empty());
    }

    #[test]
    fn join_order_demotes_join_reordering() {
        let advice = vec![item(AdviceTag::JoinOrder, &["a", "b"])];
        let groups = demoted_rule_groups(&advice);
        assert!(groups.contains("join-reordering"));
    }

    #[test]
    fn scan_method_advice_does_not_demote_anything_yet() {
        // RelExpr has no physical scan choice, so demoting rules
        // can't honor SEQ_SCAN. Future work: extraction-time
        // penalty for plans that violate scan-method advice.
        let advice = vec![item(AdviceTag::SeqScan, &["t"])];
        assert!(demoted_rule_groups(&advice).is_empty());
    }

    #[test]
    fn join_order_combined_with_other_advice() {
        let advice = vec![
            item(AdviceTag::JoinOrder, &["a", "b"]),
            item(AdviceTag::SeqScan, &["a", "b"]),
        ];
        let groups = demoted_rule_groups(&advice);
        assert_eq!(groups.len(), 1);
        assert!(groups.contains("join-reordering"));
    }
}
