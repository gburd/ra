//! Rule dependency analyzer.
//!
//! Analyzes rule dependencies and potential conflicts to support
//! incremental optimization. When rules are added or removed, the
//! analyzer identifies which other rules are affected and which
//! queries need reoptimization.

use std::collections::{HashMap, HashSet};

/// A named operator category that rules can touch.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OperatorKind {
    /// Table scan operator.
    Scan,
    /// Filter / selection operator.
    Filter,
    /// Projection operator.
    Project,
    /// Join operator (any join type).
    Join,
    /// Aggregation / GROUP BY.
    Aggregate,
    /// Sort / ORDER BY.
    Sort,
    /// Limit / offset.
    Limit,
    /// Set operations (UNION, INTERSECT, EXCEPT).
    SetOp,
}

/// Metadata about a single rewrite rule.
#[derive(Debug, Clone)]
pub struct RuleInfo {
    /// The rule's unique name.
    pub name: String,
    /// The category this rule belongs to.
    pub category: String,
    /// Which operator kinds this rule reads (pattern side).
    pub reads: HashSet<OperatorKind>,
    /// Which operator kinds this rule writes (replacement side).
    pub writes: HashSet<OperatorKind>,
}

/// A dependency edge between two rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDependency {
    /// The rule that produces output.
    pub producer: String,
    /// The rule that consumes the output.
    pub consumer: String,
    /// The operator kinds where they overlap.
    pub shared_operators: HashSet<OperatorKind>,
}

/// Analyzes rule dependencies and conflicts.
///
/// The analyzer builds a dependency graph based on which
/// operators each rule reads and writes. Two rules are
/// dependent if one writes to operator kinds that the other
/// reads.
#[derive(Debug, Default)]
pub struct Analyzer {
    rules: HashMap<String, RuleInfo>,
}

impl Analyzer {
    /// Create an empty analyzer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a rule with its metadata.
    pub fn add_rule(&mut self, info: RuleInfo) {
        self.rules.insert(info.name.clone(), info);
    }

    /// Return the number of registered rules.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Get info for a specific rule.
    #[must_use]
    pub fn get_rule(&self, name: &str) -> Option<&RuleInfo> {
        self.rules.get(name)
    }

    /// Compute all dependencies between registered rules.
    ///
    /// A dependency exists from rule A to rule B if A writes
    /// to an operator kind that B reads. This means changes
    /// to A's output may affect B's behavior.
    #[must_use]
    pub fn compute_dependencies(&self) -> Vec<RuleDependency> {
        let mut deps = Vec::new();

        let rule_list: Vec<&RuleInfo> = self.rules.values().collect();

        for producer in &rule_list {
            for consumer in &rule_list {
                if producer.name == consumer.name {
                    continue;
                }
                let shared: HashSet<OperatorKind> = producer
                    .writes
                    .intersection(&consumer.reads)
                    .cloned()
                    .collect();

                if !shared.is_empty() {
                    deps.push(RuleDependency {
                        producer: producer.name.clone(),
                        consumer: consumer.name.clone(),
                        shared_operators: shared,
                    });
                }
            }
        }

        deps
    }

    /// Find all rules that might be affected by a change to
    /// the given rule.
    ///
    /// Returns the names of rules that read operator kinds
    /// that the given rule writes to.
    #[must_use]
    pub fn affected_by(&self, rule_name: &str) -> Vec<String> {
        let Some(rule) = self.rules.get(rule_name) else {
            return Vec::new();
        };

        let mut affected = Vec::new();
        for other in self.rules.values() {
            if other.name == rule.name {
                continue;
            }
            let overlaps = other.reads.intersection(&rule.writes).next().is_some();
            if overlaps {
                affected.push(other.name.clone());
            }
        }

        affected.sort();
        affected
    }

    /// Find all rules in the same category.
    #[must_use]
    pub fn rules_in_category(&self, category: &str) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .rules
            .values()
            .filter(|r| r.category == category)
            .map(|r| r.name.as_str())
            .collect();
        names.sort_unstable();
        names
    }

    /// Detect potential conflicts between rules.
    ///
    /// Two rules conflict if they both write to the same
    /// operator kind, meaning their rewrites may interfere.
    #[must_use]
    pub fn detect_conflicts(&self) -> Vec<(String, String, HashSet<OperatorKind>)> {
        let mut conflicts = Vec::new();
        let rule_list: Vec<&RuleInfo> = self.rules.values().collect();

        for (i, a) in rule_list.iter().enumerate() {
            for b in &rule_list[i + 1..] {
                let shared: HashSet<OperatorKind> =
                    a.writes.intersection(&b.writes).cloned().collect();

                if !shared.is_empty() {
                    conflicts.push((a.name.clone(), b.name.clone(), shared));
                }
            }
        }

        conflicts
    }

    /// Build rule info for common predicate-pushdown rules.
    #[must_use]
    pub fn predicate_pushdown_rules() -> Vec<RuleInfo> {
        vec![
            RuleInfo {
                name: "filter-through-join".into(),
                category: "predicate-pushdown".into(),
                reads: HashSet::from([OperatorKind::Filter, OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Filter, OperatorKind::Join]),
            },
            RuleInfo {
                name: "filter-through-project".into(),
                category: "predicate-pushdown".into(),
                reads: HashSet::from([OperatorKind::Filter, OperatorKind::Project]),
                writes: HashSet::from([OperatorKind::Filter, OperatorKind::Project]),
            },
            RuleInfo {
                name: "filter-merge".into(),
                category: "predicate-pushdown".into(),
                reads: HashSet::from([OperatorKind::Filter]),
                writes: HashSet::from([OperatorKind::Filter]),
            },
        ]
    }

    /// Build rule info for join-reordering rules.
    #[must_use]
    pub fn join_reordering_rules() -> Vec<RuleInfo> {
        vec![
            RuleInfo {
                name: "join-commutativity".into(),
                category: "join-reordering".into(),
                reads: HashSet::from([OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Join]),
            },
            RuleInfo {
                name: "join-associativity".into(),
                category: "join-reordering".into(),
                reads: HashSet::from([OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Join]),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_analyzer() -> Analyzer {
        let mut analyzer = Analyzer::new();
        for rule in Analyzer::predicate_pushdown_rules() {
            analyzer.add_rule(rule);
        }
        for rule in Analyzer::join_reordering_rules() {
            analyzer.add_rule(rule);
        }
        analyzer
    }

    #[test]
    fn new_analyzer_empty() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.rule_count(), 0);
    }

    #[test]
    fn add_and_get_rule() {
        let mut analyzer = Analyzer::new();
        analyzer.add_rule(RuleInfo {
            name: "test".into(),
            category: "test-cat".into(),
            reads: HashSet::new(),
            writes: HashSet::new(),
        });
        assert_eq!(analyzer.rule_count(), 1);
        assert!(analyzer.get_rule("test").is_some());
        assert!(analyzer.get_rule("missing").is_none());
    }

    #[test]
    fn predicate_pushdown_rules_count() {
        let rules = Analyzer::predicate_pushdown_rules();
        assert_eq!(rules.len(), 3);
    }

    #[test]
    fn join_reordering_rules_count() {
        let rules = Analyzer::join_reordering_rules();
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn compute_dependencies_finds_edges() {
        let analyzer = setup_analyzer();
        let deps = analyzer.compute_dependencies();
        // filter-through-join writes Filter, and filter-merge reads
        // Filter. So there should be a dependency.
        let has_dep = deps
            .iter()
            .any(|d| d.producer == "filter-through-join" && d.consumer == "filter-merge");
        assert!(
            has_dep,
            "expected dependency from filter-through-join to filter-merge"
        );
    }

    #[test]
    fn affected_by_filter_merge() {
        let analyzer = setup_analyzer();
        let affected = analyzer.affected_by("filter-merge");
        // filter-merge writes Filter, so rules that read Filter
        // should be affected.
        assert!(
            affected.contains(&"filter-through-join".to_owned()),
            "filter-through-join should be affected"
        );
        assert!(
            affected.contains(&"filter-through-project".to_owned()),
            "filter-through-project should be affected"
        );
    }

    #[test]
    fn affected_by_nonexistent_rule() {
        let analyzer = setup_analyzer();
        let affected = analyzer.affected_by("nonexistent");
        assert!(affected.is_empty());
    }

    #[test]
    fn rules_in_category() {
        let analyzer = setup_analyzer();
        let pp_rules = analyzer.rules_in_category("predicate-pushdown");
        assert_eq!(pp_rules.len(), 3);
        assert!(pp_rules.contains(&"filter-merge"));
    }

    #[test]
    fn rules_in_nonexistent_category() {
        let analyzer = setup_analyzer();
        let rules = analyzer.rules_in_category("nonexistent");
        assert!(rules.is_empty());
    }

    #[test]
    fn detect_conflicts_join_rules() {
        let analyzer = setup_analyzer();
        let conflicts = analyzer.detect_conflicts();
        // join-commutativity and join-associativity both write Join.
        let has_conflict = conflicts.iter().any(|(a, b, ops)| {
            ((a == "join-commutativity" && b == "join-associativity")
                || (a == "join-associativity" && b == "join-commutativity"))
                && ops.contains(&OperatorKind::Join)
        });
        assert!(has_conflict, "expected join rule conflict");
    }

    #[test]
    fn detect_conflicts_filter_rules() {
        let analyzer = setup_analyzer();
        let conflicts = analyzer.detect_conflicts();
        // Multiple filter rules write to Filter.
        let filter_conflicts: Vec<_> = conflicts
            .iter()
            .filter(|(_, _, ops)| ops.contains(&OperatorKind::Filter))
            .collect();
        assert!(
            !filter_conflicts.is_empty(),
            "expected filter rule conflicts"
        );
    }

    #[test]
    fn compute_dependencies_empty_analyzer() {
        let analyzer = Analyzer::new();
        assert!(analyzer.compute_dependencies().is_empty());
    }

    #[test]
    fn compute_dependencies_single_rule() {
        let mut analyzer = Analyzer::new();
        analyzer.add_rule(RuleInfo {
            name: "only".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter]),
        });
        // A single rule cannot depend on itself.
        assert!(analyzer.compute_dependencies().is_empty());
    }

    #[test]
    fn compute_dependencies_no_overlap() {
        let mut analyzer = Analyzer::new();
        analyzer.add_rule(RuleInfo {
            name: "a".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter]),
        });
        analyzer.add_rule(RuleInfo {
            name: "b".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Join]),
            writes: HashSet::from([OperatorKind::Join]),
        });
        // No overlap: a writes Filter, b reads Join (not Filter).
        assert!(analyzer.compute_dependencies().is_empty());
    }

    #[test]
    fn affected_by_rule_with_empty_writes() {
        let mut analyzer = Analyzer::new();
        analyzer.add_rule(RuleInfo {
            name: "no-writes".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::new(),
        });
        analyzer.add_rule(RuleInfo {
            name: "reader".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter]),
        });
        // no-writes has empty writes, so nothing is affected by it.
        assert!(analyzer.affected_by("no-writes").is_empty());
    }

    #[test]
    fn detect_conflicts_no_write_overlap() {
        let mut analyzer = Analyzer::new();
        analyzer.add_rule(RuleInfo {
            name: "a".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter]),
        });
        analyzer.add_rule(RuleInfo {
            name: "b".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Join]),
            writes: HashSet::from([OperatorKind::Join]),
        });
        assert!(analyzer.detect_conflicts().is_empty());
    }

    #[test]
    fn detect_conflicts_empty_analyzer() {
        let analyzer = Analyzer::new();
        assert!(analyzer.detect_conflicts().is_empty());
    }

    #[test]
    fn add_rule_overwrites_existing() {
        let mut analyzer = Analyzer::new();
        analyzer.add_rule(RuleInfo {
            name: "r".into(),
            category: "cat-a".into(),
            reads: HashSet::new(),
            writes: HashSet::new(),
        });
        analyzer.add_rule(RuleInfo {
            name: "r".into(),
            category: "cat-b".into(),
            reads: HashSet::new(),
            writes: HashSet::new(),
        });
        assert_eq!(analyzer.rule_count(), 1);
        assert_eq!(
            analyzer.get_rule("r").map(|r| r.category.as_str()),
            Some("cat-b")
        );
    }

    #[test]
    fn rules_in_category_sorted() {
        let analyzer = setup_analyzer();
        let rules = analyzer.rules_in_category("predicate-pushdown");
        let mut sorted = rules.clone();
        sorted.sort_unstable();
        assert_eq!(rules, sorted);
    }

    #[test]
    fn affected_by_returns_sorted() {
        let analyzer = setup_analyzer();
        let affected = analyzer.affected_by("filter-merge");
        let mut sorted = affected.clone();
        sorted.sort();
        assert_eq!(affected, sorted);
    }

    #[test]
    fn dependency_shared_operators_correct() {
        let mut analyzer = Analyzer::new();
        analyzer.add_rule(RuleInfo {
            name: "producer".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Scan]),
            writes: HashSet::from([OperatorKind::Filter, OperatorKind::Join]),
        });
        analyzer.add_rule(RuleInfo {
            name: "consumer".into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Filter, OperatorKind::Sort]),
            writes: HashSet::from([OperatorKind::Sort]),
        });

        let deps = analyzer.compute_dependencies();
        let dep = deps
            .iter()
            .find(|d| d.producer == "producer" && d.consumer == "consumer");
        assert!(dep.is_some(), "expected a dependency");
        // Only Filter should be shared (producer writes Filter+Join,
        // consumer reads Filter+Sort).
        if let Some(dep) = dep {
            assert_eq!(dep.shared_operators, HashSet::from([OperatorKind::Filter]));
        }
    }

    #[test]
    fn all_operator_kinds_in_enum() {
        // Verify every variant can be constructed and compared.
        let all = [
            OperatorKind::Scan,
            OperatorKind::Filter,
            OperatorKind::Project,
            OperatorKind::Join,
            OperatorKind::Aggregate,
            OperatorKind::Sort,
            OperatorKind::Limit,
            OperatorKind::SetOp,
        ];
        let set: HashSet<_> = all.iter().collect();
        assert_eq!(set.len(), 8);
    }

    #[test]
    fn rule_info_fields_accessible() {
        let info = RuleInfo {
            name: "test-name".into(),
            category: "test-cat".into(),
            reads: HashSet::from([OperatorKind::Scan]),
            writes: HashSet::from([OperatorKind::Filter]),
        };
        assert_eq!(info.name, "test-name");
        assert_eq!(info.category, "test-cat");
        assert!(info.reads.contains(&OperatorKind::Scan));
        assert!(info.writes.contains(&OperatorKind::Filter));
    }

    #[test]
    fn rule_dependency_fields_accessible() {
        let dep = RuleDependency {
            producer: "p".into(),
            consumer: "c".into(),
            shared_operators: HashSet::from([OperatorKind::Join]),
        };
        assert_eq!(dep.producer, "p");
        assert_eq!(dep.consumer, "c");
        assert!(dep.shared_operators.contains(&OperatorKind::Join));
    }

    #[test]
    fn rule_info_clone() {
        let info = RuleInfo {
            name: "original".into(),
            category: "cat".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Join]),
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, info.name);
        assert_eq!(cloned.reads, info.reads);
    }

    #[test]
    fn operator_kind_clone_and_eq() {
        let a = OperatorKind::Filter;
        let b = a.clone();
        assert_eq!(a, b);
    }
}
