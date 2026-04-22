//! Rule registry mapping rule IDs to numeric indices.
//!
//! This module provides zero-overhead rule tracking using sparse bitmaps.
//! Rules are assigned compile-time numeric IDs, enabling efficient set
//! operations (union, intersection) without string comparisons.
//!
//! # Design
//!
//! - Each rule gets a unique `u16` ID (supports up to 65,536 rules)
//! - Tracking uses `SparseMap` (sparse bitmap) instead of `Vec<String>`
//! - Only enabled when `--rules` flag is passed (CLI) or `track_rules` is set (API)
//! - Zero overhead in production (PostgreSQL, Stoolap) when disabled

use sparsemap::SparseMap;
use std::sync::OnceLock;

/// Rule ID type (supports up to 65,536 rules).
pub type RuleId = u16;

/// Maximum number of rules supported.
pub const MAX_RULES: usize = 65536;

/// Set of applied rules, represented as a sparse bitmap.
#[derive(Debug, Clone)]
pub struct RuleSet {
    bitmap: SparseMap,
}

impl RuleSet {
    /// Create an empty rule set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bitmap: SparseMap::new(),
        }
    }

    /// Record that a rule was applied.
    #[inline]
    pub fn mark_applied(&mut self, rule_id: RuleId) {
        self.bitmap.set(rule_id as u32);
    }

    /// Check if a rule was applied.
    #[must_use]
    #[inline]
    pub fn was_applied(&self, rule_id: RuleId) -> bool {
        self.bitmap.is_set(rule_id as u32)
    }

    /// Count total rules applied.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.bitmap.count()
    }

    /// Union with another rule set (self |= other).
    pub fn union(&mut self, other: &Self) {
        self.bitmap.union(&other.bitmap);
    }

    /// Intersect with another rule set (self &= other).
    pub fn intersect(&mut self, other: &Self) {
        self.bitmap.intersect(&other.bitmap);
    }

    /// Iterate over applied rule IDs.
    pub fn iter(&self) -> impl Iterator<Item = RuleId> + '_ {
        self.bitmap.iter().map(|id| id as RuleId)
    }

    /// Convert to a list of rule names for display.
    #[must_use]
    pub fn to_names(&self) -> Vec<String> {
        self.iter()
            .filter_map(|id| registry().id_to_name(id))
            .map(|s| s.to_owned())
            .collect()
    }
}

impl Default for RuleSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Rule metadata.
#[derive(Debug, Clone)]
pub struct RuleInfo {
    pub id: RuleId,
    pub name: &'static str,
    pub category: &'static str,
}

/// Global rule registry (initialized at compile time).
pub struct RuleRegistry {
    rules: Vec<RuleInfo>,
}

impl RuleRegistry {
    const fn new() -> Self {
        Self { rules: Vec::new() }
    }

    fn init(&mut self) {
        // Register all rules with numeric IDs from rule_priority.rs
        let mut id = 0;

        // Null simplification
        self.register(id, "and-null-left", "null");
        id += 1;
        self.register(id, "and-null-right", "null");
        id += 1;
        self.register(id, "or-null-true-right", "null");
        id += 1;
        self.register(id, "or-null-true-left", "null");
        id += 1;
        self.register(id, "or-null-false-right", "null");
        id += 1;
        self.register(id, "or-null-false-left", "null");
        id += 1;
        self.register(id, "null-eq", "null");
        id += 1;
        self.register(id, "null-ne", "null");
        id += 1;
        self.register(id, "null-lt", "null");
        id += 1;
        self.register(id, "null-le", "null");
        id += 1;
        self.register(id, "null-gt", "null");
        id += 1;
        self.register(id, "null-ge", "null");
        id += 1;
        self.register(id, "is-null-of-null", "null");
        id += 1;
        self.register(id, "is-not-null-of-null", "null");
        id += 1;
        self.register(id, "filter-null-elimination", "null");
        id += 1;
        self.register(id, "null-add-left", "null");
        id += 1;
        self.register(id, "null-add-right", "null");
        id += 1;
        self.register(id, "null-sub-left", "null");
        id += 1;
        self.register(id, "null-sub-right", "null");
        id += 1;
        self.register(id, "null-mul-left", "null");
        id += 1;
        self.register(id, "null-mul-right", "null");
        id += 1;

        // Boolean simplification
        self.register(id, "and-true-left", "boolean");
        id += 1;
        self.register(id, "and-true-right", "boolean");
        id += 1;
        self.register(id, "and-false-left", "boolean");
        id += 1;
        self.register(id, "and-false-right", "boolean");
        id += 1;
        self.register(id, "or-false-left", "boolean");
        id += 1;
        self.register(id, "or-false-right", "boolean");
        id += 1;
        self.register(id, "or-true-left", "boolean");
        id += 1;
        self.register(id, "or-true-right", "boolean");
        id += 1;
        self.register(id, "double-negation", "boolean");
        id += 1;
        self.register(id, "eq-reflexive", "boolean");
        id += 1;
        self.register(id, "ne-reflexive", "boolean");
        id += 1;
        self.register(id, "eq-null", "boolean");
        id += 1;
        self.register(id, "ne-null", "boolean");
        id += 1;
        self.register(id, "not-is-null", "boolean");
        id += 1;
        self.register(id, "not-is-not-null", "boolean");
        id += 1;
        self.register(id, "and-idempotent", "boolean");
        id += 1;
        self.register(id, "or-idempotent", "boolean");
        id += 1;
        self.register(id, "filter-true", "boolean");
        id += 1;

        // Predicate pushdown
        self.register(id, "filter-through-join-left", "pushdown");
        id += 1;
        self.register(id, "filter-through-join-right", "pushdown");
        id += 1;
        self.register(id, "filter-into-join-condition", "pushdown");
        id += 1;
        self.register(id, "filter-merge", "pushdown");
        id += 1;
        self.register(id, "filter-split-and", "pushdown");
        id += 1;
        self.register(id, "filter-through-project", "pushdown");
        id += 1;
        self.register(id, "filter-below-aggregate", "pushdown");
        id += 1;

        // Join reordering
        self.register(id, "join-commutativity", "join");
        id += 1;
        self.register(id, "cross-join-commutativity", "join");
        id += 1;
        self.register(id, "join-associativity-left", "join");
        id += 1;
        self.register(id, "join-associativity-right", "join");
        id += 1;
        self.register(id, "cartesian-to-join", "join");
        id += 1;

        // Projection
        self.register(id, "project-merge", "projection");
        id += 1;

        // Aggregate
        self.register(id, "aggregate-over-aggregate", "aggregate");
        id += 1;

        // Limit/Sort
        self.register(id, "limit-through-project", "limit");
        id += 1;
        self.register(id, "limit-merge", "limit");
        id += 1;
        self.register(id, "sort-below-sort", "sort");
        // Note: id not incremented after last register - it's no longer used
    }

    fn register(&mut self, id: RuleId, name: &'static str, category: &'static str) {
        self.rules.push(RuleInfo { id, name, category });
    }

    /// Get rule name by ID.
    #[must_use]
    pub fn id_to_name(&self, id: RuleId) -> Option<&'static str> {
        self.rules.iter().find(|r| r.id == id).map(|r| r.name)
    }

    /// Get rule ID by name.
    #[must_use]
    pub fn name_to_id(&self, name: &str) -> Option<RuleId> {
        self.rules.iter().find(|r| r.name == name).map(|r| r.id)
    }

    /// Get all registered rules.
    #[must_use]
    pub fn all_rules(&self) -> &[RuleInfo] {
        &self.rules
    }
}

/// Global rule registry instance.
static RULE_REGISTRY: OnceLock<RuleRegistry> = OnceLock::new();

/// Get the global rule registry.
#[must_use]
pub fn registry() -> &'static RuleRegistry {
    RULE_REGISTRY.get_or_init(|| {
        let mut reg = RuleRegistry::new();
        reg.init();
        reg
    })
}

/// Get a rule ID by name, returning Result instead of panicking.
///
/// # Example
/// ```
/// use ra_engine::rule_id;
/// let id = rule_id!("and-true-left")?;
/// ```
#[macro_export]
macro_rules! rule_id {
    ($name:literal) => {{
        $crate::rule_registry::registry()
            .name_to_id($name)
            .ok_or_else(|| anyhow::anyhow!("rule not found: {}", $name))
    }};
}

/// Get a rule ID by name, panicking if not found.
///
/// This is for test code where panicking is acceptable.
/// Production code should use `rule_id!` and handle the Result.
///
/// # Example
/// ```
/// use ra_engine::rule_id_unchecked;
/// let id = rule_id_unchecked!("and-true-left");
/// ```
#[macro_export]
macro_rules! rule_id_unchecked {
    ($name:literal) => {{
        $crate::rule_registry::registry()
            .name_to_id($name)
            .expect(concat!("rule not found: ", $name))
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_set_basic() {
        let mut set = RuleSet::new();
        assert_eq!(set.count(), 0);

        set.mark_applied(5);
        assert!(set.was_applied(5));
        assert!(!set.was_applied(6));
        assert_eq!(set.count(), 1);
    }

    #[test]
    fn test_rule_set_union() {
        let mut set1 = RuleSet::new();
        set1.mark_applied(1);
        set1.mark_applied(2);

        let mut set2 = RuleSet::new();
        set2.mark_applied(2);
        set2.mark_applied(3);

        set1.union(&set2);
        assert!(set1.was_applied(1));
        assert!(set1.was_applied(2));
        assert!(set1.was_applied(3));
        assert_eq!(set1.count(), 3);
    }

    #[test]
    fn test_rule_set_intersect() {
        let mut set1 = RuleSet::new();
        set1.mark_applied(1);
        set1.mark_applied(2);
        set1.mark_applied(3);

        let mut set2 = RuleSet::new();
        set2.mark_applied(2);
        set2.mark_applied(3);
        set2.mark_applied(4);

        set1.intersect(&set2);
        assert!(!set1.was_applied(1));
        assert!(set1.was_applied(2));
        assert!(set1.was_applied(3));
        assert!(!set1.was_applied(4));
        assert_eq!(set1.count(), 2);
    }

    #[test]
    fn test_registry_lookup() {
        let reg = registry();

        // Should have registered rules
        assert!(!reg.all_rules().is_empty());

        // Lookup by ID
        let name = reg.id_to_name(0);
        assert!(name.is_some());

        // Lookup by name
        if let Some(name) = name {
            let id = reg.name_to_id(name);
            assert_eq!(id, Some(0));
        }
    }

    #[test]
    fn test_rule_id_macro() {
        // Test successful lookup
        let id = rule_id!("and-true-left");
        assert!(id.is_ok());
        assert_eq!(id.unwrap(), registry().name_to_id("and-true-left").unwrap());

        // Test failed lookup
        let id = rule_id!("nonexistent-rule");
        assert!(id.is_err());
        assert!(id.unwrap_err().to_string().contains("rule not found"));
    }

    #[test]
    fn test_rule_id_unchecked_macro() {
        // Test successful lookup
        let id = rule_id_unchecked!("and-true-left");
        assert_eq!(id, registry().name_to_id("and-true-left").unwrap());
    }

    #[test]
    #[should_panic(expected = "rule not found: nonexistent-rule")]
    fn test_rule_id_unchecked_panic() {
        let _id = rule_id_unchecked!("nonexistent-rule");
    }
}
