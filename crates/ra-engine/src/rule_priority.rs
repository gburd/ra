//! Rule complexity prioritization (RFC 0058).
//!
//! Assigns priority scores to rewrite rules based on their
//! complexity class and expected benefit. Rules with higher
//! priority (high benefit, low complexity) are applied first
//! during equality saturation, reducing optimization time by
//! exploring the most promising rewrites early.
//!
//! # Priority formula
//!
//! ```text
//! priority = expected_benefit / complexity_weight
//! ```
//!
//! Where `expected_benefit` is the midpoint of the rule's benefit
//! range and `complexity_weight` is derived from the complexity
//! class (O(1)=1, O(n)=2, O(n^2)=4, O(exp)=8).

use egg::Rewrite;
use std::collections::HashMap;

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::rule_metadata::{BenefitRange, ComplexityClass};

/// Priority metadata for a single rewrite rule.
#[derive(Debug, Clone, Copy)]
pub struct RulePriority {
    pub complexity: ComplexityClass,
    pub benefit: BenefitRange,
    pub score: f64,
}

impl RulePriority {
    /// Compute priority score from complexity and benefit.
    #[must_use]
    pub fn new(complexity: ComplexityClass, benefit: BenefitRange) -> Self {
        let score = compute_priority(complexity, benefit);
        Self {
            complexity,
            benefit,
            score,
        }
    }
}

/// Compute a priority score for a rule.
///
/// Higher score = should be applied earlier.
/// Formula: `expected_benefit / complexity_weight`
#[must_use]
pub fn compute_priority(complexity: ComplexityClass, benefit: BenefitRange) -> f64 {
    benefit.expected() / complexity.weight()
}

/// Default priority annotations for known rule names.
///
/// Maps rule name (the string label in egg `rewrite!`) to
/// `(ComplexityClass, BenefitRange)`. Rules not present here
/// get a default middle-of-the-road priority so they still
/// participate in optimization.
#[must_use]
pub fn default_rule_priorities() -> HashMap<&'static str, (ComplexityClass, BenefitRange)> {
    let mut m = HashMap::with_capacity(120);

    // Helper closures to reduce boilerplate
    let o1 = ComplexityClass::O1;
    let on = ComplexityClass::On;
    let on2 = ComplexityClass::On2;
    let br = BenefitRange::new;

    // -- Null simplification (highest priority: cheap, always beneficial) --
    m.insert("and-null-left", (o1, br(0.3, 0.7)));
    m.insert("and-null-right", (o1, br(0.3, 0.7)));
    m.insert("or-null-true-right", (o1, br(0.2, 0.5)));
    m.insert("or-null-true-left", (o1, br(0.2, 0.5)));
    m.insert("or-null-false-right", (o1, br(0.2, 0.5)));
    m.insert("or-null-false-left", (o1, br(0.2, 0.5)));
    m.insert("null-eq", (o1, br(0.3, 0.6)));
    m.insert("null-ne", (o1, br(0.3, 0.6)));
    m.insert("null-lt", (o1, br(0.3, 0.6)));
    m.insert("null-le", (o1, br(0.3, 0.6)));
    m.insert("null-gt", (o1, br(0.3, 0.6)));
    m.insert("null-ge", (o1, br(0.3, 0.6)));
    m.insert("is-null-of-null", (o1, br(0.4, 0.8)));
    m.insert("is-not-null-of-null", (o1, br(0.4, 0.8)));
    m.insert("filter-null-elimination", (o1, br(0.5, 0.9)));
    m.insert("null-add-left", (o1, br(0.2, 0.5)));
    m.insert("null-add-right", (o1, br(0.2, 0.5)));
    m.insert("null-sub-left", (o1, br(0.2, 0.5)));
    m.insert("null-sub-right", (o1, br(0.2, 0.5)));
    m.insert("null-mul-left", (o1, br(0.2, 0.5)));
    m.insert("null-mul-right", (o1, br(0.2, 0.5)));

    // -- Boolean simplification (cheap constant folding) --
    m.insert("and-true-left", (o1, br(0.3, 0.6)));
    m.insert("and-true-right", (o1, br(0.3, 0.6)));
    m.insert("and-false-left", (o1, br(0.5, 0.9)));
    m.insert("and-false-right", (o1, br(0.5, 0.9)));
    m.insert("or-false-left", (o1, br(0.3, 0.6)));
    m.insert("or-false-right", (o1, br(0.3, 0.6)));
    m.insert("or-true-left", (o1, br(0.5, 0.9)));
    m.insert("or-true-right", (o1, br(0.5, 0.9)));
    m.insert("double-negation", (o1, br(0.2, 0.5)));
    m.insert("eq-reflexive", (o1, br(0.4, 0.8)));
    m.insert("ne-reflexive", (o1, br(0.4, 0.8)));
    m.insert("eq-null", (o1, br(0.3, 0.6)));
    m.insert("ne-null", (o1, br(0.3, 0.6)));
    m.insert("not-is-null", (o1, br(0.1, 0.3)));
    m.insert("not-is-not-null", (o1, br(0.1, 0.3)));
    m.insert("and-idempotent", (o1, br(0.2, 0.5)));
    m.insert("or-idempotent", (o1, br(0.2, 0.5)));
    m.insert("filter-true", (o1, br(0.6, 1.0)));

    // -- Arithmetic simplification --
    m.insert("add-zero-right", (o1, br(0.1, 0.3)));
    m.insert("add-zero-left", (o1, br(0.1, 0.3)));
    m.insert("mul-one-right", (o1, br(0.1, 0.3)));
    m.insert("mul-one-left", (o1, br(0.1, 0.3)));
    m.insert("mul-zero-right", (o1, br(0.3, 0.7)));
    m.insert("mul-zero-left", (o1, br(0.3, 0.7)));
    m.insert("sub-zero", (o1, br(0.1, 0.3)));
    m.insert("double-neg-arith", (o1, br(0.1, 0.3)));
    m.insert("duckdb-sub-self", (o1, br(0.2, 0.5)));

    // -- Predicate pushdown (high benefit, linear complexity) --
    m.insert("filter-through-join-left", (on, br(0.5, 0.9)));
    m.insert("filter-through-join-right", (on, br(0.5, 0.9)));
    m.insert("filter-into-join-condition", (on, br(0.4, 0.8)));
    m.insert("filter-merge", (o1, br(0.3, 0.6)));
    m.insert("filter-split-and", (o1, br(0.2, 0.5)));
    m.insert("filter-through-project", (o1, br(0.3, 0.7)));
    m.insert("filter-through-union", (on, br(0.3, 0.6)));
    m.insert("filter-through-intersect", (on, br(0.3, 0.6)));
    m.insert("filter-through-except-left", (on, br(0.2, 0.5)));
    m.insert("filter-below-aggregate", (on, br(0.5, 0.9)));
    m.insert("duckdb-filter-through-left-join-left", (on, br(0.4, 0.8)));

    // -- Join reordering (high benefit, higher complexity) --
    m.insert("join-commutativity", (o1, br(0.2, 0.8)));
    m.insert("cross-join-commutativity", (o1, br(0.1, 0.5)));
    m.insert("join-associativity-left", (on2, br(0.3, 0.9)));
    m.insert("join-associativity-right", (on2, br(0.3, 0.9)));
    m.insert("cartesian-to-join", (o1, br(0.7, 1.0)));
    m.insert("left-outer-to-inner-with-filter", (on, br(0.4, 0.8)));
    m.insert("right-outer-to-inner-with-filter", (on, br(0.4, 0.8)));

    // -- Projection pushdown --
    m.insert("project-merge", (o1, br(0.2, 0.5)));
    m.insert("duckdb-project-pushdown", (o1, br(0.2, 0.5)));

    // -- Join elimination --
    m.insert("cross-join-single-row-right", (o1, br(0.5, 0.9)));

    // -- Aggregate optimization --
    m.insert("aggregate-over-aggregate", (o1, br(0.4, 0.8)));

    // -- Limit/Sort optimization --
    m.insert("limit-through-project", (o1, br(0.2, 0.5)));
    m.insert("limit-merge", (o1, br(0.3, 0.6)));
    m.insert("sort-below-sort", (o1, br(0.3, 0.7)));
    m.insert("duckdb-limit-through-union", (on, br(0.2, 0.5)));
    m.insert("duckdb-sort-below-aggregate", (o1, br(0.4, 0.8)));

    // -- Set operations --
    m.insert("union-commutativity", (o1, br(0.1, 0.3)));
    m.insert("intersect-commutativity", (o1, br(0.1, 0.3)));
    m.insert("union-self-all", (o1, br(0.5, 0.9)));
    m.insert("intersect-self", (o1, br(0.5, 0.9)));
    m.insert("except-self", (o1, br(0.5, 0.9)));

    // -- Subquery optimization --
    m.insert("filter-semi-join-merge", (o1, br(0.3, 0.6)));
    m.insert("filter-anti-join-merge", (o1, br(0.3, 0.6)));

    // -- DuckDB-inspired comparison simplification --
    m.insert("duckdb-not-lt", (o1, br(0.1, 0.3)));
    m.insert("duckdb-not-le", (o1, br(0.1, 0.3)));
    m.insert("duckdb-not-gt", (o1, br(0.1, 0.3)));
    m.insert("duckdb-not-ge", (o1, br(0.1, 0.3)));
    m.insert("duckdb-not-eq", (o1, br(0.1, 0.3)));
    m.insert("duckdb-not-ne", (o1, br(0.1, 0.3)));

    // -- SQLite-inspired rules --
    m.insert("sqlite-range-to-eq", (o1, br(0.3, 0.6)));
    m.insert("sqlite-eq-transitive", (on, br(0.2, 0.6)));
    m.insert("sqlite-not-in-to-anti", (o1, br(0.2, 0.5)));
    m.insert("sqlite-or-distribute", (o1, br(0.2, 0.5)));
    m.insert("sqlite-eq-implies-not-null-left", (o1, br(0.3, 0.6)));
    m.insert("sqlite-eq-implies-not-null-right", (o1, br(0.3, 0.6)));
    m.insert("sqlite-const-prop-join", (on, br(0.3, 0.7)));

    // -- Commutativity rules (low benefit, just for canonicalization) --
    m.insert("add-commutative", (o1, br(0.0, 0.1)));
    m.insert("mul-commutative", (o1, br(0.0, 0.1)));
    m.insert("eq-commutative", (o1, br(0.0, 0.1)));
    m.insert("ne-commutative", (o1, br(0.0, 0.1)));
    m.insert("and-commutative", (o1, br(0.0, 0.1)));
    m.insert("or-commutative", (o1, br(0.0, 0.1)));
    m.insert("lt-to-gt", (o1, br(0.0, 0.1)));
    m.insert("gt-to-lt", (o1, br(0.0, 0.1)));
    m.insert("le-to-ge", (o1, br(0.0, 0.1)));
    m.insert("ge-to-le", (o1, br(0.0, 0.1)));

    // -- De Morgan's rules --
    m.insert("demorgan-and-to-or", (o1, br(0.1, 0.3)));
    m.insert("demorgan-or-to-and", (o1, br(0.1, 0.3)));

    // -- Runtime filter rules (higher complexity, high benefit) --
    m.insert("runtime-filter-hash-to-semi", (on, br(0.3, 0.8)));
    m.insert("runtime-filter-through-project", (o1, br(0.2, 0.5)));
    m.insert("runtime-filter-through-filter", (o1, br(0.2, 0.5)));

    // -- Consensus rules --
    m.insert("extract-equijoin-from-and-left", (o1, br(0.5, 0.9)));
    m.insert("extract-equijoin-from-and-right", (o1, br(0.5, 0.9)));
    m.insert("filter-null-join-key-left", (on, br(0.3, 0.7)));
    m.insert("filter-null-join-key-right", (on, br(0.3, 0.7)));
    m.insert("empty-filter", (o1, br(0.5, 0.9)));
    m.insert("empty-join-left", (o1, br(0.5, 0.9)));
    m.insert("empty-join-right", (o1, br(0.5, 0.9)));
    m.insert("empty-project", (o1, br(0.5, 0.9)));
    m.insert("empty-aggregate", (o1, br(0.5, 0.9)));
    m.insert("empty-union-left", (o1, br(0.4, 0.7)));
    m.insert("empty-union-right", (o1, br(0.4, 0.7)));

    // -- Join transformations --
    m.insert("left-outer-to-inner-with-comparison", (on, br(0.4, 0.8)));
    m.insert("right-outer-to-inner-with-comparison", (on, br(0.4, 0.8)));
    m.insert("outer-to-semi-exists", (on, br(0.3, 0.7)));
    m.insert("outer-to-anti-not-exists", (on, br(0.3, 0.7)));

    // -- Parquet pushdown --
    m.insert("parquet-filter-split-for-pushdown", (o1, br(0.3, 0.7)));

    // -- Metadata shortcuts --
    m.insert("count-star-to-metadata", (o1, br(0.7, 1.0)));
    m.insert("count-col-to-metadata", (o1, br(0.6, 0.9)));

    // -- Covering index --
    m.insert("scan-to-index-only-scan", (on, br(0.4, 0.9)));

    // -- MIN/MAX index --
    m.insert("min-to-index-scan", (o1, br(0.5, 0.9)));
    m.insert("max-to-index-scan", (o1, br(0.5, 0.9)));

    // -- MV rewrite --
    m.insert("mv-agg-rewrite", (on2, br(0.5, 1.0)));

    m
}

/// Default priority for rules without explicit annotations.
///
/// Uses O(n) complexity and moderate benefit so unclassified rules
/// end up in the middle of the priority ordering.
pub const DEFAULT_COMPLEXITY: ComplexityClass = ComplexityClass::On;
pub const DEFAULT_BENEFIT: BenefitRange = BenefitRange { min: 0.2, max: 0.5 };

/// Sort rewrite rules by priority (highest priority first).
///
/// Rules with known priority annotations are sorted by
/// `benefit / complexity_weight` descending. Unknown rules
/// get a default middle-of-the-road score.
///
/// This is backwards-compatible: rules without metadata are
/// still included, just with default ordering.
pub fn sort_rules_by_priority(
    rules: Vec<Rewrite<RelLang, RelAnalysis>>,
) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let priorities = default_rule_priorities();

    let mut scored: Vec<(f64, usize, Rewrite<RelLang, RelAnalysis>)> = rules
        .into_iter()
        .enumerate()
        .map(|(idx, rule)| {
            let name = rule.name.as_str();
            let score = if let Some(&(complexity, benefit)) = priorities.get(name) {
                compute_priority(complexity, benefit)
            } else {
                compute_priority(DEFAULT_COMPLEXITY, DEFAULT_BENEFIT)
            };
            (score, idx, rule)
        })
        .collect();

    // Sort by score descending; break ties by original index
    // to preserve relative order of equally-scored rules.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });

    scored.into_iter().map(|(_, _, rule)| rule).collect()
}

/// Get the priority score for a named rule (for diagnostics).
#[must_use]
pub fn rule_score(name: &str) -> f64 {
    let priorities = default_rule_priorities();
    if let Some(&(complexity, benefit)) = priorities.get(name) {
        compute_priority(complexity, benefit)
    } else {
        compute_priority(DEFAULT_COMPLEXITY, DEFAULT_BENEFIT)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn complexity_weight_ordering() {
        assert!(ComplexityClass::O1.weight() < ComplexityClass::On.weight());
        assert!(ComplexityClass::On.weight() < ComplexityClass::On2.weight());
        assert!(ComplexityClass::On2.weight() < ComplexityClass::Oexp.weight());
    }

    #[test]
    fn benefit_range_clamping() {
        let br = BenefitRange::new(-0.5, 1.5);
        assert!((br.min - 0.0).abs() < f64::EPSILON);
        assert!((br.max - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn benefit_range_expected_midpoint() {
        let br = BenefitRange::new(0.2, 0.8);
        assert!((br.expected() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn priority_higher_for_cheaper_rules() {
        let cheap = compute_priority(ComplexityClass::O1, BenefitRange::new(0.5, 0.9));
        let expensive = compute_priority(ComplexityClass::On2, BenefitRange::new(0.5, 0.9));
        assert!(cheap > expensive);
    }

    #[test]
    fn priority_higher_for_more_beneficial_rules() {
        let high_benefit = compute_priority(ComplexityClass::On, BenefitRange::new(0.7, 1.0));
        let low_benefit = compute_priority(ComplexityClass::On, BenefitRange::new(0.0, 0.2));
        assert!(high_benefit > low_benefit);
    }

    #[test]
    fn sort_puts_high_priority_first() {
        use egg::rewrite;
        let rules: Vec<Rewrite<RelLang, RelAnalysis>> = vec![
            // Commutativity: low benefit (score ~ 0.05/1.0 = 0.05)
            rewrite!("add-commutative";
                "(add ?x ?y)" => "(add ?y ?x)"
            ),
            // Filter true: high benefit (score ~ 0.8/1.0 = 0.8)
            rewrite!("filter-true";
                "(filter (const-bool true) ?input)" => "?input"
            ),
            // Join assoc: medium benefit, quadratic
            // (score ~ 0.6/4.0 = 0.15)
            rewrite!("join-associativity-left";
                "(join inner ?c1 (join inner ?c2 ?a ?b) ?c)" =>
                "(join inner ?c2 ?a (join inner ?c1 ?b ?c))"
            ),
        ];

        let sorted = sort_rules_by_priority(rules);
        assert_eq!(sorted[0].name.as_str(), "filter-true");
        assert_eq!(sorted[1].name.as_str(), "join-associativity-left");
        assert_eq!(sorted[2].name.as_str(), "add-commutative");
    }

    #[test]
    fn unknown_rules_get_default_score() {
        let score = rule_score("some-unknown-rule-xyz");
        let default = compute_priority(DEFAULT_COMPLEXITY, DEFAULT_BENEFIT);
        assert!((score - default).abs() < f64::EPSILON);
    }

    #[test]
    fn default_priorities_cover_high_value_rules() {
        let priorities = default_rule_priorities();
        // Verify some high-value rules are annotated
        assert!(priorities.contains_key("filter-through-join-left"));
        assert!(priorities.contains_key("join-commutativity"));
        assert!(priorities.contains_key("filter-true"));
        assert!(priorities.contains_key("cartesian-to-join"));
        assert!(priorities.contains_key("count-star-to-metadata"));
        assert!(priorities.len() >= 100);
    }

    #[test]
    fn complexity_class_display() {
        assert_eq!(format!("{}", ComplexityClass::O1), "O(1)");
        assert_eq!(format!("{}", ComplexityClass::On), "O(n)");
        assert_eq!(format!("{}", ComplexityClass::On2), "O(n^2)");
        assert_eq!(format!("{}", ComplexityClass::Oexp), "O(exp)");
    }

    #[test]
    fn complexity_class_serde_roundtrip() {
        let json = serde_json::to_string(&ComplexityClass::On2).unwrap();
        let back: ComplexityClass = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ComplexityClass::On2);
    }
}
