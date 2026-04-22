//! Query feature detection for rule filtering.
//!
//! `QueryFeatureSet` is a compact bitflag representation of the structural
//! and content-type features present in a query. The rule advisor uses it
//! to eliminate rules that require features absent from the current query.

use crate::lazy_rules::LazyQueryPattern;

/// Bitflag set of query features.
///
/// Each bit represents a structural or content-type feature detected in
/// the query tree. Rules declare which features they require via
/// [`RuleAnnotation`](crate::rule_advisor::RuleAnnotation); a rule is
/// excluded when its required features have no overlap with the query's
/// feature set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct QueryFeatureSet(u32);

// Feature bit positions
const HAS_JOIN: u32 = 1 << 0;
const HAS_AGGREGATE: u32 = 1 << 1;
const HAS_SUBQUERY: u32 = 1 << 2;
const HAS_SET_OPS: u32 = 1 << 3;
const HAS_WINDOW: u32 = 1 << 4;
const HAS_SORT: u32 = 1 << 5;
const HAS_LIMIT: u32 = 1 << 6;
const HAS_DISTINCT: u32 = 1 << 7;
const HAS_CTE: u32 = 1 << 8;
const HAS_RECURSIVE_CTE: u32 = 1 << 9;
const HAS_JSON_ACCESS: u32 = 1 << 10;
const HAS_BSON_FUNC: u32 = 1 << 11;
const HAS_VECTOR_DISTANCE: u32 = 1 << 12;
const HAS_FTS_MATCH: u32 = 1 << 13;
const HAS_HYBRID_SEARCH: u32 = 1 << 14;
const HAS_XML_FUNC: u32 = 1 << 15;
const HAS_CAST: u32 = 1 << 16;
const MULTI_TABLE: u32 = 1 << 17;
const IS_UNIVERSAL: u32 = 1 << 31;

#[allow(clippy::missing_docs_in_private_items)]
impl QueryFeatureSet {
    pub const HAS_JOIN: Self = Self(HAS_JOIN);
    pub const HAS_AGGREGATE: Self = Self(HAS_AGGREGATE);
    pub const HAS_SUBQUERY: Self = Self(HAS_SUBQUERY);
    pub const HAS_SET_OPS: Self = Self(HAS_SET_OPS);
    pub const HAS_WINDOW: Self = Self(HAS_WINDOW);
    pub const HAS_SORT: Self = Self(HAS_SORT);
    pub const HAS_LIMIT: Self = Self(HAS_LIMIT);
    pub const HAS_DISTINCT: Self = Self(HAS_DISTINCT);
    pub const HAS_CTE: Self = Self(HAS_CTE);
    pub const HAS_RECURSIVE_CTE: Self = Self(HAS_RECURSIVE_CTE);
    pub const HAS_JSON_ACCESS: Self = Self(HAS_JSON_ACCESS);
    pub const HAS_BSON_FUNC: Self = Self(HAS_BSON_FUNC);
    pub const HAS_VECTOR_DISTANCE: Self = Self(HAS_VECTOR_DISTANCE);
    pub const HAS_FTS_MATCH: Self = Self(HAS_FTS_MATCH);
    pub const HAS_HYBRID_SEARCH: Self = Self(HAS_HYBRID_SEARCH);
    pub const HAS_XML_FUNC: Self = Self(HAS_XML_FUNC);
    pub const HAS_CAST: Self = Self(HAS_CAST);
    pub const MULTI_TABLE: Self = Self(MULTI_TABLE);
    pub const UNIVERSAL: Self = Self(IS_UNIVERSAL);

    /// Empty feature set (no features detected).
    pub const EMPTY: Self = Self(0);

    /// Create a feature set from raw bits.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Get the raw bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Check if this feature set is marked as universal
    /// (matches any query).
    #[must_use]
    pub const fn is_universal(self) -> bool {
        self.0 & IS_UNIVERSAL != 0
    }

    /// Check if this set is empty (no features).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check if this set contains all features in `other`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Check if this set has any overlap with `other`.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Union of two feature sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Number of feature bits set.
    #[must_use]
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// Build a `QueryFeatureSet` from a `LazyQueryPattern`.
    #[must_use]
    pub fn from_pattern(pattern: &LazyQueryPattern) -> Self {
        let mut bits: u32 = 0;

        if pattern.has_joins {
            bits |= HAS_JOIN;
        }
        if pattern.has_aggregates {
            bits |= HAS_AGGREGATE;
        }
        if pattern.has_subqueries {
            bits |= HAS_SUBQUERY;
        }
        if pattern.has_set_ops {
            bits |= HAS_SET_OPS;
        }
        if pattern.has_window_functions {
            bits |= HAS_WINDOW;
        }
        if pattern.has_sorting {
            bits |= HAS_SORT;
        }
        if pattern.has_limits {
            bits |= HAS_LIMIT;
        }
        if pattern.has_distinct {
            bits |= HAS_DISTINCT;
        }
        if pattern.has_cte {
            bits |= HAS_CTE;
        }
        if pattern.has_recursive_cte {
            bits |= HAS_RECURSIVE_CTE;
        }
        if pattern.has_json_access {
            bits |= HAS_JSON_ACCESS;
        }
        if pattern.has_bson_func {
            bits |= HAS_BSON_FUNC;
        }
        if pattern.has_vector_distance {
            bits |= HAS_VECTOR_DISTANCE;
        }
        if pattern.has_fts_match {
            bits |= HAS_FTS_MATCH;
        }
        if pattern.has_xml_func {
            bits |= HAS_XML_FUNC;
        }
        if pattern.has_cast {
            bits |= HAS_CAST;
        }
        if pattern.table_count > 1 {
            bits |= MULTI_TABLE;
        }

        Self(bits)
    }
}

impl std::ops::BitOr for QueryFeatureSet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for QueryFeatureSet {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::fmt::Display for QueryFeatureSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names = Vec::new();
        if self.0 & HAS_JOIN != 0 {
            names.push("join");
        }
        if self.0 & HAS_AGGREGATE != 0 {
            names.push("aggregate");
        }
        if self.0 & HAS_SUBQUERY != 0 {
            names.push("subquery");
        }
        if self.0 & HAS_SET_OPS != 0 {
            names.push("set_ops");
        }
        if self.0 & HAS_WINDOW != 0 {
            names.push("window");
        }
        if self.0 & HAS_SORT != 0 {
            names.push("sort");
        }
        if self.0 & HAS_LIMIT != 0 {
            names.push("limit");
        }
        if self.0 & HAS_DISTINCT != 0 {
            names.push("distinct");
        }
        if self.0 & HAS_CTE != 0 {
            names.push("cte");
        }
        if self.0 & HAS_RECURSIVE_CTE != 0 {
            names.push("recursive_cte");
        }
        if self.0 & HAS_JSON_ACCESS != 0 {
            names.push("json_access");
        }
        if self.0 & HAS_BSON_FUNC != 0 {
            names.push("bson_func");
        }
        if self.0 & HAS_VECTOR_DISTANCE != 0 {
            names.push("vector_distance");
        }
        if self.0 & HAS_FTS_MATCH != 0 {
            names.push("fts_match");
        }
        if self.0 & HAS_HYBRID_SEARCH != 0 {
            names.push("hybrid_search");
        }
        if self.0 & HAS_XML_FUNC != 0 {
            names.push("xml_func");
        }
        if self.0 & HAS_CAST != 0 {
            names.push("cast");
        }
        if self.0 & MULTI_TABLE != 0 {
            names.push("multi_table");
        }
        if self.0 & IS_UNIVERSAL != 0 {
            names.push("UNIVERSAL");
        }
        if names.is_empty() {
            write!(f, "(none)")
        } else {
            write!(f, "{}", names.join("|"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_feature_set() {
        let fs = QueryFeatureSet::EMPTY;
        assert!(fs.is_empty());
        assert!(!fs.is_universal());
        assert_eq!(fs.count(), 0);
    }

    #[test]
    fn universal_feature_set() {
        let fs = QueryFeatureSet::UNIVERSAL;
        assert!(fs.is_universal());
        assert!(!fs.is_empty());
    }

    #[test]
    fn union_and_intersect() {
        let a = QueryFeatureSet::HAS_JOIN | QueryFeatureSet::HAS_AGGREGATE;
        let b = QueryFeatureSet::HAS_AGGREGATE | QueryFeatureSet::HAS_SORT;
        let u = a.union(b);
        assert!(u.contains(QueryFeatureSet::HAS_JOIN));
        assert!(u.contains(QueryFeatureSet::HAS_AGGREGATE));
        assert!(u.contains(QueryFeatureSet::HAS_SORT));

        let i = a & b;
        assert!(i.contains(QueryFeatureSet::HAS_AGGREGATE));
        assert!(!i.contains(QueryFeatureSet::HAS_JOIN));
    }

    #[test]
    fn intersects_check() {
        let a = QueryFeatureSet::HAS_JOIN;
        let b = QueryFeatureSet::HAS_JOIN | QueryFeatureSet::HAS_SORT;
        let c = QueryFeatureSet::HAS_SORT;
        assert!(a.intersects(b));
        assert!(!a.intersects(c));
    }

    #[test]
    fn from_pattern_basic() {
        let pattern = LazyQueryPattern {
            has_joins: true,
            has_aggregates: true,
            table_count: 3,
            ..LazyQueryPattern::default()
        };
        let fs = QueryFeatureSet::from_pattern(&pattern);
        assert!(fs.contains(QueryFeatureSet::HAS_JOIN));
        assert!(fs.contains(QueryFeatureSet::HAS_AGGREGATE));
        assert!(fs.contains(QueryFeatureSet::MULTI_TABLE));
        assert!(!fs.contains(QueryFeatureSet::HAS_VECTOR_DISTANCE));
    }

    #[test]
    fn display_formatting() {
        let fs = QueryFeatureSet::HAS_JOIN | QueryFeatureSet::HAS_FTS_MATCH;
        let s = fs.to_string();
        assert!(s.contains("join"));
        assert!(s.contains("fts_match"));
    }
}
