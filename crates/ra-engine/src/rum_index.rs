//! PostgreSQL RUM index optimization.
//!
//! Provides cost modeling, query classification, and e-graph rewrite
//! rules for queries that can exploit RUM indexes. RUM extends GIN
//! by storing additional metadata (positions, timestamps) in posting
//! list entries, enabling distance-ordered scans that GIN cannot
//! provide.
//!
//! Key capabilities over GIN:
//! - Distance-ordered retrieval (`<=>` operator) for ranked text search
//! - In-index phrase position verification (no heap recheck)
//! - Addon column ordering (timestamp-ordered text search)
//! - KNN retrieval via depth-first traversal
//!
//! All optimizations are non-fatal: when RUM is not installed or a
//! RUM-specific rewrite fails, the optimizer falls back to standard
//! GIN-based or sequential scan planning.
//!
//! See: `rfcs/text/0079-postgresql-rum-index.md`

use egg::{rewrite, Id, Rewrite, Subst, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

// ------------------------------------------------------------------
// RUM query type classification
// ------------------------------------------------------------------

/// Classification of query patterns that RUM can optimize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RumQueryType {
    /// Boolean match only (`tsvector @@ tsquery`).
    /// RUM is slightly slower than GIN here due to wider postings.
    BooleanMatch,
    /// Ranked retrieval with optional LIMIT.
    /// RUM's distance scan avoids computing rank for all matches.
    RankedRetrieval,
    /// Phrase search using the `<->` proximity operator.
    /// RUM verifies positions in-index without heap recheck.
    PhraseSearch,
    /// Text search with timestamp ordering via addon ops.
    /// RUM provides the ordering natively from the index.
    TimestampOrdered,
    /// K-nearest-neighbor retrieval using distance operators.
    Knn,
}

impl RumQueryType {
    /// Whether this query type benefits from RUM over GIN.
    #[must_use]
    pub fn benefits_from_rum(self) -> bool {
        match self {
            Self::BooleanMatch => false,
            Self::RankedRetrieval | Self::PhraseSearch | Self::TimestampOrdered | Self::Knn => true,
        }
    }

    /// Descriptive label for diagnostics and logging.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::BooleanMatch => "boolean_match",
            Self::RankedRetrieval => "ranked_retrieval",
            Self::PhraseSearch => "phrase_search",
            Self::TimestampOrdered => "timestamp_ordered",
            Self::Knn => "knn",
        }
    }
}

impl std::fmt::Display for RumQueryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ------------------------------------------------------------------
// RUM operator class recognition
// ------------------------------------------------------------------

/// RUM operator classes that Ra recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RumOpclass {
    /// `rum_tsvector_ops`: FTS with distance ordering.
    TsvectorOps,
    /// `rum_tsvector_hash_ops`: Hash-based FTS (no prefix search).
    TsvectorHashOps,
    /// `rum_tsvector_addon_ops`: FTS with additional sort field.
    TsvectorAddonOps,
    /// `rum_tsquery_ops`: Query-side indexing.
    TsqueryOps,
    /// `rum_anyarray_ops`: Array operations with length.
    AnyarrayOps,
    /// Numeric/timestamp distance operators.
    ScalarOps,
}

impl RumOpclass {
    /// Parse an operator class name string.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "rum_tsvector_ops" => Some(Self::TsvectorOps),
            "rum_tsvector_hash_ops" => Some(Self::TsvectorHashOps),
            "rum_tsvector_addon_ops" => Some(Self::TsvectorAddonOps),
            "rum_tsquery_ops" => Some(Self::TsqueryOps),
            "rum_anyarray_ops" => Some(Self::AnyarrayOps),
            s if s.starts_with("rum_") && s.ends_with("_ops") => Some(Self::ScalarOps),
            _ => None,
        }
    }

    /// Whether this operator class supports distance ordering.
    #[must_use]
    pub fn supports_distance_ordering(self) -> bool {
        match self {
            Self::TsvectorOps | Self::TsvectorAddonOps | Self::ScalarOps => true,
            Self::TsvectorHashOps | Self::TsqueryOps | Self::AnyarrayOps => false,
        }
    }

    /// Whether this operator class supports phrase search.
    #[must_use]
    pub fn supports_phrase_search(self) -> bool {
        match self {
            Self::TsvectorOps | Self::TsvectorAddonOps => true,
            Self::TsvectorHashOps | Self::TsqueryOps | Self::AnyarrayOps | Self::ScalarOps => false,
        }
    }

    /// Whether this operator class supports addon column ordering.
    #[must_use]
    pub fn supports_addon_ordering(self) -> bool {
        self == Self::TsvectorAddonOps
    }
}

impl std::fmt::Display for RumOpclass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TsvectorOps => write!(f, "rum_tsvector_ops"),
            Self::TsvectorHashOps => {
                write!(f, "rum_tsvector_hash_ops")
            }
            Self::TsvectorAddonOps => {
                write!(f, "rum_tsvector_addon_ops")
            }
            Self::TsqueryOps => write!(f, "rum_tsquery_ops"),
            Self::AnyarrayOps => write!(f, "rum_anyarray_ops"),
            Self::ScalarOps => write!(f, "rum_scalar_ops"),
        }
    }
}

// ------------------------------------------------------------------
// Cost model
// ------------------------------------------------------------------

/// Cost parameters for RUM index scans.
///
/// RUM posting list entries are wider than GIN (they include positional
/// data and optional addon fields), so per-entry costs are slightly
/// higher. However, ordered retrieval avoids full-scan + sort.
#[derive(Debug, Clone)]
pub struct RumCostParams {
    /// Cost per posting list term lookup.
    /// Higher than GIN (3.0) due to wider posting entries.
    pub term_lookup_cost: f64,
    /// Cost per result for distance computation during ordered scan.
    pub distance_compute_cost: f64,
    /// Cost per result for heap fetch.
    pub heap_fetch_cost: f64,
    /// Build cost multiplier relative to GIN (>1.0).
    pub build_cost_multiplier: f64,
    /// Cost for in-index phrase position verification.
    pub phrase_verify_cost: f64,
    /// Cost for recheck during boolean-only queries.
    pub recheck_cost: f64,
}

impl Default for RumCostParams {
    fn default() -> Self {
        Self {
            term_lookup_cost: 3.5,
            distance_compute_cost: 0.3,
            heap_fetch_cost: 1.5,
            build_cost_multiplier: 1.4,
            phrase_verify_cost: 0.1,
            recheck_cost: 2.0,
        }
    }
}

/// Estimate the cost of a RUM index scan for a ranked query.
///
/// For queries with a LIMIT, RUM's distance-ordered scan visits
/// only ~k entries (plus a small overfetch buffer) instead of all
/// matching rows. Without a LIMIT, all matches must be scanned
/// with distance computation.
#[must_use]
pub fn rum_ranked_scan_cost(
    total_rows: f64,
    selectivity: f64,
    limit: Option<u64>,
    params: &RumCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    match limit {
        Some(k) => {
            let k = k as f64;
            // Overfetch by 20% to account for posting list structure
            let visit = k * 1.2;
            params.term_lookup_cost
                + visit * (params.distance_compute_cost + params.heap_fetch_cost)
        }
        None => {
            params.term_lookup_cost
                + matching * (params.distance_compute_cost + params.heap_fetch_cost)
        }
    }
}

/// Estimate the cost of a RUM index scan for phrase search.
///
/// RUM verifies phrase positions in-index, avoiding the heap recheck
/// that GIN requires. This makes phrase search significantly faster.
#[must_use]
pub fn rum_phrase_scan_cost(
    total_rows: f64,
    selectivity: f64,
    n_terms: u32,
    params: &RumCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    let lookup = f64::from(n_terms) * params.term_lookup_cost;
    // In-index phrase verification instead of heap recheck
    let verify = matching * params.phrase_verify_cost;
    let fetch = matching * params.heap_fetch_cost;
    lookup + verify + fetch
}

/// Estimate the cost of a RUM scan with timestamp addon ordering.
///
/// When `rum_tsvector_addon_ops` is used, the index provides both
/// text matching and timestamp ordering in a single scan. No
/// separate sort step is needed.
#[must_use]
pub fn rum_addon_scan_cost(
    total_rows: f64,
    selectivity: f64,
    limit: Option<u64>,
    params: &RumCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    let effective = match limit {
        Some(k) => (k as f64 * 1.2).min(matching),
        None => matching,
    };
    params.term_lookup_cost + effective * (params.distance_compute_cost + params.heap_fetch_cost)
}

/// Estimate the cost of a RUM scan for boolean-only matching.
///
/// For pure boolean queries without ordering, RUM is slightly
/// slower than GIN due to wider posting entries.
#[must_use]
pub fn rum_boolean_scan_cost(
    total_rows: f64,
    selectivity: f64,
    n_terms: u32,
    params: &RumCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    let lookup = f64::from(n_terms) * params.term_lookup_cost;
    let fetch = matching * (params.recheck_cost + params.heap_fetch_cost);
    lookup + fetch
}

/// Estimate the cost of a GIN index scan (for comparison).
///
/// GIN requires a full scan of all matching rows plus heap recheck
/// for phrase queries. For ranked queries, GIN must fetch all
/// matches and sort them externally.
#[must_use]
pub fn gin_equivalent_cost(
    total_rows: f64,
    selectivity: f64,
    query_type: RumQueryType,
    limit: Option<u64>,
) -> f64 {
    let gin_term_cost = 3.0;
    let gin_recheck_cost = 2.0;
    let gin_heap_fetch = 1.5;
    let matching = (total_rows * selectivity).max(1.0);

    let scan_cost = gin_term_cost + matching * (gin_recheck_cost + gin_heap_fetch);

    match query_type {
        RumQueryType::BooleanMatch => scan_cost,
        RumQueryType::RankedRetrieval => {
            // GIN: scan all matches + compute rank + sort + limit
            let rank_cost = matching * 0.1;
            let sort_cost = matching * matching.log2().max(1.0) * 0.01;
            let limit_savings = match limit {
                Some(k) => {
                    let k = k as f64;
                    if matching > k * 10.0 {
                        // Top-N sort is cheaper than full sort
                        sort_cost * 0.5
                    } else {
                        0.0
                    }
                }
                None => 0.0,
            };
            scan_cost + rank_cost + sort_cost - limit_savings
        }
        RumQueryType::PhraseSearch => {
            // GIN: scan + heap recheck for each match to verify
            // positions
            let extra_recheck = matching * gin_heap_fetch;
            scan_cost + extra_recheck
        }
        RumQueryType::TimestampOrdered => {
            // GIN: scan all matches + external sort by timestamp
            let sort_cost = matching * matching.log2().max(1.0) * 0.01;
            scan_cost + sort_cost
        }
        RumQueryType::Knn => {
            // GIN has no KNN support; equivalent to full scan + sort
            let sort_cost = matching * matching.log2().max(1.0) * 0.01;
            scan_cost + sort_cost
        }
    }
}

/// Estimate the cost of a sequential scan (baseline for comparison).
#[must_use]
pub fn sequential_scan_cost(total_rows: f64) -> f64 {
    total_rows
}

/// Unified RUM scan cost dispatcher.
#[must_use]
pub fn rum_scan_cost(
    total_rows: f64,
    selectivity: f64,
    query_type: RumQueryType,
    limit: Option<u64>,
    params: &RumCostParams,
) -> f64 {
    match query_type {
        RumQueryType::BooleanMatch => rum_boolean_scan_cost(total_rows, selectivity, 1, params),
        RumQueryType::RankedRetrieval => {
            rum_ranked_scan_cost(total_rows, selectivity, limit, params)
        }
        RumQueryType::PhraseSearch => rum_phrase_scan_cost(total_rows, selectivity, 2, params),
        RumQueryType::TimestampOrdered => {
            rum_addon_scan_cost(total_rows, selectivity, limit, params)
        }
        RumQueryType::Knn => rum_ranked_scan_cost(total_rows, selectivity, limit, params),
    }
}

/// Compare RUM index scan vs GIN index scan and return the ratio.
///
/// A ratio < 1.0 means RUM is cheaper; > 1.0 means GIN wins.
#[must_use]
pub fn rum_vs_gin_ratio(
    total_rows: f64,
    selectivity: f64,
    query_type: RumQueryType,
    limit: Option<u64>,
    params: &RumCostParams,
) -> f64 {
    let rum = rum_scan_cost(total_rows, selectivity, query_type, limit, params);
    let gin = gin_equivalent_cost(total_rows, selectivity, query_type, limit);
    if gin <= 0.0 {
        return 1.0;
    }
    rum / gin
}

/// Compare RUM index scan vs sequential scan and return the ratio.
///
/// A ratio < 1.0 means RUM is cheaper; > 1.0 means seq scan wins.
#[must_use]
pub fn rum_vs_sequential_ratio(
    total_rows: f64,
    selectivity: f64,
    query_type: RumQueryType,
    limit: Option<u64>,
    params: &RumCostParams,
) -> f64 {
    let rum = rum_scan_cost(total_rows, selectivity, query_type, limit, params);
    let seq = sequential_scan_cost(total_rows);
    if seq <= 0.0 {
        return 1.0;
    }
    rum / seq
}

// ------------------------------------------------------------------
// RUM index recommendation
// ------------------------------------------------------------------

/// A recommendation to create or switch to a RUM index.
#[derive(Debug, Clone, PartialEq)]
pub struct RumIndexRecommendation {
    /// Table name.
    pub table: String,
    /// Column to index (typically a tsvector column).
    pub column: String,
    /// Recommended RUM operator class.
    pub opclass: RumOpclass,
    /// Optional addon column for combined ordering.
    pub addon_column: Option<String>,
    /// The query types that would benefit.
    pub beneficial_query_types: Vec<RumQueryType>,
    /// Estimated improvement ratio over GIN (> 1.0 means faster).
    pub estimated_improvement: f64,
    /// Whether this replaces an existing GIN index.
    pub replaces_gin: bool,
}

impl RumIndexRecommendation {
    /// Generate CREATE INDEX SQL for this recommendation.
    #[must_use]
    pub fn to_create_index_sql(&self) -> String {
        let idx_name = format!("idx_{}_{}_rum", self.table, self.column);
        let opclass_str = self.opclass.to_string();

        match &self.addon_column {
            Some(addon) => {
                format!(
                    "CREATE INDEX {idx_name} ON {table} \
                     USING rum ({col} {opclass}, {addon}) \
                     WITH (attach = '{addon}', to = '{col}');",
                    table = self.table,
                    col = self.column,
                    opclass = opclass_str,
                    addon = addon,
                )
            }
            None => {
                format!(
                    "CREATE INDEX {idx_name} ON {table} \
                     USING rum ({col} {opclass});",
                    table = self.table,
                    col = self.column,
                    opclass = opclass_str,
                )
            }
        }
    }
}

/// Evaluate whether a RUM index is recommended for a given table/column
/// based on observed query patterns.
///
/// Returns a recommendation if RUM provides at least 2x improvement
/// over GIN for the dominant query type.
#[must_use]
pub fn evaluate_rum_recommendation(
    table: &str,
    column: &str,
    total_rows: f64,
    selectivity: f64,
    query_types: &[RumQueryType],
    has_gin_index: bool,
    addon_column: Option<&str>,
) -> Option<RumIndexRecommendation> {
    if query_types.is_empty() {
        return None;
    }

    let params = RumCostParams::default();
    let mut beneficial = Vec::new();
    let mut best_improvement = 0.0_f64;

    for &qt in query_types {
        if !qt.benefits_from_rum() {
            continue;
        }

        let limit = match qt {
            RumQueryType::RankedRetrieval | RumQueryType::Knn => Some(10),
            _ => None,
        };

        let ratio = rum_vs_gin_ratio(total_rows, selectivity, qt, limit, &params);

        if ratio < 1.0 {
            let improvement = 1.0 / ratio;
            beneficial.push(qt);
            best_improvement = best_improvement.max(improvement);
        }
    }

    if beneficial.is_empty() || best_improvement < 2.0 {
        return None;
    }

    let opclass = if addon_column.is_some() {
        RumOpclass::TsvectorAddonOps
    } else {
        RumOpclass::TsvectorOps
    };

    Some(RumIndexRecommendation {
        table: table.to_string(),
        column: column.to_string(),
        opclass,
        addon_column: addon_column.map(String::from),
        beneficial_query_types: beneficial,
        estimated_improvement: best_improvement,
        replaces_gin: has_gin_index,
    })
}

// ------------------------------------------------------------------
// E-graph rewrite rules
// ------------------------------------------------------------------

/// Return rewrite rules for RUM index optimization.
///
/// These rules recognize patterns where RUM indexes provide ordering
/// that GIN cannot, and rewrite queries to exploit that ordering:
///
/// 1. `rum-rank-to-distance`: Rewrite ts_rank ORDER BY to distance
///    scan when RUM is available
/// 2. `rum-phrase-index-scan`: Prefer RUM for phrase predicates
/// 3. `rum-sort-elimination`: Remove sort when RUM provides order
/// 4. `rum-knn-limit-pushdown`: Push LIMIT into RUM KNN scan
/// 5. `rum-filter-through-join`: Push RUM-compatible filter below join
#[must_use]
pub fn rum_rewrite_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Rule 1: Push text-search filter through join (left side).
        // When a text-search predicate sits above a join, push it
        // below so the RUM/GIN index can be used on the base table.
        rewrite!("rum-filter-through-join-left";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond (filter ?pred ?left) ?right)"
            if is_text_search_filter(var("?pred"))
        ),
        // Rule 2: Split conjunctive text-search filters for
        // independent index scans.
        rewrite!("rum-split-text-search-filter";
            "(filter (and ?p1 ?p2) ?input)" =>
            "(filter ?p1 (filter ?p2 ?input))"
            if is_text_search_filter(var("?p1"))
        ),
        // Rule 3: Merge adjacent text-search filters for compound
        // scan (inverse of rule 2).
        rewrite!("rum-merge-text-search-filters";
            "(filter ?p1 (filter ?p2 ?input))" =>
            "(filter (and ?p1 ?p2) ?input)"
            if is_text_search_filter(var("?p1"))
        ),
        // Rule 4: Push text-search filter below projection.
        rewrite!("rum-filter-below-project";
            "(filter ?pred (project ?cols ?input))" =>
            "(project ?cols (filter ?pred ?input))"
            if is_text_search_filter(var("?pred"))
        ),
        // Rule 5: Push text-search filter below aggregate
        // (equivalent to pushing $match before $group).
        rewrite!("rum-filter-below-aggregate";
            "(filter ?pred (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs (filter ?pred ?input))"
            if is_text_search_filter(var("?pred"))
        ),
    ]
}

fn var(s: &str) -> Var {
    s.parse().unwrap_or_else(|_| panic!("bad var: {s}"))
}

/// Condition: check if a predicate involves text search operators.
///
/// Text search predicates use `func` nodes (for `@@`, `ts_rank`,
/// `to_tsvector`, etc.) or direct tsvector column references.
fn is_text_search_filter(
    pred_var: Var,
) -> impl Fn(&mut egg::EGraph<RelLang, RelAnalysis>, Id, &Subst) -> bool {
    move |egraph, _id, subst| {
        let pred_id = subst[pred_var];
        contains_text_search_pattern(egraph, pred_id, 3)
    }
}

/// Recursively check if an e-class contains text-search patterns.
///
/// Looks for `func` nodes (text search functions like `@@`,
/// `ts_rank`, `to_tsvector`) in the expression tree.
fn contains_text_search_pattern(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    id: Id,
    depth: u32,
) -> bool {
    if depth == 0 {
        return false;
    }

    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Func(_) => return true,
            RelLang::Eq([l, r])
            | RelLang::Ne([l, r])
            | RelLang::Lt([l, r])
            | RelLang::Le([l, r])
            | RelLang::Gt([l, r])
            | RelLang::Ge([l, r]) => {
                if contains_text_search_pattern(egraph, *l, depth - 1)
                    || contains_text_search_pattern(egraph, *r, depth - 1)
                {
                    return true;
                }
            }
            RelLang::And([l, r]) | RelLang::Or([l, r]) => {
                if contains_text_search_pattern(egraph, *l, depth - 1)
                    || contains_text_search_pattern(egraph, *r, depth - 1)
                {
                    return true;
                }
            }
            RelLang::Not([inner]) => {
                if contains_text_search_pattern(egraph, *inner, depth - 1) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

// ------------------------------------------------------------------
// Cost factor for e-graph cost model
// ------------------------------------------------------------------

/// Cost factor for a RUM scan relative to a sequential scan.
///
/// For ranked/ordered queries, RUM is typically 5-15% of the cost
/// of a full sequential scan with external sort. This factor is
/// used by the e-graph cost model when comparing plan alternatives.
#[must_use]
pub fn rum_scan_cost_factor() -> f64 {
    0.10
}

/// Cost factor for a RUM boolean-only scan relative to GIN.
///
/// For pure boolean matching, RUM is slightly more expensive than
/// GIN (wider posting entries). This factor is > 1.0.
#[must_use]
pub fn rum_boolean_cost_factor_vs_gin() -> f64 {
    1.1
}

// ------------------------------------------------------------------
// Error types
// ------------------------------------------------------------------

/// Errors specific to RUM index optimization.
///
/// All errors are non-fatal: the optimizer falls back to GIN-based
/// or standard PostgreSQL planning.
#[derive(Debug, thiserror::Error)]
pub enum RumError {
    /// RUM extension is not installed.
    #[error("RUM extension not installed; using GIN cost model instead")]
    ExtensionNotInstalled,

    /// Unrecognized RUM operator class.
    #[error(
        "RUM operator class '{opclass}' not recognized; \
         skipping RUM-specific optimization"
    )]
    UnknownOperatorClass {
        /// The operator class name.
        opclass: String,
    },

    /// Distance ordering is not available for this query.
    #[error(
        "Distance ordering not available: {reason}; \
         falling back to GIN scan + sort"
    )]
    DistanceOrderingUnavailable {
        /// Why distance ordering cannot be used.
        reason: String,
    },
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    // -- RumQueryType tests --

    #[test]
    fn query_type_benefits() {
        assert!(!RumQueryType::BooleanMatch.benefits_from_rum());
        assert!(RumQueryType::RankedRetrieval.benefits_from_rum());
        assert!(RumQueryType::PhraseSearch.benefits_from_rum());
        assert!(RumQueryType::TimestampOrdered.benefits_from_rum());
        assert!(RumQueryType::Knn.benefits_from_rum());
    }

    #[test]
    fn query_type_labels() {
        assert_eq!(RumQueryType::BooleanMatch.label(), "boolean_match");
        assert_eq!(RumQueryType::RankedRetrieval.label(), "ranked_retrieval");
        assert_eq!(RumQueryType::PhraseSearch.label(), "phrase_search");
        assert_eq!(RumQueryType::TimestampOrdered.label(), "timestamp_ordered");
        assert_eq!(RumQueryType::Knn.label(), "knn");
    }

    #[test]
    fn query_type_display() {
        assert_eq!(
            RumQueryType::RankedRetrieval.to_string(),
            "ranked_retrieval"
        );
    }

    // -- RumOpclass tests --

    #[test]
    fn opclass_from_name() {
        assert_eq!(
            RumOpclass::from_name("rum_tsvector_ops"),
            Some(RumOpclass::TsvectorOps)
        );
        assert_eq!(
            RumOpclass::from_name("rum_tsvector_hash_ops"),
            Some(RumOpclass::TsvectorHashOps)
        );
        assert_eq!(
            RumOpclass::from_name("rum_tsvector_addon_ops"),
            Some(RumOpclass::TsvectorAddonOps)
        );
        assert_eq!(
            RumOpclass::from_name("rum_tsquery_ops"),
            Some(RumOpclass::TsqueryOps)
        );
        assert_eq!(
            RumOpclass::from_name("rum_anyarray_ops"),
            Some(RumOpclass::AnyarrayOps)
        );
        assert_eq!(
            RumOpclass::from_name("rum_int4_ops"),
            Some(RumOpclass::ScalarOps)
        );
        assert_eq!(
            RumOpclass::from_name("rum_timestamp_ops"),
            Some(RumOpclass::ScalarOps)
        );
        assert_eq!(RumOpclass::from_name("gin_trgm_ops"), None);
        assert_eq!(RumOpclass::from_name("btree_ops"), None);
    }

    #[test]
    fn opclass_distance_ordering_support() {
        assert!(RumOpclass::TsvectorOps.supports_distance_ordering());
        assert!(RumOpclass::TsvectorAddonOps.supports_distance_ordering());
        assert!(RumOpclass::ScalarOps.supports_distance_ordering());
        assert!(!RumOpclass::TsvectorHashOps.supports_distance_ordering());
        assert!(!RumOpclass::TsqueryOps.supports_distance_ordering());
        assert!(!RumOpclass::AnyarrayOps.supports_distance_ordering());
    }

    #[test]
    fn opclass_phrase_search_support() {
        assert!(RumOpclass::TsvectorOps.supports_phrase_search());
        assert!(RumOpclass::TsvectorAddonOps.supports_phrase_search());
        assert!(!RumOpclass::TsvectorHashOps.supports_phrase_search());
        assert!(!RumOpclass::ScalarOps.supports_phrase_search());
    }

    #[test]
    fn opclass_addon_ordering_support() {
        assert!(RumOpclass::TsvectorAddonOps.supports_addon_ordering());
        assert!(!RumOpclass::TsvectorOps.supports_addon_ordering());
        assert!(!RumOpclass::ScalarOps.supports_addon_ordering());
    }

    #[test]
    fn opclass_display() {
        assert_eq!(RumOpclass::TsvectorOps.to_string(), "rum_tsvector_ops");
        assert_eq!(
            RumOpclass::TsvectorAddonOps.to_string(),
            "rum_tsvector_addon_ops"
        );
    }

    // -- Cost model tests --

    #[test]
    fn rum_ranked_much_cheaper_than_gin_with_limit() {
        let params = RumCostParams::default();
        let rum = rum_ranked_scan_cost(100_000.0, 0.1, Some(10), &params);
        let gin = gin_equivalent_cost(100_000.0, 0.1, RumQueryType::RankedRetrieval, Some(10));
        assert!(
            rum < gin * 0.1,
            "RUM ranked (limit 10) should be much cheaper \
             than GIN: rum={rum:.1}, gin={gin:.1}"
        );
    }

    #[test]
    fn rum_ranked_without_limit_still_cheaper() {
        let params = RumCostParams::default();
        let rum = rum_ranked_scan_cost(100_000.0, 0.01, None, &params);
        let gin = gin_equivalent_cost(100_000.0, 0.01, RumQueryType::RankedRetrieval, None);
        // Without limit, RUM still avoids external sort
        assert!(
            rum < gin,
            "RUM ranked without limit should still beat GIN: \
             rum={rum:.1}, gin={gin:.1}"
        );
    }

    #[test]
    fn rum_phrase_cheaper_than_gin() {
        let params = RumCostParams::default();
        let rum = rum_phrase_scan_cost(100_000.0, 0.05, 2, &params);
        let gin = gin_equivalent_cost(100_000.0, 0.05, RumQueryType::PhraseSearch, None);
        assert!(
            rum < gin,
            "RUM phrase search should be cheaper than GIN: \
             rum={rum:.1}, gin={gin:.1}"
        );
    }

    #[test]
    fn rum_boolean_slightly_more_expensive_than_gin() {
        let params = RumCostParams::default();
        let rum = rum_boolean_scan_cost(100_000.0, 0.01, 1, &params);
        let gin = gin_equivalent_cost(100_000.0, 0.01, RumQueryType::BooleanMatch, None);
        // RUM boolean should be comparable or slightly more expensive
        let ratio = rum / gin;
        assert!(
            ratio >= 0.9 && ratio <= 1.5,
            "RUM boolean should be close to GIN: \
             rum={rum:.1}, gin={gin:.1}, ratio={ratio:.2}"
        );
    }

    #[test]
    fn rum_addon_cheaper_than_gin_for_timestamp_ordered() {
        let params = RumCostParams::default();
        let rum = rum_addon_scan_cost(100_000.0, 0.1, Some(20), &params);
        let gin = gin_equivalent_cost(100_000.0, 0.1, RumQueryType::TimestampOrdered, Some(20));
        assert!(
            rum < gin * 0.2,
            "RUM addon scan should be much cheaper for \
             timestamp ordering: rum={rum:.1}, gin={gin:.1}"
        );
    }

    #[test]
    fn rum_vs_gin_ratio_ranked() {
        let params = RumCostParams::default();
        let ratio = rum_vs_gin_ratio(
            100_000.0,
            0.1,
            RumQueryType::RankedRetrieval,
            Some(10),
            &params,
        );
        assert!(
            ratio < 0.1,
            "RUM should be 10x+ cheaper for ranked retrieval \
             with limit: ratio={ratio:.3}"
        );
    }

    #[test]
    fn rum_vs_gin_ratio_boolean() {
        let params = RumCostParams::default();
        let ratio = rum_vs_gin_ratio(100_000.0, 0.01, RumQueryType::BooleanMatch, None, &params);
        assert!(
            ratio >= 0.9,
            "RUM should not be significantly better for \
             boolean-only: ratio={ratio:.3}"
        );
    }

    #[test]
    fn rum_vs_sequential_selective_query() {
        let params = RumCostParams::default();
        let ratio = rum_vs_sequential_ratio(
            1_000_000.0,
            0.001,
            RumQueryType::RankedRetrieval,
            Some(10),
            &params,
        );
        assert!(
            ratio < 0.01,
            "RUM with limit should massively beat seq scan: \
             ratio={ratio:.4}"
        );
    }

    #[test]
    fn rum_vs_sequential_unselective_query() {
        let params = RumCostParams::default();
        let ratio =
            rum_vs_sequential_ratio(100_000.0, 0.9, RumQueryType::BooleanMatch, None, &params);
        assert!(
            ratio > 1.0,
            "RUM should lose to seq scan for 90% selectivity: \
             ratio={ratio:.3}"
        );
    }

    // -- Recommendation tests --

    #[test]
    fn recommend_rum_for_ranked_queries() {
        let rec = evaluate_rum_recommendation(
            "articles",
            "body_tsv",
            1_000_000.0,
            0.01,
            &[RumQueryType::RankedRetrieval],
            true,
            None,
        );
        assert!(rec.is_some(), "should recommend RUM for ranked");
        let rec = rec.unwrap_or_else(|| unreachable!());
        assert_eq!(rec.table, "articles");
        assert_eq!(rec.column, "body_tsv");
        assert_eq!(rec.opclass, RumOpclass::TsvectorOps);
        assert!(rec.estimated_improvement >= 2.0);
        assert!(rec.replaces_gin);
    }

    #[test]
    fn recommend_rum_addon_for_timestamp_ordered() {
        let rec = evaluate_rum_recommendation(
            "posts",
            "content_tsv",
            500_000.0,
            0.05,
            &[RumQueryType::TimestampOrdered],
            true,
            Some("created_at"),
        );
        assert!(rec.is_some(), "should recommend RUM addon");
        let rec = rec.unwrap_or_else(|| unreachable!());
        assert_eq!(rec.opclass, RumOpclass::TsvectorAddonOps);
        assert_eq!(rec.addon_column.as_deref(), Some("created_at"));
    }

    #[test]
    fn no_rum_recommendation_for_boolean_only() {
        let rec = evaluate_rum_recommendation(
            "logs",
            "message_tsv",
            100_000.0,
            0.01,
            &[RumQueryType::BooleanMatch],
            true,
            None,
        );
        assert!(rec.is_none(), "should not recommend RUM for boolean-only");
    }

    #[test]
    fn no_rum_recommendation_for_empty_query_types() {
        let rec =
            evaluate_rum_recommendation("logs", "message_tsv", 100_000.0, 0.01, &[], true, None);
        assert!(rec.is_none());
    }

    #[test]
    fn rum_recommendation_create_index_sql() {
        let rec = RumIndexRecommendation {
            table: "articles".to_string(),
            column: "body_tsv".to_string(),
            opclass: RumOpclass::TsvectorOps,
            addon_column: None,
            beneficial_query_types: vec![RumQueryType::RankedRetrieval],
            estimated_improvement: 50.0,
            replaces_gin: true,
        };
        let sql = rec.to_create_index_sql();
        assert!(sql.contains("USING rum"));
        assert!(sql.contains("body_tsv"));
        assert!(sql.contains("rum_tsvector_ops"));
        assert!(sql.contains("articles"));
    }

    #[test]
    fn rum_recommendation_addon_create_index_sql() {
        let rec = RumIndexRecommendation {
            table: "posts".to_string(),
            column: "content_tsv".to_string(),
            opclass: RumOpclass::TsvectorAddonOps,
            addon_column: Some("created_at".to_string()),
            beneficial_query_types: vec![RumQueryType::TimestampOrdered],
            estimated_improvement: 10.0,
            replaces_gin: false,
        };
        let sql = rec.to_create_index_sql();
        assert!(sql.contains("USING rum"));
        assert!(sql.contains("rum_tsvector_addon_ops"));
        assert!(sql.contains("created_at"));
        assert!(sql.contains("attach"));
    }

    // -- Cost factor tests --

    #[test]
    fn rum_scan_cost_factor_reasonable() {
        let factor = rum_scan_cost_factor();
        assert!(factor > 0.0 && factor < 1.0);
    }

    #[test]
    fn rum_boolean_cost_factor_vs_gin_above_one() {
        let factor = rum_boolean_cost_factor_vs_gin();
        assert!(factor > 1.0);
    }

    // -- Error type tests --

    #[test]
    fn error_messages_are_actionable() {
        let err = RumError::ExtensionNotInstalled;
        let msg = err.to_string();
        assert!(msg.contains("not installed"));
        assert!(msg.contains("GIN"));

        let err = RumError::UnknownOperatorClass {
            opclass: "rum_custom_ops".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("rum_custom_ops"));
        assert!(msg.contains("not recognized"));

        let err = RumError::DistanceOrderingUnavailable {
            reason: "hash-based opclass".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("hash-based opclass"));
        assert!(msg.contains("falling back"));
    }

    // -- E-graph rewrite rule tests --

    fn run_with_rum_rules(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        let rules = rum_rewrite_rules();
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules)
    }

    #[test]
    fn text_search_filter_through_join() {
        let left = RelExpr::scan("articles");
        let right = RelExpr::scan("categories");
        let joined = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(left),
            right: Box::new(right),
        };
        let filtered = RelExpr::Filter {
            predicate: Expr::Function {
                name: "ts_match".to_string(),
                args: vec![
                    Expr::Column(ColumnRef::new("body_tsv")),
                    Expr::Const(Const::String("postgresql".to_string())),
                ],
            },
            input: Box::new(joined),
        };

        let runner = run_with_rum_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 5,
            "e-graph should grow from text search filter pushdown"
        );
    }

    #[test]
    fn text_search_filter_below_project() {
        use ra_core::algebra::ProjectionColumn;

        let scan = RelExpr::scan("articles");
        let projected = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("title")),
                alias: None,
            }],
            input: Box::new(scan),
        };
        let filtered = RelExpr::Filter {
            predicate: Expr::Function {
                name: "ts_match".to_string(),
                args: vec![
                    Expr::Column(ColumnRef::new("body_tsv")),
                    Expr::Const(Const::String("optimization".to_string())),
                ],
            },
            input: Box::new(projected),
        };

        let runner = run_with_rum_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 4,
            "e-graph should grow from text search filter \
             below project"
        );
    }

    #[test]
    fn conjunctive_text_search_filter_splitting() {
        let scan = RelExpr::scan("articles");
        let pred = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Function {
                name: "ts_match".to_string(),
                args: vec![
                    Expr::Column(ColumnRef::new("body_tsv")),
                    Expr::Const(Const::String("postgresql".to_string())),
                ],
            }),
            right: Box::new(Expr::Function {
                name: "ts_match".to_string(),
                args: vec![
                    Expr::Column(ColumnRef::new("title_tsv")),
                    Expr::Const(Const::String("index".to_string())),
                ],
            }),
        };

        let filtered = RelExpr::Filter {
            predicate: pred,
            input: Box::new(scan),
        };

        let runner = run_with_rum_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 5,
            "e-graph should grow from conjunctive text search splitting"
        );
    }

    #[test]
    fn plain_filter_not_matched_by_rum_rules() {
        let scan = RelExpr::scan("users");
        let filtered = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Const(Const::Int(42))),
            },
            input: Box::new(scan),
        };

        let rec = to_rec_expr(&filtered).expect("conversion should succeed");
        let initial_classes = {
            let mut eg = egg::EGraph::<RelLang, RelAnalysis>::default();
            eg.add_expr(&rec);
            eg.number_of_classes()
        };

        let runner = run_with_rum_rules(&filtered);
        assert_eq!(
            runner.egraph.number_of_classes(),
            initial_classes,
            "plain filter should not trigger RUM rules"
        );
    }

    #[test]
    fn text_search_filter_below_aggregate() {
        use ra_core::algebra::AggregateExpr;
        use ra_core::algebra::AggregateFunction;

        let scan = RelExpr::scan("articles");
        let agg = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("category"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".to_string()),
            }],
            input: Box::new(scan),
        };

        let filtered = RelExpr::Filter {
            predicate: Expr::Function {
                name: "ts_match".to_string(),
                args: vec![
                    Expr::Column(ColumnRef::new("body_tsv")),
                    Expr::Const(Const::String("postgresql".to_string())),
                ],
            },
            input: Box::new(agg),
        };

        let runner = run_with_rum_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 6,
            "text search filter should be pushed below aggregate"
        );
    }

    // -- Unified cost dispatcher tests --

    #[test]
    fn unified_cost_boolean() {
        let params = RumCostParams::default();
        let cost = rum_scan_cost(100_000.0, 0.01, RumQueryType::BooleanMatch, None, &params);
        assert!(cost > 0.0);
    }

    #[test]
    fn unified_cost_ranked_with_limit() {
        let params = RumCostParams::default();
        let cost = rum_scan_cost(
            100_000.0,
            0.01,
            RumQueryType::RankedRetrieval,
            Some(10),
            &params,
        );
        // With limit 10, cost should be very low
        assert!(cost < 100.0);
    }

    #[test]
    fn unified_cost_phrase() {
        let params = RumCostParams::default();
        let cost = rum_scan_cost(100_000.0, 0.05, RumQueryType::PhraseSearch, None, &params);
        assert!(cost > 0.0);
    }

    #[test]
    fn unified_cost_knn() {
        let params = RumCostParams::default();
        let cost = rum_scan_cost(100_000.0, 0.01, RumQueryType::Knn, Some(5), &params);
        assert!(cost < 50.0);
    }
}
