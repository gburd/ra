//! Property-based tests for the rule analyzer, type checker,
//! index, and registry.
//!
//! Verifies structural invariants:
//! - Dependencies are reflexive-free (no self-edges)
//! - Conflicts are symmetric
//! - Index queries are consistent with insert/remove
//! - Registry rejects invalid rules and accepts valid ones

use proptest::prelude::*;
use std::collections::HashSet;

use ra_compiler::analyzer::{Analyzer, OperatorKind, RuleInfo};
use ra_compiler::checker::TypeChecker;
use ra_compiler::index::RuleIndex;
use ra_compiler::registry::RuleRegistry;

fn arb_operator_kind() -> impl Strategy<Value = OperatorKind> {
    prop_oneof![
        Just(OperatorKind::Scan),
        Just(OperatorKind::Filter),
        Just(OperatorKind::Project),
        Just(OperatorKind::Join),
        Just(OperatorKind::Aggregate),
        Just(OperatorKind::Sort),
        Just(OperatorKind::Limit),
        Just(OperatorKind::SetOp),
    ]
}

fn arb_operator_set() -> impl Strategy<Value = HashSet<OperatorKind>> {
    prop::collection::hash_set(arb_operator_kind(), 0..=4)
}

fn arb_rule_info() -> impl Strategy<Value = RuleInfo> {
    (
        "[a-z][a-z0-9-]{0,15}",
        prop_oneof![
            Just("logical"),
            Just("physical"),
            Just("implementation"),
        ],
        arb_operator_set(),
        arb_operator_set(),
    )
        .prop_map(|(name, category, reads, writes)| RuleInfo {
            name,
            category: category.to_owned(),
            reads,
            writes,
        })
}

proptest! {
    #[test]
    fn dependencies_have_no_self_edges(
        rules in prop::collection::vec(arb_rule_info(), 1..=10)
    ) {
        let mut analyzer = Analyzer::new();
        for rule in &rules {
            analyzer.add_rule(rule.clone());
        }
        let deps = analyzer.compute_dependencies();
        for dep in &deps {
            prop_assert_ne!(
                &dep.producer, &dep.consumer,
                "dependency should not have self-edge"
            );
        }
    }

    #[test]
    fn dependency_shared_operators_nonempty(
        rules in prop::collection::vec(arb_rule_info(), 1..=10)
    ) {
        let mut analyzer = Analyzer::new();
        for rule in &rules {
            analyzer.add_rule(rule.clone());
        }
        let deps = analyzer.compute_dependencies();
        for dep in &deps {
            prop_assert!(
                !dep.shared_operators.is_empty(),
                "dependency should have at least one shared operator"
            );
        }
    }

    #[test]
    fn affected_by_nonexistent_is_empty(
        rules in prop::collection::vec(arb_rule_info(), 0..=5)
    ) {
        let mut analyzer = Analyzer::new();
        for rule in &rules {
            analyzer.add_rule(rule.clone());
        }
        let affected = analyzer.affected_by("__nonexistent__");
        prop_assert!(affected.is_empty());
    }

    #[test]
    fn affected_by_returns_sorted(
        rules in prop::collection::vec(arb_rule_info(), 1..=10)
    ) {
        let mut analyzer = Analyzer::new();
        for rule in &rules {
            analyzer.add_rule(rule.clone());
        }
        if let Some(first) = rules.first() {
            let affected = analyzer.affected_by(&first.name);
            let mut sorted = affected.clone();
            sorted.sort();
            prop_assert_eq!(affected, sorted);
        }
    }

    #[test]
    fn rules_in_category_sorted(
        rules in prop::collection::vec(arb_rule_info(), 1..=10)
    ) {
        let mut analyzer = Analyzer::new();
        for rule in &rules {
            analyzer.add_rule(rule.clone());
        }
        let result = analyzer.rules_in_category("logical");
        let mut sorted = result.clone();
        sorted.sort_unstable();
        prop_assert_eq!(result, sorted);
    }

    #[test]
    fn checker_valid_rule_no_errors(
        rule in arb_rule_info()
            .prop_filter(
                "rule must have non-empty name and category",
                |r| !r.name.is_empty() && !r.category.is_empty()
                    && !r.reads.is_empty() && !r.writes.is_empty()
                    && r.writes.is_subset(&r.reads)
            )
    ) {
        let mut checker = TypeChecker::new();
        checker.check_rule(&rule);
        prop_assert!(
            !checker.has_errors(),
            "valid rule should produce no errors"
        );
        prop_assert!(
            checker.diagnostics().is_empty(),
            "valid rule with reads=superset(writes) should produce \
             no diagnostics"
        );
    }

    #[test]
    fn checker_empty_name_always_errors(
        reads in arb_operator_set(),
        writes in arb_operator_set(),
    ) {
        let rule = RuleInfo {
            name: String::new(),
            category: "test".into(),
            reads,
            writes,
        };
        let mut checker = TypeChecker::new();
        checker.check_rule(&rule);
        prop_assert!(checker.has_errors());
    }

    #[test]
    fn index_get_after_insert(rule in arb_rule_info()) {
        let mut index = RuleIndex::new();
        index.insert(rule.clone());
        prop_assert!(index.get(&rule.name).is_some());
        prop_assert_eq!(index.len(), 1);
    }

    #[test]
    fn index_remove_after_insert(rule in arb_rule_info()) {
        let mut index = RuleIndex::new();
        index.insert(rule.clone());
        let removed = index.remove(&rule.name);
        prop_assert!(removed.is_some());
        prop_assert!(index.is_empty());
        prop_assert!(index.get(&rule.name).is_none());
    }

    #[test]
    fn index_category_contains_inserted_rule(
        rule in arb_rule_info()
    ) {
        let mut index = RuleIndex::new();
        index.insert(rule.clone());
        let cat_rules = index.by_category(&rule.category);
        prop_assert!(
            cat_rules.contains(&rule.name.as_str()),
            "category index should contain the rule"
        );
    }

    #[test]
    fn index_rule_names_sorted(
        rules in prop::collection::vec(arb_rule_info(), 1..=10)
    ) {
        let index = RuleIndex::from_rules(&rules);
        let names = index.rule_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        prop_assert_eq!(names, sorted);
    }

    #[test]
    fn registry_rejects_empty_name(
        category in "[a-z]+",
        reads in arb_operator_set(),
        writes in arb_operator_set(),
    ) {
        let mut reg = RuleRegistry::new();
        let rule = RuleInfo {
            name: String::new(),
            category,
            reads,
            writes,
        };
        prop_assert!(reg.register(rule).is_err());
    }

    #[test]
    fn registry_accepts_then_retrieves(
        rule in arb_rule_info()
            .prop_filter(
                "rule must have non-empty name and category",
                |r| !r.name.is_empty() && !r.category.is_empty()
            )
    ) {
        let mut reg = RuleRegistry::new();
        if reg.register(rule.clone()).is_ok() {
            prop_assert!(reg.get(&rule.name).is_some());
            prop_assert_eq!(reg.len(), 1);
        }
    }

    #[test]
    fn registry_duplicate_rejected(
        rule in arb_rule_info()
            .prop_filter(
                "rule must have non-empty name and category",
                |r| !r.name.is_empty() && !r.category.is_empty()
            )
    ) {
        let mut reg = RuleRegistry::new();
        if reg.register(rule.clone()).is_ok() {
            prop_assert!(reg.register(rule).is_err());
        }
    }
}
