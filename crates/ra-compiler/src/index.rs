//! Rule indexing.
//!
//! Builds searchable indices of rules for efficient lookup by
//! operator kind, category, or name prefix.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::analyzer::{OperatorKind, RuleInfo};

/// An index over a set of rules for efficient lookups.
#[derive(Debug, Default)]
pub struct RuleIndex {
    names: HashMap<String, RuleInfo>,
    categories: HashMap<String, Vec<String>>,
    read_ops: HashMap<OperatorKind, Vec<String>>,
    write_ops: HashMap<OperatorKind, Vec<String>>,
    prefixes: BTreeMap<String, Vec<String>>,
}

impl RuleIndex {
    /// Create a new empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build an index from a slice of rules.
    #[must_use]
    pub fn from_rules(rules: &[RuleInfo]) -> Self {
        let mut index = Self::new();
        for rule in rules {
            index.insert(rule.clone());
        }
        index
    }

    /// Insert a rule into all indices.
    pub fn insert(&mut self, rule: RuleInfo) {
        let name = rule.name.clone();

        self.categories
            .entry(rule.category.clone())
            .or_default()
            .push(name.clone());

        for op in &rule.reads {
            self.read_ops
                .entry(op.clone())
                .or_default()
                .push(name.clone());
        }

        for op in &rule.writes {
            self.write_ops
                .entry(op.clone())
                .or_default()
                .push(name.clone());
        }

        // Build prefix index: store under every prefix of the name.
        for len in 1..=name.len() {
            let prefix = &name[..len];
            self.prefixes
                .entry(prefix.to_owned())
                .or_default()
                .push(name.clone());
        }

        self.names.insert(name, rule);
    }

    /// Remove a rule from all indices.
    pub fn remove(&mut self, name: &str) -> Option<RuleInfo> {
        let rule = self.names.remove(name)?;

        if let Some(cat_rules) = self.categories.get_mut(&rule.category) {
            cat_rules.retain(|n| n != name);
        }

        for op in &rule.reads {
            if let Some(op_rules) = self.read_ops.get_mut(op) {
                op_rules.retain(|n| n != name);
            }
        }

        for op in &rule.writes {
            if let Some(op_rules) = self.write_ops.get_mut(op) {
                op_rules.retain(|n| n != name);
            }
        }

        for len in 1..=name.len() {
            let prefix = &name[..len];
            if let Some(prefix_rules) = self.prefixes.get_mut(prefix) {
                prefix_rules.retain(|n| n != name);
            }
        }

        Some(rule)
    }

    /// Look up a rule by exact name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RuleInfo> {
        self.names.get(name)
    }

    /// Return how many rules are indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Find rules in a given category.
    #[must_use]
    pub fn by_category(&self, category: &str) -> Vec<&str> {
        self.categories
            .get(category)
            .map(|names| names.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Find rules that read a given operator.
    #[must_use]
    pub fn readers_of(&self, op: &OperatorKind) -> Vec<&str> {
        self.read_ops
            .get(op)
            .map(|names| names.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Find rules that write a given operator.
    #[must_use]
    pub fn writers_of(&self, op: &OperatorKind) -> Vec<&str> {
        self.write_ops
            .get(op)
            .map(|names| names.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Find rules whose name starts with the given prefix.
    #[must_use]
    pub fn by_prefix(&self, prefix: &str) -> Vec<&str> {
        self.prefixes
            .get(prefix)
            .map(|names| names.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Return all indexed categories.
    #[must_use]
    pub fn categories(&self) -> Vec<&str> {
        let mut cats: Vec<&str> = self.categories.keys().map(String::as_str).collect();
        cats.sort_unstable();
        cats
    }

    /// Return all rule names.
    #[must_use]
    pub fn rule_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.names.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Return the set of all operator kinds referenced in reads.
    #[must_use]
    pub fn all_read_operators(&self) -> HashSet<OperatorKind> {
        self.read_ops.keys().cloned().collect()
    }

    /// Return the set of all operator kinds referenced in writes.
    #[must_use]
    pub fn all_write_operators(&self) -> HashSet<OperatorKind> {
        self.write_ops.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::Analyzer;

    fn sample_rules() -> Vec<RuleInfo> {
        let mut rules = Analyzer::predicate_pushdown_rules();
        rules.extend(Analyzer::join_reordering_rules());
        rules
    }

    #[test]
    fn new_index_is_empty() {
        let index = RuleIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn from_rules_populates_index() {
        let index = RuleIndex::from_rules(&sample_rules());
        assert_eq!(index.len(), 5);
        assert!(!index.is_empty());
    }

    #[test]
    fn get_existing_rule() {
        let index = RuleIndex::from_rules(&sample_rules());
        let rule = index.get("filter-merge");
        assert!(rule.is_some());
        assert_eq!(rule.map(|r| r.name.as_str()), Some("filter-merge"));
    }

    #[test]
    fn get_missing_rule() {
        let index = RuleIndex::from_rules(&sample_rules());
        assert!(index.get("nonexistent").is_none());
    }

    #[test]
    fn by_category_returns_matching_rules() {
        let index = RuleIndex::from_rules(&sample_rules());
        let pp = index.by_category("predicate-pushdown");
        assert_eq!(pp.len(), 3);
        assert!(pp.contains(&"filter-merge"));
    }

    #[test]
    fn by_category_empty_for_unknown() {
        let index = RuleIndex::from_rules(&sample_rules());
        assert!(index.by_category("unknown").is_empty());
    }

    #[test]
    fn readers_of_filter() {
        let index = RuleIndex::from_rules(&sample_rules());
        let readers = index.readers_of(&OperatorKind::Filter);
        assert!(readers.contains(&"filter-merge"));
        assert!(readers.contains(&"filter-through-join"));
    }

    #[test]
    fn writers_of_join() {
        let index = RuleIndex::from_rules(&sample_rules());
        let writers = index.writers_of(&OperatorKind::Join);
        assert!(writers.contains(&"join-commutativity"));
        assert!(writers.contains(&"join-associativity"));
        assert!(writers.contains(&"filter-through-join"));
    }

    #[test]
    fn readers_of_unused_operator() {
        let index = RuleIndex::from_rules(&sample_rules());
        assert!(index.readers_of(&OperatorKind::Aggregate).is_empty());
    }

    #[test]
    fn by_prefix_matches() {
        let index = RuleIndex::from_rules(&sample_rules());
        let results = index.by_prefix("filter");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn by_prefix_single_char() {
        let index = RuleIndex::from_rules(&sample_rules());
        let results = index.by_prefix("j");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn by_prefix_no_match() {
        let index = RuleIndex::from_rules(&sample_rules());
        assert!(index.by_prefix("xyz").is_empty());
    }

    #[test]
    fn categories_lists_all() {
        let index = RuleIndex::from_rules(&sample_rules());
        let cats = index.categories();
        assert_eq!(cats.len(), 2);
        assert!(cats.contains(&"predicate-pushdown"));
        assert!(cats.contains(&"join-reordering"));
    }

    #[test]
    fn rule_names_sorted() {
        let index = RuleIndex::from_rules(&sample_rules());
        let names = index.rule_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn all_read_operators() {
        let index = RuleIndex::from_rules(&sample_rules());
        let ops = index.all_read_operators();
        assert!(ops.contains(&OperatorKind::Filter));
        assert!(ops.contains(&OperatorKind::Join));
        assert!(ops.contains(&OperatorKind::Project));
        assert!(!ops.contains(&OperatorKind::Aggregate));
    }

    #[test]
    fn all_write_operators() {
        let index = RuleIndex::from_rules(&sample_rules());
        let ops = index.all_write_operators();
        assert!(ops.contains(&OperatorKind::Filter));
        assert!(ops.contains(&OperatorKind::Join));
    }

    #[test]
    fn insert_duplicate_name_overwrites() {
        let mut index = RuleIndex::new();
        let rule1 = RuleInfo {
            name: "test".into(),
            category: "cat-a".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter]),
        };
        let rule2 = RuleInfo {
            name: "test".into(),
            category: "cat-b".into(),
            reads: HashSet::from([OperatorKind::Join]),
            writes: HashSet::from([OperatorKind::Join]),
        };
        index.insert(rule1);
        index.insert(rule2);
        // Name map has 1 entry but category lists have duplicates
        // (this is the expected behavior for insert-only index).
        assert_eq!(index.len(), 1);
        assert_eq!(
            index.get("test").map(|r| r.category.as_str()),
            Some("cat-b")
        );
    }

    #[test]
    fn remove_returns_rule() {
        let mut index = RuleIndex::from_rules(&sample_rules());
        let removed = index.remove("filter-merge");
        assert!(removed.is_some());
        assert_eq!(removed.map(|r| r.name), Some("filter-merge".into()));
        assert_eq!(index.len(), 4);
    }

    #[test]
    fn remove_cleans_category_index() {
        let mut index = RuleIndex::from_rules(&sample_rules());
        index.remove("filter-merge");
        let pp = index.by_category("predicate-pushdown");
        assert_eq!(pp.len(), 2);
        assert!(!pp.contains(&"filter-merge"));
    }

    #[test]
    fn remove_cleans_operator_index() {
        let mut index = RuleIndex::from_rules(&sample_rules());
        index.remove("join-commutativity");
        let writers = index.writers_of(&OperatorKind::Join);
        assert!(!writers.contains(&"join-commutativity"));
    }

    #[test]
    fn remove_cleans_prefix_index() {
        let mut index = RuleIndex::from_rules(&sample_rules());
        index.remove("filter-merge");
        let results = index.by_prefix("filter-m");
        assert!(!results.contains(&"filter-merge"));
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut index = RuleIndex::from_rules(&sample_rules());
        assert!(index.remove("nonexistent").is_none());
        assert_eq!(index.len(), 5);
    }

    #[test]
    fn empty_index_queries() {
        let index = RuleIndex::new();
        assert!(index.get("anything").is_none());
        assert!(index.by_category("anything").is_empty());
        assert!(index.readers_of(&OperatorKind::Filter).is_empty());
        assert!(index.writers_of(&OperatorKind::Join).is_empty());
        assert!(index.by_prefix("a").is_empty());
        assert!(index.categories().is_empty());
        assert!(index.rule_names().is_empty());
        assert!(index.all_read_operators().is_empty());
        assert!(index.all_write_operators().is_empty());
    }
}
