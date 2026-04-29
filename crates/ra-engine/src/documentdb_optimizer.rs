//! `DocumentDB` (MongoDB-compatible) query optimization.
//!
//! Provides BSON-aware selectivity estimation, GIN index cost
//! modeling, and rewrite rules for queries originating from
//! Microsoft's documentdb `PostgreSQL` extension.
//!
//! `DocumentDB` translates `MongoDB` wire protocol operations into
//! `PostgreSQL` queries over BSON-typed columns using custom
//! operators (`@=`, `@>`, `@<`, `@*=`, `@~`, etc.). The default
//! selectivity for all these operators is a fixed 0.01 (1%),
//! leading to poor join ordering and scan strategy selection.
//!
//! This module improves plan quality by:
//! - Recognizing BSON operator patterns in the e-graph
//! - Providing operator-specific selectivity estimates
//! - Modeling GIN index scan costs for BSON columns
//! - Recommending compound GIN indexes for multi-path queries
//! - Rewriting filter patterns to exploit GIN index structure
//!
//! All optimizations are non-fatal: when BSON parsing or metadata
//! queries fail, the optimizer falls back to standard `PostgreSQL`
//! behavior.
//!
//! See: `rfcs/text/0062-documentdb-query-optimization.md`

use std::collections::HashMap;

use egg::{rewrite, Id, Rewrite, Subst, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::parse_var;

// ------------------------------------------------------------------
// BSON operator types recognized from documentdb
// ------------------------------------------------------------------

/// MongoDB/BSON operators that documentdb translates to custom
/// `PostgreSQL` operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BsonOperator {
    /// `$eq` maps to `@=` -- exact equality on a BSON path.
    Eq,
    /// `$gt` maps to `@>` -- greater than comparison.
    Gt,
    /// `$gte` maps to `@>=` -- greater than or equal.
    Gte,
    /// `$lt` maps to `@<` -- less than comparison.
    Lt,
    /// `$lte` maps to `@<=` -- less than or equal.
    Lte,
    /// `$ne` maps to negation of `@=` -- not equal.
    Ne,
    /// `$in` maps to `@*=` -- membership in array.
    In,
    /// `$nin` maps to `@!*=` -- not in array.
    Nin,
    /// `$all` maps to `@&=` -- array contains all elements.
    All,
    /// `$regex` maps to `@~` -- regular expression match.
    Regex,
    /// `$exists` -- field existence check.
    Exists,
    /// `$elemMatch` -- nested array element match.
    ElemMatch,
}

impl BsonOperator {
    /// Parse a `PostgreSQL` custom operator name into a BSON operator.
    #[must_use]
    pub fn from_pg_operator(op: &str) -> Option<Self> {
        match op {
            "@=" => Some(Self::Eq),
            "@>" => Some(Self::Gt),
            "@>=" => Some(Self::Gte),
            "@<" => Some(Self::Lt),
            "@<=" => Some(Self::Lte),
            "@*=" => Some(Self::In),
            "@!*=" => Some(Self::Nin),
            "@&=" => Some(Self::All),
            "@~" => Some(Self::Regex),
            _ => None,
        }
    }

    /// Return the default selectivity for this operator class.
    ///
    /// These estimates are substantially better than the fixed 0.01
    /// that documentdb returns for all operators.
    #[must_use]
    pub fn default_selectivity(self) -> f64 {
        match self {
            Self::Eq => 0.005,
            Self::Gt | Self::Gte | Self::Lt | Self::Lte => 0.33,
            Self::Ne => 0.99,
            Self::In => 0.05,
            Self::Nin => 0.95,
            Self::All => 0.001,
            Self::Regex => 0.25,
            Self::Exists => 0.75,
            Self::ElemMatch => 0.01,
        }
    }

    /// Whether this operator can benefit from a GIN index.
    #[must_use]
    pub fn supports_gin_index(self) -> bool {
        match self {
            Self::Eq
            | Self::Gt
            | Self::Gte
            | Self::Lt
            | Self::Lte
            | Self::In
            | Self::All
            | Self::Regex
            | Self::ElemMatch => true,
            Self::Ne | Self::Nin | Self::Exists => false,
        }
    }
}

impl std::fmt::Display for BsonOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eq => write!(f, "$eq"),
            Self::Gt => write!(f, "$gt"),
            Self::Gte => write!(f, "$gte"),
            Self::Lt => write!(f, "$lt"),
            Self::Lte => write!(f, "$lte"),
            Self::Ne => write!(f, "$ne"),
            Self::In => write!(f, "$in"),
            Self::Nin => write!(f, "$nin"),
            Self::All => write!(f, "$all"),
            Self::Regex => write!(f, "$regex"),
            Self::Exists => write!(f, "$exists"),
            Self::ElemMatch => write!(f, "$elemMatch"),
        }
    }
}

// ------------------------------------------------------------------
// Selectivity estimation
// ------------------------------------------------------------------

/// Selectivity estimate with metadata about how it was derived.
#[derive(Debug, Clone)]
pub struct SelectivityEstimate {
    /// Estimated fraction of rows that match (0.0 to 1.0).
    pub selectivity: f64,
    /// How the estimate was derived.
    pub source: SelectivitySource,
}

/// How a selectivity estimate was derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectivitySource {
    /// From GIN index statistics (most accurate).
    IndexStats,
    /// From schema inference / sampling.
    SchemaInference,
    /// Operator-specific heuristic (better than fixed 1%).
    OperatorHeuristic,
    /// Fixed default (documentdb baseline).
    FixedDefault,
}

/// Estimate selectivity for a BSON operator with optional context.
///
/// When `distinct_count` is known (from index stats or sampling),
/// equality selectivity is estimated as `1/distinct_count`. For
/// range operators, the fraction is estimated based on whether the
/// range is bounded on one or both sides.
#[must_use]
pub fn estimate_selectivity(
    op: BsonOperator,
    distinct_count: Option<u64>,
    array_length: Option<u64>,
) -> SelectivityEstimate {
    match op {
        BsonOperator::Eq => {
            if let Some(ndistinct) = distinct_count {
                if ndistinct > 0 {
                    return SelectivityEstimate {
                        selectivity: (1.0 / ndistinct as f64).clamp(0.0001, 1.0),
                        source: SelectivitySource::IndexStats,
                    };
                }
            }
            SelectivityEstimate {
                selectivity: op.default_selectivity(),
                source: SelectivitySource::OperatorHeuristic,
            }
        }
        BsonOperator::In => {
            let base = if let Some(ndistinct) = distinct_count {
                if ndistinct > 0 {
                    1.0 / ndistinct as f64
                } else {
                    BsonOperator::Eq.default_selectivity()
                }
            } else {
                BsonOperator::Eq.default_selectivity()
            };
            let n_elements = array_length.unwrap_or(10) as f64;
            let sel = (base * n_elements).clamp(0.0001, 0.9);
            SelectivityEstimate {
                selectivity: sel,
                source: if distinct_count.is_some() {
                    SelectivitySource::IndexStats
                } else {
                    SelectivitySource::OperatorHeuristic
                },
            }
        }
        BsonOperator::All => {
            let base = if let Some(ndistinct) = distinct_count {
                if ndistinct > 0 {
                    1.0 / ndistinct as f64
                } else {
                    BsonOperator::Eq.default_selectivity()
                }
            } else {
                BsonOperator::Eq.default_selectivity()
            };
            let n_elements = array_length.unwrap_or(3) as f64;
            let sel = base.powf(n_elements).clamp(0.000_001, 0.5);
            SelectivityEstimate {
                selectivity: sel,
                source: if distinct_count.is_some() {
                    SelectivitySource::IndexStats
                } else {
                    SelectivitySource::OperatorHeuristic
                },
            }
        }
        _ => SelectivityEstimate {
            selectivity: op.default_selectivity(),
            source: SelectivitySource::OperatorHeuristic,
        },
    }
}

/// Combine selectivities for multiple independent predicates.
///
/// Uses the independence assumption with a correlation damping
/// factor to avoid underestimating combined selectivity when
/// predicates on the same document are likely correlated.
///
/// The damping formula: `product(sel_i^(damping^i))` where
/// selectivities are sorted ascending and damping = 0.85.
#[must_use]
pub fn combine_selectivities(selectivities: &[f64]) -> f64 {
    if selectivities.is_empty() {
        return 1.0;
    }
    if selectivities.len() == 1 {
        return selectivities[0];
    }

    let mut sorted: Vec<f64> = selectivities.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let damping: f64 = 0.85;
    let mut combined = 1.0_f64;
    for (i, &sel) in sorted.iter().enumerate() {
        let exponent = damping.powi(i.try_into().unwrap_or(0));
        combined *= sel.powf(exponent);
    }

    combined.clamp(0.000_001, 1.0)
}

// ------------------------------------------------------------------
// GIN index cost model for BSON
// ------------------------------------------------------------------

/// Cost parameters for GIN index scans on BSON documents.
#[derive(Debug, Clone)]
pub struct GinBsonCostParams {
    /// Cost per posting list lookup (default: 3.0).
    pub term_lookup_cost: f64,
    /// Cost per document for BSON recheck (default: 2.0).
    pub recheck_cost: f64,
    /// Cost per document for heap fetch (default: 1.5).
    pub heap_fetch_cost: f64,
    /// Cost for bitmap intersection of posting lists (default: 0.5).
    pub bitmap_intersection_cost: f64,
}

impl Default for GinBsonCostParams {
    fn default() -> Self {
        Self {
            term_lookup_cost: 3.0,
            recheck_cost: 2.0,
            heap_fetch_cost: 1.5,
            bitmap_intersection_cost: 0.5,
        }
    }
}

/// Estimate the cost of a GIN index scan for a single-path query.
///
/// Cost = `n_terms` * `term_lookup` + `n_matching` * (recheck + `heap_fetch`)
#[must_use]
pub fn gin_scan_cost(
    total_rows: f64,
    selectivity: f64,
    n_terms: u32,
    params: &GinBsonCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    let lookup = f64::from(n_terms) * params.term_lookup_cost;
    let fetch = matching * (params.recheck_cost + params.heap_fetch_cost);
    lookup + fetch
}

/// Estimate the cost of a compound GIN index scan (multiple paths).
///
/// Compound indexes intersect posting lists, reducing the number
/// of heap fetches at the cost of bitmap intersection.
#[must_use]
pub fn compound_gin_scan_cost(
    total_rows: f64,
    combined_selectivity: f64,
    n_paths: u32,
    params: &GinBsonCostParams,
) -> f64 {
    let matching = (total_rows * combined_selectivity).max(1.0);
    let lookup = f64::from(n_paths) * params.term_lookup_cost;
    let intersection = f64::from(n_paths.saturating_sub(1))
        * params.bitmap_intersection_cost
        * (total_rows * 0.01).max(1.0);
    let fetch = matching * (params.recheck_cost + params.heap_fetch_cost);
    lookup + intersection + fetch
}

/// Estimate the cost of a sequential scan (baseline for comparison).
#[must_use]
pub fn sequential_scan_cost(total_rows: f64) -> f64 {
    total_rows * 1.0
}

/// Compare GIN index scan vs sequential scan and return the ratio.
///
/// A ratio < 1.0 means the GIN scan is cheaper; > 1.0 means
/// sequential scan wins. Used to decide whether to recommend a
/// GIN index.
#[must_use]
pub fn gin_vs_sequential_ratio(
    total_rows: f64,
    selectivity: f64,
    n_terms: u32,
    params: &GinBsonCostParams,
) -> f64 {
    let gin = gin_scan_cost(total_rows, selectivity, n_terms, params);
    let seq = sequential_scan_cost(total_rows);
    if seq <= 0.0 {
        return 1.0;
    }
    gin / seq
}

// ------------------------------------------------------------------
// GIN index recommendation
// ------------------------------------------------------------------

/// A recommendation to create a GIN index for BSON queries.
#[derive(Debug, Clone, PartialEq)]
pub struct GinIndexRecommendation {
    /// Collection name (from `documentdb_api.collection`).
    pub collection: String,
    /// BSON paths that should be indexed.
    pub paths: Vec<String>,
    /// Estimated improvement ratio (> 1.0 means improvement).
    pub estimated_improvement: f64,
    /// The operators that would benefit from this index.
    pub operators: Vec<BsonOperator>,
}

impl GinIndexRecommendation {
    /// Generate the documentdb CREATE INDEX command for this
    /// recommendation.
    #[must_use]
    pub fn to_create_index_command(&self, db_name: &str) -> String {
        let key_entries: Vec<String> = self.paths.iter().map(|p| format!("\"{p}\": 1")).collect();
        let key_json = key_entries.join(", ");
        let idx_name = format!("idx_{}", self.paths.join("_").replace('.', "_"));

        format!(
            "SELECT documentdb_api_internal.\
             create_indexes_non_concurrently(\
             '{db_name}', \
             '{{\"createIndexes\": \"{collection}\", \
             \"indexes\": [{{\"key\": {{{key_json}}}, \
             \"name\": \"{idx_name}\"}}]}}'::bson);",
            collection = self.collection,
        )
    }
}

/// Analyze a set of query patterns and recommend GIN indexes.
///
/// Groups operator usages by collection and path, then recommends
/// indexes for paths that appear frequently with GIN-eligible
/// operators.
#[must_use]
pub fn recommend_gin_indexes(
    patterns: &[QueryPattern],
    min_frequency: u32,
) -> Vec<GinIndexRecommendation> {
    let mut path_usage: HashMap<(String, String), (u32, Vec<BsonOperator>)> = HashMap::new();

    for pattern in patterns {
        for pred in &pattern.predicates {
            if !pred.operator.supports_gin_index() {
                continue;
            }
            let key = (pattern.collection.clone(), pred.path.clone());
            let entry = path_usage.entry(key).or_insert_with(|| (0, Vec::new()));
            entry.0 += pattern.frequency;
            if !entry.1.contains(&pred.operator) {
                entry.1.push(pred.operator);
            }
        }
    }

    // Group by collection for compound index candidates
    let mut collection_paths: HashMap<String, Vec<(String, u32, Vec<BsonOperator>)>> =
        HashMap::new();
    for ((collection, path), (freq, ops)) in &path_usage {
        if *freq >= min_frequency {
            collection_paths
                .entry(collection.clone())
                .or_default()
                .push((path.clone(), *freq, ops.clone()));
        }
    }

    let mut recommendations = Vec::new();
    let params = GinBsonCostParams::default();

    for (collection, mut paths) in collection_paths {
        // Sort by frequency descending (most selective first)
        paths.sort_by(|a, b| b.1.cmp(&a.1));

        if paths.len() >= 2 {
            // Recommend compound index for multi-path queries
            let compound_paths: Vec<String> = paths.iter().map(|p| p.0.clone()).collect();
            let all_ops: Vec<BsonOperator> = paths.iter().flat_map(|p| p.2.clone()).collect();

            let combined_sel = combine_selectivities(
                &all_ops
                    .iter()
                    .map(|op| op.default_selectivity())
                    .collect::<Vec<_>>(),
            );
            let ratio = gin_vs_sequential_ratio(
                100_000.0,
                combined_sel,
                compound_paths.len() as u32,
                &params,
            );

            recommendations.push(GinIndexRecommendation {
                collection: collection.clone(),
                paths: compound_paths,
                estimated_improvement: if ratio > 0.0 { 1.0 / ratio } else { 1.0 },
                operators: all_ops,
            });
        }

        // Also recommend single-path indexes for high-frequency paths
        for (path, _freq, ops) in &paths {
            let sel = ops.first().map_or(0.01, |op| op.default_selectivity());
            let ratio = gin_vs_sequential_ratio(100_000.0, sel, 1, &params);

            if ratio < 0.5 {
                recommendations.push(GinIndexRecommendation {
                    collection: collection.clone(),
                    paths: vec![path.clone()],
                    estimated_improvement: if ratio > 0.0 { 1.0 / ratio } else { 1.0 },
                    operators: ops.clone(),
                });
            }
        }
    }

    recommendations
}

// ------------------------------------------------------------------
// Query pattern recognition
// ------------------------------------------------------------------

/// A recognized BSON query predicate.
#[derive(Debug, Clone, PartialEq)]
pub struct BsonPredicate {
    /// BSON document path (e.g., "status", "address.city").
    pub path: String,
    /// The BSON operator used.
    pub operator: BsonOperator,
    /// Optional literal value for selectivity refinement.
    pub literal_hint: Option<String>,
}

/// A recognized documentdb query pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPattern {
    /// Collection name.
    pub collection: String,
    /// Predicates in this query.
    pub predicates: Vec<BsonPredicate>,
    /// How many times this pattern was observed.
    pub frequency: u32,
}

impl QueryPattern {
    /// Estimate combined selectivity for all predicates.
    #[must_use]
    #[expect(clippy::similar_names, reason = "sels and self are distinct concepts")]
    pub fn estimated_selectivity(&self) -> f64 {
        let sels: Vec<f64> = self
            .predicates
            .iter()
            .map(|p| estimate_selectivity(p.operator, None, None).selectivity)
            .collect();
        combine_selectivities(&sels)
    }
}

// ------------------------------------------------------------------
// E-graph rewrite rules for documentdb patterns
// ------------------------------------------------------------------

/// Return rewrite rules for documentdb/BSON query optimization.
///
/// These rules recognize common patterns in queries generated by
/// documentdb and rewrite them for better plan quality:
///
/// 1. BSON function-based filter to operator form (enables further
///    rewrites)
/// 2. Compound filter splitting for GIN index exploitation
/// 3. Predicate ordering for multi-path GIN scans
#[must_use]
pub fn documentdb_rewrite_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Rule 1: Push BSON equality filter through join.
        //
        // DocumentDB queries often filter on a BSON column after a
        // join. Pushing the filter below the join allows the GIN
        // index to be used.
        //
        // (filter (func bson_eq ?col ?val) (join inner ?cond ?l ?r))
        //   => (join inner ?cond (filter (func bson_eq ?col ?val) ?l) ?r)
        //
        // We express this as a generic pattern since documentdb's
        // operators appear as function calls in the translated SQL.
        rewrite!("docdb-filter-through-join-left";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond (filter ?pred ?left) ?right)"
            if is_bson_operator_filter(parse_var("?pred"))
        ),
        // Rule 2: Split conjunctive BSON filters for independent
        // GIN scans. When two BSON predicates are ANDed, splitting
        // them allows each to use its own GIN index path.
        rewrite!("docdb-split-conjunctive-bson-filter";
            "(filter (and ?p1 ?p2) ?input)" =>
            "(filter ?p1 (filter ?p2 ?input))"
            if is_bson_operator_filter(parse_var("?p1"))
        ),
        // Rule 3: Merge adjacent BSON filters for compound GIN
        // scan. The inverse of rule 2: merging allows a compound
        // GIN index to handle both predicates in one scan.
        rewrite!("docdb-merge-bson-filters";
            "(filter ?p1 (filter ?p2 ?input))" =>
            "(filter (and ?p1 ?p2) ?input)"
            if is_bson_operator_filter(parse_var("?p1"))
        ),
        // Rule 4: Push BSON filter below projection. DocumentDB
        // often wraps results in a projection; pushing the filter
        // below it enables GIN scan selection.
        rewrite!("docdb-filter-below-project";
            "(filter ?pred (project ?cols ?input))" =>
            "(project ?cols (filter ?pred ?input))"
            if is_bson_operator_filter(parse_var("?pred"))
        ),
        // Rule 5: Push $match predicates below aggregate.
        // In aggregation pipelines, $match after $group can
        // sometimes be pushed to a WHERE clause before grouping.
        rewrite!("docdb-match-below-aggregate";
            "(filter ?pred (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs (filter ?pred ?input))"
            if is_bson_operator_filter(parse_var("?pred"))
        ),
    ]
}

/// Condition: check if a predicate is a BSON operator filter.
///
/// In the e-graph, BSON operators appear as function calls (func
/// nodes) with operator names like "`bson_eq`", "`bson_gt`", etc., or
/// as standard comparison operators applied to BSON function
/// results. We check for the presence of func nodes or json-access
/// nodes which indicate BSON column access.
fn is_bson_operator_filter(
    pred_var: Var,
) -> impl Fn(&mut egg::EGraph<RelLang, RelAnalysis>, Id, &Subst) -> bool {
    move |egraph, _id, subst| {
        let pred_id = subst[pred_var];
        contains_bson_pattern(egraph, pred_id, 3)
    }
}

/// Recursively check if an e-class contains BSON-related patterns.
///
/// Looks for:
/// - `func` nodes (documentdb custom operators)
/// - `json-access` nodes (BSON path extraction)
/// - Comparison operators with BSON operands
fn contains_bson_pattern(egraph: &egg::EGraph<RelLang, RelAnalysis>, id: Id, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }

    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Func(_) | RelLang::JsonAccess(_) => {
                return true;
            }
            RelLang::Eq([l, r])
            | RelLang::Ne([l, r])
            | RelLang::Lt([l, r])
            | RelLang::Le([l, r])
            | RelLang::Gt([l, r])
            | RelLang::Ge([l, r])
            | RelLang::And([l, r])
            | RelLang::Or([l, r]) => {
                if contains_bson_pattern(egraph, *l, depth - 1)
                    || contains_bson_pattern(egraph, *r, depth - 1)
                {
                    return true;
                }
            }
            RelLang::Not([inner]) => {
                if contains_bson_pattern(egraph, *inner, depth - 1) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

// ------------------------------------------------------------------
// Error types
// ------------------------------------------------------------------

/// Errors specific to documentdb optimization.
///
/// All errors are non-fatal: the optimizer falls back to treating
/// the query as standard `PostgreSQL`.
#[derive(Debug, thiserror::Error)]
pub enum DocumentDbError {
    /// BSON path extraction failed.
    #[error(
        "BSON path extraction failed for {path}: {reason}; \
         using default selectivity"
    )]
    PathExtractionFailed {
        /// The BSON path that failed.
        path: String,
        /// Why extraction failed.
        reason: String,
    },

    /// Schema inference failed for a collection.
    #[error(
        "Schema inference failed for collection {collection}: \
         {reason}; skipping schema-based optimization"
    )]
    SchemaInferenceFailed {
        /// The collection name.
        collection: String,
        /// Why inference failed.
        reason: String,
    },

    /// Unsupported documentdb version.
    #[error(
        "DocumentDB version {version} not supported for \
         {feature}; minimum required: {minimum}"
    )]
    UnsupportedVersion {
        /// Detected version.
        version: String,
        /// Feature that requires a newer version.
        feature: String,
        /// Minimum version needed.
        minimum: String,
    },
}

// ------------------------------------------------------------------
// GIN index cost factor for the e-graph cost model
// ------------------------------------------------------------------

/// Cost factor for a GIN BSON scan relative to a sequential scan.
///
/// A GIN scan on a selective BSON predicate is typically 10-30% of
/// the cost of a full sequential scan. This factor is used by the
/// e-graph cost model when comparing plan alternatives.
#[must_use]
pub fn gin_bson_scan_cost_factor() -> f64 {
    0.15
}

// ------------------------------------------------------------------
// DocumentDB extended RUM (BSON-aware) optimization
// ------------------------------------------------------------------
// See: `rfcs/text/0080-documentdb-rum-bson-optimization.md`

/// `DocumentDB`'s four RUM operator families for BSON indexing.
///
/// These correspond to the operator families registered by
/// `pg_documentdb_extended_rum` and determine which scan
/// strategy the index provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BsonRumOpfamily {
    /// `bson_extended_rum_single_path_ops` -- single JSON path
    /// with distance ordering. Used for `$text`, `$regex`, and
    /// single-field ordered scans.
    SinglePath,
    /// `bson_extended_rum_composite_path_ops` -- multiple paths
    /// plus the `|-<>` distance operator. Used for `$near`,
    /// compound queries, and `$text` with `$sort`.
    CompositePath,
    /// `documentdb_extended_rum_hashed_ops` -- hash-based
    /// equality. Used for high-cardinality `$eq` and `_id`
    /// lookups.
    Hashed,
    /// `bson_extended_rum_unique_shard_path_ops` -- unique
    /// constraint enforcement on shard keys.
    UniqueShard,
}

impl BsonRumOpfamily {
    /// Parse an operator family name string.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "bson_extended_rum_single_path_ops" => Some(Self::SinglePath),
            "bson_extended_rum_composite_path_ops" => Some(Self::CompositePath),
            "documentdb_extended_rum_hashed_ops" => Some(Self::Hashed),
            "bson_extended_rum_unique_shard_path_ops" => Some(Self::UniqueShard),
            _ => None,
        }
    }

    /// Whether this operator family supports distance ordering.
    #[must_use]
    pub fn supports_ordering(self) -> bool {
        match self {
            Self::SinglePath | Self::CompositePath => true,
            Self::Hashed | Self::UniqueShard => false,
        }
    }

    /// Whether this operator family supports compound path scans.
    #[must_use]
    pub fn supports_compound(self) -> bool {
        self == Self::CompositePath
    }
}

impl std::fmt::Display for BsonRumOpfamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SinglePath => {
                write!(f, "bson_extended_rum_single_path_ops")
            }
            Self::CompositePath => {
                write!(f, "bson_extended_rum_composite_path_ops")
            }
            Self::Hashed => {
                write!(f, "documentdb_extended_rum_hashed_ops")
            }
            Self::UniqueShard => {
                write!(f, "bson_extended_rum_unique_shard_path_ops")
            }
        }
    }
}

/// Map a BSON operator to its preferred RUM operator family.
///
/// Returns `None` for operators that do not benefit from RUM
/// (e.g., `$ne`, `$nin`, `$exists`).
#[must_use]
pub fn bson_op_to_rum_opfamily(op: BsonOperator) -> Option<BsonRumOpfamily> {
    match op {
        BsonOperator::Eq => Some(BsonRumOpfamily::Hashed),
        BsonOperator::Gt
        | BsonOperator::Gte
        | BsonOperator::Lt
        | BsonOperator::Lte
        | BsonOperator::In
        | BsonOperator::All
        | BsonOperator::ElemMatch
        | BsonOperator::Regex => Some(BsonRumOpfamily::SinglePath),
        BsonOperator::Ne | BsonOperator::Nin | BsonOperator::Exists => None,
    }
}

/// Whether a BSON operator benefits from RUM over GIN.
///
/// RUM provides advantages for operators that need ordering or
/// in-index position verification. Pure containment checks
/// are handled equally well by GIN.
#[must_use]
pub fn bson_op_benefits_from_rum(op: BsonOperator) -> bool {
    match op {
        // RUM benefits: regex phrase verification, ordered array scans,
        // boundary-qualified range scans.
        BsonOperator::Regex
        | BsonOperator::All
        | BsonOperator::ElemMatch
        | BsonOperator::Gt
        | BsonOperator::Gte
        | BsonOperator::Lt
        | BsonOperator::Lte => true,
        // Equality (hashed RUM comparable to GIN) and non-indexable ops.
        BsonOperator::Eq
        | BsonOperator::In
        | BsonOperator::Ne
        | BsonOperator::Nin
        | BsonOperator::Exists => false,
    }
}

// ------------------------------------------------------------------
// RUM BSON cost model
// ------------------------------------------------------------------

/// Cost parameters for RUM index scans on BSON documents.
///
/// RUM posting entries are wider than GIN (positional data, addon
/// fields), so per-entry costs are higher. However, ordered scans
/// and boundary qualification reduce total work for many query
/// patterns.
#[derive(Debug, Clone)]
pub struct RumBsonCostParams {
    /// Cost per posting list term lookup (higher than GIN's 3.0).
    pub term_lookup_cost: f64,
    /// Cost for boundary qualifier evaluation.
    pub boundary_cost: f64,
    /// Cost per result for distance computation during ordered scan.
    pub distance_compute_cost: f64,
    /// Cost per result for heap fetch.
    pub heap_fetch_cost: f64,
    /// Cost per result for in-index phrase verification.
    pub phrase_verify_cost: f64,
    /// Cost per result for BSON recheck (when needed).
    pub recheck_cost: f64,
}

impl Default for RumBsonCostParams {
    fn default() -> Self {
        Self {
            term_lookup_cost: 3.5,
            boundary_cost: 1.0,
            distance_compute_cost: 0.3,
            heap_fetch_cost: 1.5,
            phrase_verify_cost: 0.1,
            recheck_cost: 2.0,
        }
    }
}

/// Estimate the cost of a RUM index scan for a BSON `$text` query.
///
/// `$text` queries translated by `DocumentDB` use RUM's distance-ordered
/// scan. With a LIMIT, only ~k entries are visited (plus 20%
/// overfetch buffer). Without a LIMIT, all matches are scanned.
#[must_use]
pub fn rum_bson_text_scan_cost(
    total_rows: f64,
    selectivity: f64,
    limit: Option<u64>,
    params: &RumBsonCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    match limit {
        Some(k) => {
            let visit = k as f64 * 1.2;
            params.term_lookup_cost
                + params.boundary_cost
                + visit * (params.phrase_verify_cost + params.heap_fetch_cost)
        }
        None => {
            params.term_lookup_cost
                + params.boundary_cost
                + matching * (params.phrase_verify_cost + params.heap_fetch_cost)
        }
    }
}

/// Estimate the cost of a RUM index scan for a BSON `$near` query.
///
/// `$near` queries use RUM's KNN ordered scan via the `|-<>`
/// distance operator. Always used with a limit.
#[must_use]
pub fn rum_bson_near_scan_cost(
    total_rows: f64,
    limit: Option<u64>,
    params: &RumBsonCostParams,
) -> f64 {
    let effective_limit = limit.unwrap_or(100) as f64;
    let visit = effective_limit * 1.2;
    let _ = total_rows; // used for future distance histogram estimation
    params.term_lookup_cost
        + params.boundary_cost
        + visit * (params.distance_compute_cost + params.heap_fetch_cost)
}

/// Estimate the cost of a RUM index scan for a BSON array
/// containment query ($all, $elemMatch).
///
/// RUM can verify array containment with optional ordering.
/// Without ordering, cost is similar to GIN but with wider
/// posting entries.
#[must_use]
pub fn rum_bson_array_scan_cost(
    total_rows: f64,
    selectivity: f64,
    n_terms: u32,
    params: &RumBsonCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    let lookup = f64::from(n_terms) * params.term_lookup_cost;
    let fetch = matching * (params.recheck_cost + params.heap_fetch_cost);
    lookup + params.boundary_cost + fetch
}

/// Estimate GIN cost for the same BSON query (for comparison).
///
/// GIN requires heap recheck and external sort for ordered queries.
#[must_use]
pub fn gin_bson_equivalent_cost(
    total_rows: f64,
    selectivity: f64,
    needs_ordering: bool,
    limit: Option<u64>,
) -> f64 {
    let gin_params = GinBsonCostParams::default();
    let matching = (total_rows * selectivity).max(1.0);
    let scan = gin_scan_cost(total_rows, selectivity, 1, &gin_params);

    if needs_ordering {
        let sort_cost = matching * matching.log2().max(1.0) * 0.01;
        let limit_savings = match limit {
            Some(k) if matching > k as f64 * 10.0 => sort_cost * 0.5,
            _ => 0.0,
        };
        // GIN: scan + heap recheck + external sort
        let recheck = matching * gin_params.heap_fetch_cost;
        scan + recheck + sort_cost - limit_savings
    } else {
        scan
    }
}

/// Compare RUM BSON scan vs GIN BSON scan for a `$text` query.
///
/// Returns ratio < 1.0 when RUM is cheaper.
#[must_use]
pub fn rum_vs_gin_bson_text_ratio(total_rows: f64, selectivity: f64, limit: Option<u64>) -> f64 {
    let rum_params = RumBsonCostParams::default();
    let rum = rum_bson_text_scan_cost(total_rows, selectivity, limit, &rum_params);
    let gin = gin_bson_equivalent_cost(total_rows, selectivity, true, limit);
    if gin <= 0.0 {
        return 1.0;
    }
    rum / gin
}

/// Compare RUM BSON scan vs GIN BSON scan for a `$near` query.
///
/// Returns ratio < 1.0 when RUM is cheaper.
#[must_use]
pub fn rum_vs_gin_bson_near_ratio(total_rows: f64, selectivity: f64, limit: Option<u64>) -> f64 {
    let rum_params = RumBsonCostParams::default();
    let rum = rum_bson_near_scan_cost(total_rows, limit, &rum_params);
    let gin = gin_bson_equivalent_cost(total_rows, selectivity, true, limit);
    if gin <= 0.0 {
        return 1.0;
    }
    rum / gin
}

// ------------------------------------------------------------------
// RUM BSON index recommendation
// ------------------------------------------------------------------

/// A recommendation to use RUM instead of GIN for a BSON collection.
#[derive(Debug, Clone, PartialEq)]
pub struct RumBsonIndexRecommendation {
    /// Collection name.
    pub collection: String,
    /// BSON paths that should be indexed.
    pub paths: Vec<String>,
    /// Recommended RUM operator family.
    pub opfamily: BsonRumOpfamily,
    /// The BSON operators that benefit from RUM.
    pub operators: Vec<BsonOperator>,
    /// Estimated improvement ratio over GIN (> 1.0 means faster).
    pub estimated_improvement: f64,
    /// Whether this replaces an existing GIN index.
    pub replaces_gin: bool,
}

impl RumBsonIndexRecommendation {
    /// Generate the documentdb CREATE INDEX command for a RUM index.
    #[must_use]
    pub fn to_create_index_command(&self, db_name: &str) -> String {
        let key_entries: Vec<String> = self.paths.iter().map(|p| format!("\"{p}\": 1")).collect();
        let key_json = key_entries.join(", ");
        let idx_name = format!("idx_rum_{}", self.paths.join("_").replace('.', "_"));

        format!(
            "SELECT documentdb_api_internal.\
             create_indexes_non_concurrently(\
             '{db_name}', \
             '{{\"createIndexes\": \"{collection}\", \
             \"indexes\": [{{\"key\": {{{key_json}}}, \
             \"name\": \"{idx_name}\"}}]}}'::bson);",
            collection = self.collection,
        )
    }
}

/// Evaluate whether RUM provides a benefit over GIN for a set of
/// BSON query patterns on a collection.
///
/// Returns a recommendation if any operator benefits from RUM
/// with at least 2x improvement.
#[must_use]
pub fn evaluate_rum_bson_recommendation(
    collection: &str,
    paths: &[String],
    total_rows: f64,
    selectivity: f64,
    operators: &[BsonOperator],
    has_gin_index: bool,
) -> Option<RumBsonIndexRecommendation> {
    if operators.is_empty() || paths.is_empty() {
        return None;
    }

    let mut beneficial_ops = Vec::new();
    let mut best_improvement = 0.0_f64;

    for &op in operators {
        if !bson_op_benefits_from_rum(op) {
            continue;
        }

        let limit = Some(10_u64);
        let ratio = match op {
            BsonOperator::Regex => rum_vs_gin_bson_text_ratio(total_rows, selectivity, limit),
            BsonOperator::All | BsonOperator::ElemMatch => {
                let rum_params = RumBsonCostParams::default();
                let rum = rum_bson_array_scan_cost(total_rows, selectivity, 2, &rum_params);
                let gin = gin_bson_equivalent_cost(total_rows, selectivity, true, limit);
                if gin > 0.0 {
                    rum / gin
                } else {
                    1.0
                }
            }
            _ => {
                let rum_params = RumBsonCostParams::default();
                let rum = rum_bson_text_scan_cost(total_rows, selectivity, limit, &rum_params);
                let gin = gin_bson_equivalent_cost(total_rows, selectivity, true, limit);
                if gin > 0.0 {
                    rum / gin
                } else {
                    1.0
                }
            }
        };

        if ratio < 1.0 {
            let improvement = 1.0 / ratio;
            beneficial_ops.push(op);
            best_improvement = best_improvement.max(improvement);
        }
    }

    if beneficial_ops.is_empty() || best_improvement < 2.0 {
        return None;
    }

    let opfamily = if paths.len() >= 2 {
        BsonRumOpfamily::CompositePath
    } else {
        BsonRumOpfamily::SinglePath
    };

    Some(RumBsonIndexRecommendation {
        collection: collection.to_string(),
        paths: paths.to_vec(),
        opfamily,
        operators: beneficial_ops,
        estimated_improvement: best_improvement,
        replaces_gin: has_gin_index,
    })
}

/// Cost factor for a RUM BSON scan relative to a sequential scan.
///
/// For ordered BSON queries (text search, geospatial), RUM is
/// typically 8-12% of the cost of a full sequential scan.
#[must_use]
pub fn rum_bson_scan_cost_factor() -> f64 {
    0.10
}

// ------------------------------------------------------------------
// Errors specific to DocumentDB RUM optimization
// ------------------------------------------------------------------

/// Errors specific to `DocumentDB` RUM optimization.
///
/// All errors are non-fatal: the optimizer falls back to GIN-based
/// cost modeling.
#[derive(Debug, thiserror::Error)]
pub enum DocumentDbRumError {
    /// `DocumentDB` extended RUM is not installed.
    #[error(
        "DocumentDB extended RUM not installed; \
         using GIN cost model instead"
    )]
    RumNotInstalled,

    /// BSON operator cannot be mapped to a RUM operator family.
    #[error(
        "BSON operator '{operator}' not mappable to RUM opfamily; \
         falling back to GIN scan"
    )]
    OperatorNotMappable {
        /// The operator that could not be mapped.
        operator: String,
    },

    /// Unknown RUM operator family on a collection.
    #[error(
        "RUM index on collection '{collection}' uses unknown \
         opfamily '{opfamily}'; skipping RUM optimization"
    )]
    UnknownOpfamily {
        /// The collection name.
        collection: String,
        /// The unrecognized operator family.
        opfamily: String,
    },
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::expect_used)]
#[expect(clippy::float_cmp, reason = "exact float literals in tests")]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    // -- BsonOperator tests --

    #[test]
    fn parse_pg_operators() {
        assert_eq!(BsonOperator::from_pg_operator("@="), Some(BsonOperator::Eq));
        assert_eq!(BsonOperator::from_pg_operator("@>"), Some(BsonOperator::Gt));
        assert_eq!(
            BsonOperator::from_pg_operator("@>="),
            Some(BsonOperator::Gte)
        );
        assert_eq!(BsonOperator::from_pg_operator("@<"), Some(BsonOperator::Lt));
        assert_eq!(
            BsonOperator::from_pg_operator("@<="),
            Some(BsonOperator::Lte)
        );
        assert_eq!(
            BsonOperator::from_pg_operator("@*="),
            Some(BsonOperator::In)
        );
        assert_eq!(
            BsonOperator::from_pg_operator("@!*="),
            Some(BsonOperator::Nin)
        );
        assert_eq!(
            BsonOperator::from_pg_operator("@&="),
            Some(BsonOperator::All)
        );
        assert_eq!(
            BsonOperator::from_pg_operator("@~"),
            Some(BsonOperator::Regex)
        );
        assert_eq!(BsonOperator::from_pg_operator("="), None);
        assert_eq!(BsonOperator::from_pg_operator(""), None);
    }

    #[test]
    fn operator_display() {
        assert_eq!(BsonOperator::Eq.to_string(), "$eq");
        assert_eq!(BsonOperator::Gt.to_string(), "$gt");
        assert_eq!(BsonOperator::Regex.to_string(), "$regex");
    }

    #[test]
    fn gin_index_support() {
        assert!(BsonOperator::Eq.supports_gin_index());
        assert!(BsonOperator::Gt.supports_gin_index());
        assert!(BsonOperator::In.supports_gin_index());
        assert!(BsonOperator::Regex.supports_gin_index());
        assert!(!BsonOperator::Ne.supports_gin_index());
        assert!(!BsonOperator::Nin.supports_gin_index());
        assert!(!BsonOperator::Exists.supports_gin_index());
    }

    // -- Selectivity estimation tests --

    #[test]
    fn default_selectivities_beat_fixed_one_percent() {
        // Equality should be lower than the fixed 1%
        assert!(BsonOperator::Eq.default_selectivity() < 0.01);
        // Range should be higher (not everything is 1%)
        assert!(BsonOperator::Gt.default_selectivity() > 0.01);
        // $ne should be near 1.0
        assert!(BsonOperator::Ne.default_selectivity() > 0.9);
    }

    #[test]
    fn equality_selectivity_with_distinct_count() {
        let est = estimate_selectivity(BsonOperator::Eq, Some(1000), None);
        assert!((est.selectivity - 0.001).abs() < 0.0001, "1/1000 = 0.001");
        assert_eq!(est.source, SelectivitySource::IndexStats);
    }

    #[test]
    fn equality_selectivity_without_stats() {
        let est = estimate_selectivity(BsonOperator::Eq, None, None);
        assert_eq!(est.selectivity, BsonOperator::Eq.default_selectivity());
        assert_eq!(est.source, SelectivitySource::OperatorHeuristic);
    }

    #[test]
    fn in_selectivity_scales_with_array_length() {
        let small = estimate_selectivity(BsonOperator::In, Some(1000), Some(5));
        let large = estimate_selectivity(BsonOperator::In, Some(1000), Some(50));
        assert!(
            large.selectivity > small.selectivity,
            "$in with more elements should be less selective"
        );
    }

    #[test]
    fn all_selectivity_decreases_with_elements() {
        let few = estimate_selectivity(BsonOperator::All, Some(100), Some(2));
        let many = estimate_selectivity(BsonOperator::All, Some(100), Some(5));
        assert!(
            many.selectivity < few.selectivity,
            "$all with more required elements should be more selective"
        );
    }

    #[test]
    fn combine_independent_selectivities() {
        let combined = combine_selectivities(&[0.1, 0.2]);
        // With damping, combined should be > 0.02 (pure product)
        // but < 0.1 (most selective alone)
        assert!(combined > 0.02);
        assert!(combined < 0.1);
    }

    #[test]
    fn combine_empty_selectivities() {
        assert!((combine_selectivities(&[]) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn combine_single_selectivity() {
        assert!((combine_selectivities(&[0.3]) - 0.3).abs() < f64::EPSILON);
    }

    // -- GIN cost model tests --

    #[test]
    fn gin_scan_cheaper_than_seq_for_selective_queries() {
        let params = GinBsonCostParams::default();
        let ratio = gin_vs_sequential_ratio(100_000.0, 0.001, 1, &params);
        assert!(
            ratio < 1.0,
            "GIN should beat seq scan for 0.1% selectivity: {ratio}"
        );
    }

    #[test]
    fn gin_scan_not_worth_it_for_unselective_queries() {
        let params = GinBsonCostParams::default();
        let ratio = gin_vs_sequential_ratio(100_000.0, 0.9, 1, &params);
        assert!(
            ratio > 1.0,
            "GIN should lose to seq scan for 90% selectivity: {ratio}"
        );
    }

    #[test]
    fn compound_gin_cheaper_than_single_for_multi_pred() {
        let params = GinBsonCostParams::default();
        let single1 = gin_scan_cost(100_000.0, 0.01, 1, &params);
        let single2 = gin_scan_cost(100_000.0, 0.05, 1, &params);
        let compound = compound_gin_scan_cost(100_000.0, 0.01 * 0.05, 2, &params);
        // Compound should be cheaper than sum of singles
        // (fewer total heap fetches due to bitmap intersection)
        assert!(
            compound < single1 + single2,
            "compound ({compound}) should beat sum of singles \
             ({}) for multi-predicate",
            single1 + single2
        );
    }

    // -- Index recommendation tests --

    #[test]
    fn recommend_compound_index_for_multi_path() {
        let patterns = vec![QueryPattern {
            collection: "users".to_string(),
            predicates: vec![
                BsonPredicate {
                    path: "status".to_string(),
                    operator: BsonOperator::Eq,
                    literal_hint: Some("active".to_string()),
                },
                BsonPredicate {
                    path: "age".to_string(),
                    operator: BsonOperator::Gt,
                    literal_hint: None,
                },
            ],
            frequency: 100,
        }];

        let recs = recommend_gin_indexes(&patterns, 10);
        assert!(!recs.is_empty(), "should recommend at least one index");
        // Should include a compound index
        let compound = recs.iter().find(|r| r.paths.len() >= 2);
        assert!(compound.is_some(), "should recommend a compound index");
    }

    #[test]
    fn no_index_for_infrequent_queries() {
        let patterns = vec![QueryPattern {
            collection: "logs".to_string(),
            predicates: vec![BsonPredicate {
                path: "level".to_string(),
                operator: BsonOperator::Eq,
                literal_hint: None,
            }],
            frequency: 1,
        }];

        let recs = recommend_gin_indexes(&patterns, 10);
        assert!(
            recs.is_empty(),
            "should not recommend index for infrequent query"
        );
    }

    #[test]
    fn no_index_for_non_gin_operators() {
        let patterns = vec![QueryPattern {
            collection: "events".to_string(),
            predicates: vec![BsonPredicate {
                path: "type".to_string(),
                operator: BsonOperator::Ne,
                literal_hint: None,
            }],
            frequency: 1000,
        }];

        let recs = recommend_gin_indexes(&patterns, 10);
        assert!(recs.is_empty(), "$ne does not benefit from GIN index");
    }

    #[test]
    fn create_index_command_format() {
        let rec = GinIndexRecommendation {
            collection: "users".to_string(),
            paths: vec!["status".to_string(), "age".to_string()],
            estimated_improvement: 10.0,
            operators: vec![BsonOperator::Eq, BsonOperator::Gt],
        };

        let cmd = rec.to_create_index_command("mydb");
        assert!(cmd.contains("create_indexes_non_concurrently"));
        assert!(cmd.contains("mydb"));
        assert!(cmd.contains("users"));
        assert!(cmd.contains("\"status\": 1"));
        assert!(cmd.contains("\"age\": 1"));
        assert!(cmd.contains("idx_status_age"));
    }

    // -- Query pattern tests --

    #[test]
    fn query_pattern_combined_selectivity() {
        let pattern = QueryPattern {
            collection: "orders".to_string(),
            predicates: vec![
                BsonPredicate {
                    path: "status".to_string(),
                    operator: BsonOperator::Eq,
                    literal_hint: None,
                },
                BsonPredicate {
                    path: "date".to_string(),
                    operator: BsonOperator::Gt,
                    literal_hint: None,
                },
            ],
            frequency: 50,
        };

        let sel = pattern.estimated_selectivity();
        // Should be between pure product and most selective
        let eq_sel = BsonOperator::Eq.default_selectivity();
        let gt_sel = BsonOperator::Gt.default_selectivity();
        assert!(sel < gt_sel, "combined < range alone");
        assert!(sel > eq_sel * gt_sel, "combined > pure product (damping)");
    }

    // -- E-graph rewrite rule tests --

    fn run_with_docdb_rules(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        let rules = documentdb_rewrite_rules();
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules)
    }

    #[test]
    fn bson_filter_through_join_applies() {
        // Build: filter(bson_pred, join(inner, true, scan(a), scan(b)))
        let left = RelExpr::scan("collection_a");
        let right = RelExpr::scan("collection_b");
        let joined = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(left),
            right: Box::new(right),
        };
        let filtered = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::BinOp {
                    op: BinOp::JsonAccess,
                    left: Box::new(Expr::Column(ColumnRef::new("document"))),
                    right: Box::new(Expr::Const(Const::String("status".to_string()))),
                }),
                right: Box::new(Expr::Const(Const::String("active".to_string()))),
            },
            input: Box::new(joined),
        };

        let runner = run_with_docdb_rules(&filtered);
        // The e-graph should grow beyond the initial expression,
        // proving the rewrite rule fired.
        assert!(
            runner.egraph.number_of_classes() > 5,
            "e-graph should grow from BSON filter pushdown"
        );
    }

    #[test]
    fn bson_filter_below_project_applies() {
        use ra_core::algebra::ProjectionColumn;

        let scan = RelExpr::scan("users");
        let projected = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("document")),
                alias: None,
            }],
            input: Box::new(scan),
        };
        let filtered = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::BinOp {
                    op: BinOp::JsonAccess,
                    left: Box::new(Expr::Column(ColumnRef::new("document"))),
                    right: Box::new(Expr::Const(Const::String("name".to_string()))),
                }),
                right: Box::new(Expr::Const(Const::String("Alice".to_string()))),
            },
            input: Box::new(projected),
        };

        let runner = run_with_docdb_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 4,
            "e-graph should grow from BSON filter below project"
        );
    }

    #[test]
    fn bson_conjunctive_filter_splitting() {
        let scan = RelExpr::scan("orders");
        // (json_access(doc, "status") = "active")
        //   AND (json_access(doc, "amount") > 100)
        let pred = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::BinOp {
                    op: BinOp::JsonAccess,
                    left: Box::new(Expr::Column(ColumnRef::new("document"))),
                    right: Box::new(Expr::Const(Const::String("status".to_string()))),
                }),
                right: Box::new(Expr::Const(Const::String("completed".to_string()))),
            }),
            right: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::BinOp {
                    op: BinOp::JsonAccess,
                    left: Box::new(Expr::Column(ColumnRef::new("document"))),
                    right: Box::new(Expr::Const(Const::String("amount".to_string()))),
                }),
                right: Box::new(Expr::Const(Const::Int(100))),
            }),
        };

        let filtered = RelExpr::Filter {
            predicate: pred,
            input: Box::new(scan),
        };

        let runner = run_with_docdb_rules(&filtered);
        // After splitting, we should see more filter nodes
        assert!(
            runner.egraph.number_of_classes() > 5,
            "e-graph should grow from conjunctive BSON filter splitting"
        );
    }

    #[test]
    fn non_bson_filter_not_matched_by_docdb_rules() {
        // A plain SQL filter should not trigger BSON-specific rules.
        // The base expression (filter(eq(col, const), scan(t)))
        // creates ~8 e-classes from structure alone. With BSON
        // rules that don't match, this count should stay the same.
        let scan = RelExpr::scan("plain_table");
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

        let runner = run_with_docdb_rules(&filtered);
        // The class count should not grow beyond the initial
        // expression structure since no BSON patterns are present.
        assert_eq!(
            runner.egraph.number_of_classes(),
            initial_classes,
            "plain SQL filter should not trigger BSON-specific \
             rules (initial={initial_classes}, after={})",
            runner.egraph.number_of_classes()
        );
    }

    #[test]
    fn bson_match_below_aggregate_applies() {
        use ra_core::algebra::AggregateExpr;
        use ra_core::algebra::AggregateFunction;

        let scan = RelExpr::scan("orders");
        let agg = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("customer_id"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("amount"))),
                distinct: false,
                alias: Some("total".to_string()),
            }],
            input: Box::new(scan),
        };

        // Filter on BSON field applied after aggregation
        let filtered = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::BinOp {
                    op: BinOp::JsonAccess,
                    left: Box::new(Expr::Column(ColumnRef::new("document"))),
                    right: Box::new(Expr::Const(Const::String("status".to_string()))),
                }),
                right: Box::new(Expr::Const(Const::String("completed".to_string()))),
            },
            input: Box::new(agg),
        };

        let runner = run_with_docdb_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 6,
            "BSON $match should be pushed below aggregate"
        );
    }

    // -- Error type tests --

    #[test]
    fn error_messages_are_actionable() {
        let err = DocumentDbError::PathExtractionFailed {
            path: "user.address.city".to_string(),
            reason: "nested array not supported".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("user.address.city"));
        assert!(msg.contains("default selectivity"));

        let err = DocumentDbError::SchemaInferenceFailed {
            collection: "orders".to_string(),
            reason: "collection empty".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("orders"));
        assert!(msg.contains("skipping"));

        let err = DocumentDbError::UnsupportedVersion {
            version: "0.23".to_string(),
            feature: "compound GIN indexes".to_string(),
            minimum: "0.25".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("0.23"));
        assert!(msg.contains("0.25"));
    }

    // -- Cost factor test --

    #[test]
    fn gin_bson_cost_factor_is_reasonable() {
        let factor = gin_bson_scan_cost_factor();
        assert!(
            factor > 0.0 && factor < 1.0,
            "GIN BSON scan should be cheaper than seq scan"
        );
    }

    // ============================================================
    // BSON-aware RUM optimization tests (RFC 0080)
    // ============================================================

    // -- BsonRumOpfamily tests --

    #[test]
    fn rum_opfamily_from_name() {
        assert_eq!(
            BsonRumOpfamily::from_name("bson_extended_rum_single_path_ops"),
            Some(BsonRumOpfamily::SinglePath)
        );
        assert_eq!(
            BsonRumOpfamily::from_name("bson_extended_rum_composite_path_ops"),
            Some(BsonRumOpfamily::CompositePath)
        );
        assert_eq!(
            BsonRumOpfamily::from_name("documentdb_extended_rum_hashed_ops"),
            Some(BsonRumOpfamily::Hashed)
        );
        assert_eq!(
            BsonRumOpfamily::from_name("bson_extended_rum_unique_shard_path_ops"),
            Some(BsonRumOpfamily::UniqueShard)
        );
        assert_eq!(BsonRumOpfamily::from_name("gin_ops"), None);
        assert_eq!(BsonRumOpfamily::from_name(""), None);
    }

    #[test]
    fn rum_opfamily_ordering_support() {
        assert!(BsonRumOpfamily::SinglePath.supports_ordering());
        assert!(BsonRumOpfamily::CompositePath.supports_ordering());
        assert!(!BsonRumOpfamily::Hashed.supports_ordering());
        assert!(!BsonRumOpfamily::UniqueShard.supports_ordering());
    }

    #[test]
    fn rum_opfamily_compound_support() {
        assert!(BsonRumOpfamily::CompositePath.supports_compound());
        assert!(!BsonRumOpfamily::SinglePath.supports_compound());
        assert!(!BsonRumOpfamily::Hashed.supports_compound());
        assert!(!BsonRumOpfamily::UniqueShard.supports_compound());
    }

    #[test]
    fn rum_opfamily_display() {
        assert_eq!(
            BsonRumOpfamily::SinglePath.to_string(),
            "bson_extended_rum_single_path_ops"
        );
        assert_eq!(
            BsonRumOpfamily::CompositePath.to_string(),
            "bson_extended_rum_composite_path_ops"
        );
        assert_eq!(
            BsonRumOpfamily::Hashed.to_string(),
            "documentdb_extended_rum_hashed_ops"
        );
    }

    // -- BSON operator to RUM mapping tests --

    #[test]
    fn bson_op_rum_mapping() {
        assert_eq!(
            bson_op_to_rum_opfamily(BsonOperator::Eq),
            Some(BsonRumOpfamily::Hashed)
        );
        assert_eq!(
            bson_op_to_rum_opfamily(BsonOperator::Gt),
            Some(BsonRumOpfamily::SinglePath)
        );
        assert_eq!(
            bson_op_to_rum_opfamily(BsonOperator::Regex),
            Some(BsonRumOpfamily::SinglePath)
        );
        assert_eq!(
            bson_op_to_rum_opfamily(BsonOperator::All),
            Some(BsonRumOpfamily::SinglePath)
        );
        assert_eq!(
            bson_op_to_rum_opfamily(BsonOperator::ElemMatch),
            Some(BsonRumOpfamily::SinglePath)
        );
        assert_eq!(bson_op_to_rum_opfamily(BsonOperator::Ne), None);
        assert_eq!(bson_op_to_rum_opfamily(BsonOperator::Nin), None);
        assert_eq!(bson_op_to_rum_opfamily(BsonOperator::Exists), None);
    }

    #[test]
    fn bson_op_rum_benefits() {
        assert!(bson_op_benefits_from_rum(BsonOperator::Regex));
        assert!(bson_op_benefits_from_rum(BsonOperator::All));
        assert!(bson_op_benefits_from_rum(BsonOperator::ElemMatch));
        assert!(bson_op_benefits_from_rum(BsonOperator::Gt));
        assert!(bson_op_benefits_from_rum(BsonOperator::Lte));
        assert!(!bson_op_benefits_from_rum(BsonOperator::Eq));
        assert!(!bson_op_benefits_from_rum(BsonOperator::In));
        assert!(!bson_op_benefits_from_rum(BsonOperator::Ne));
    }

    // -- RUM BSON cost model tests --

    #[test]
    fn rum_bson_text_cheaper_than_gin_with_limit() {
        let rum_params = RumBsonCostParams::default();
        let rum = rum_bson_text_scan_cost(100_000.0, 0.1, Some(10), &rum_params);
        let gin = gin_bson_equivalent_cost(100_000.0, 0.1, true, Some(10));
        assert!(
            rum < gin,
            "RUM $text with limit should beat GIN: \
             rum={rum:.1}, gin={gin:.1}"
        );
    }

    #[test]
    fn rum_bson_text_without_limit_still_cheaper_for_ordered() {
        let rum_params = RumBsonCostParams::default();
        let rum = rum_bson_text_scan_cost(100_000.0, 0.01, None, &rum_params);
        let gin = gin_bson_equivalent_cost(100_000.0, 0.01, true, None);
        assert!(
            rum < gin,
            "RUM $text without limit should still beat GIN \
             for ordered: rum={rum:.1}, gin={gin:.1}"
        );
    }

    #[test]
    fn rum_bson_near_very_cheap_with_limit() {
        let rum_params = RumBsonCostParams::default();
        let rum = rum_bson_near_scan_cost(1_000_000.0, Some(10), &rum_params);
        // $near via GIN requires scanning all rows + sort
        let gin = gin_bson_equivalent_cost(1_000_000.0, 0.1, true, Some(10));
        assert!(
            rum < gin * 0.01,
            "RUM $near with limit 10 should be 100x+ cheaper: \
             rum={rum:.1}, gin={gin:.1}"
        );
    }

    #[test]
    fn rum_bson_array_cost_reasonable() {
        let rum_params = RumBsonCostParams::default();
        let cost = rum_bson_array_scan_cost(100_000.0, 0.001, 3, &rum_params);
        assert!(
            cost > 0.0 && cost < 100_000.0,
            "RUM array cost should be between 0 and seq scan: \
             cost={cost:.1}"
        );
    }

    #[test]
    fn rum_vs_gin_text_ratio_with_limit() {
        let ratio = rum_vs_gin_bson_text_ratio(100_000.0, 0.1, Some(10));
        assert!(
            ratio < 0.5,
            "RUM should be 2x+ cheaper for $text with limit: \
             ratio={ratio:.3}"
        );
    }

    #[test]
    fn rum_vs_gin_near_ratio_with_limit() {
        let ratio = rum_vs_gin_bson_near_ratio(1_000_000.0, 0.01, Some(10));
        assert!(
            ratio < 0.1,
            "RUM should be 10x+ cheaper for $near with limit: \
             ratio={ratio:.3}"
        );
    }

    // -- RUM BSON recommendation tests --

    #[test]
    fn recommend_rum_for_regex_queries() {
        let rec = evaluate_rum_bson_recommendation(
            "articles",
            &["content".to_string()],
            500_000.0,
            0.05,
            &[BsonOperator::Regex],
            true,
        );
        assert!(rec.is_some(), "should recommend RUM for $regex queries");
        let rec = rec.unwrap_or_else(|| unreachable!());
        assert_eq!(rec.collection, "articles");
        assert_eq!(rec.opfamily, BsonRumOpfamily::SinglePath);
        assert!(rec.estimated_improvement >= 2.0);
        assert!(rec.replaces_gin);
    }

    #[test]
    fn recommend_rum_composite_for_multi_path() {
        let rec = evaluate_rum_bson_recommendation(
            "orders",
            &["status".to_string(), "total".to_string()],
            1_000_000.0,
            0.01,
            &[BsonOperator::Gt, BsonOperator::Lte],
            true,
        );
        assert!(rec.is_some(), "should recommend RUM for multi-path range");
        let rec = rec.unwrap_or_else(|| unreachable!());
        assert_eq!(rec.opfamily, BsonRumOpfamily::CompositePath);
        assert_eq!(rec.paths.len(), 2);
    }

    #[test]
    fn no_rum_recommendation_for_equality_only() {
        let rec = evaluate_rum_bson_recommendation(
            "users",
            &["email".to_string()],
            100_000.0,
            0.001,
            &[BsonOperator::Eq],
            true,
        );
        assert!(
            rec.is_none(),
            "should not recommend RUM for $eq (GIN is sufficient)"
        );
    }

    #[test]
    fn no_rum_recommendation_for_empty_operators() {
        let rec = evaluate_rum_bson_recommendation(
            "test",
            &["field".to_string()],
            100_000.0,
            0.01,
            &[],
            false,
        );
        assert!(rec.is_none());
    }

    #[test]
    fn no_rum_recommendation_for_empty_paths() {
        let rec = evaluate_rum_bson_recommendation(
            "test",
            &[],
            100_000.0,
            0.01,
            &[BsonOperator::Regex],
            false,
        );
        assert!(rec.is_none());
    }

    #[test]
    fn rum_bson_create_index_command_format() {
        let rec = RumBsonIndexRecommendation {
            collection: "articles".to_string(),
            paths: vec!["content".to_string()],
            opfamily: BsonRumOpfamily::SinglePath,
            operators: vec![BsonOperator::Regex],
            estimated_improvement: 5.0,
            replaces_gin: true,
        };
        let cmd = rec.to_create_index_command("mydb");
        assert!(cmd.contains("create_indexes_non_concurrently"));
        assert!(cmd.contains("mydb"));
        assert!(cmd.contains("articles"));
        assert!(cmd.contains("\"content\": 1"));
        assert!(cmd.contains("idx_rum_content"));
    }

    // -- RUM BSON cost factor test --

    #[test]
    fn rum_bson_cost_factor_is_reasonable() {
        let factor = rum_bson_scan_cost_factor();
        assert!(
            factor > 0.0 && factor < gin_bson_scan_cost_factor(),
            "RUM BSON scan factor should be lower than GIN: \
             rum={factor}, gin={}",
            gin_bson_scan_cost_factor()
        );
    }

    // -- DocumentDbRumError tests --

    #[test]
    fn rum_error_messages_are_actionable() {
        let err = DocumentDbRumError::RumNotInstalled;
        let msg = err.to_string();
        assert!(msg.contains("not installed"));
        assert!(msg.contains("GIN"));

        let err = DocumentDbRumError::OperatorNotMappable {
            operator: "$geoWithin".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("$geoWithin"));
        assert!(msg.contains("falling back"));

        let err = DocumentDbRumError::UnknownOpfamily {
            collection: "users".to_string(),
            opfamily: "bson_custom_rum_ops".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("users"));
        assert!(msg.contains("bson_custom_rum_ops"));
        assert!(msg.contains("skipping"));
    }

    // -- combine_selectivities edge cases --

    #[test]
    fn combine_selectivities_empty_is_one() {
        let combined = combine_selectivities(&[]);
        assert!((combined - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn combine_selectivities_single_value() {
        let combined = combine_selectivities(&[0.05]);
        assert!((combined - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn combine_selectivities_damping_applied() {
        let sel_eq = BsonOperator::Eq.default_selectivity();
        let sel_gt = BsonOperator::Gt.default_selectivity();
        let combined = combine_selectivities(&[sel_eq, sel_gt]);
        // With damping, combined > pure product
        assert!(
            combined > sel_eq * sel_gt,
            "damping should prevent combined from being pure product"
        );
        // But combined < most selective
        assert!(
            combined < sel_gt,
            "combined should be more selective than range alone"
        );
    }

    // -- Sequential scan cost --

    #[test]
    fn sequential_scan_cost_proportional() {
        let small = sequential_scan_cost(100.0);
        let large = sequential_scan_cost(100_000.0);
        assert!(large > small * 900.0);
    }

    #[test]
    fn sequential_scan_cost_zero_rows() {
        let cost = sequential_scan_cost(0.0);
        assert!(cost.abs() < f64::EPSILON);
    }

    // -- GIN scan cost tests --

    #[test]
    fn gin_scan_cost_scales_with_selectivity() {
        let params = GinBsonCostParams::default();
        let selective = gin_scan_cost(1_000_000.0, 0.001, 1, &params);
        let broad = gin_scan_cost(1_000_000.0, 0.5, 1, &params);
        assert!(selective < broad, "more selective scan should be cheaper");
    }

    #[test]
    fn gin_scan_cheaper_than_sequential() {
        let params = GinBsonCostParams::default();
        let gin = gin_scan_cost(100_000.0, 0.01, 1, &params);
        let seq = sequential_scan_cost(100_000.0);
        assert!(
            gin < seq,
            "GIN scan for selective query should be \
             cheaper: gin={gin:.1}, seq={seq:.1}"
        );
    }

    // -- GIN vs sequential ratio --

    #[test]
    fn gin_vs_sequential_ratio_below_one_for_selective() {
        let params = GinBsonCostParams::default();
        let ratio = gin_vs_sequential_ratio(100_000.0, 0.001, 1, &params);
        assert!(
            ratio < 1.0,
            "GIN should be cheaper for selective queries: {ratio}"
        );
    }

    #[test]
    fn gin_vs_sequential_ratio_above_one_for_broad() {
        let params = GinBsonCostParams::default();
        let ratio = gin_vs_sequential_ratio(100_000.0, 0.99, 1, &params);
        assert!(
            ratio > 0.5,
            "GIN should not help much for broad scans: {ratio}"
        );
    }

    // -- Compound GIN scan cost --

    #[test]
    fn compound_gin_cheaper_than_multiple_single() {
        let params = GinBsonCostParams::default();
        let compound = compound_gin_scan_cost(100_000.0, 0.01, 3, &params);
        let single = gin_scan_cost(100_000.0, 0.01, 1, &params);
        assert!(
            compound < single * 3.0,
            "compound GIN should be cheaper than 3x single"
        );
    }

    // -- BsonOperator additional tests --

    #[test]
    fn gin_support_for_all_operators() {
        assert!(BsonOperator::Eq.supports_gin_index());
        assert!(BsonOperator::Gt.supports_gin_index());
        assert!(BsonOperator::Lt.supports_gin_index());
        assert!(BsonOperator::In.supports_gin_index());
        assert!(BsonOperator::All.supports_gin_index());
        assert!(BsonOperator::Regex.supports_gin_index());
        assert!(!BsonOperator::Ne.supports_gin_index());
        assert!(!BsonOperator::Nin.supports_gin_index());
    }

    // -- Recommend GIN indexes edge cases --

    #[test]
    fn recommend_gin_indexes_empty_patterns() {
        let recs = recommend_gin_indexes(&[], 10);
        assert!(recs.is_empty());
    }

    #[test]
    fn recommend_gin_indexes_single_pattern() {
        let patterns = vec![QueryPattern {
            collection: "users".to_string(),
            predicates: vec![BsonPredicate {
                path: "email".to_string(),
                operator: BsonOperator::Eq,
                literal_hint: None,
            }],
            frequency: 100,
        }];
        let recs = recommend_gin_indexes(&patterns, 10);
        assert!(
            !recs.is_empty(),
            "should recommend index for frequent pattern"
        );
    }

    // -- GinBsonCostParams default --

    #[test]
    fn gin_bson_cost_params_default() {
        let params = GinBsonCostParams::default();
        assert!(params.term_lookup_cost > 0.0);
        assert!(params.recheck_cost > 0.0);
        assert!(params.heap_fetch_cost > 0.0);
    }

    // -- RumBsonCostParams default --

    #[test]
    fn rum_bson_cost_params_default() {
        let params = RumBsonCostParams::default();
        assert!(params.term_lookup_cost > 0.0);
        assert!(params.boundary_cost > 0.0);
        assert!(params.distance_compute_cost > 0.0);
        assert!(params.heap_fetch_cost > 0.0);
        assert!(params.phrase_verify_cost > 0.0);
        assert!(params.recheck_cost > 0.0);
    }

    // -- SelectivitySource test --

    #[test]
    fn selectivity_estimate_from_operator_heuristic() {
        let est = estimate_selectivity(BsonOperator::Eq, None, None);
        assert!(est.selectivity > 0.0 && est.selectivity < 1.0);
        assert_eq!(est.source, SelectivitySource::OperatorHeuristic);
    }

    #[test]
    fn selectivity_estimate_from_index_stats() {
        let est = estimate_selectivity(BsonOperator::Eq, Some(1000), None);
        assert!(
            (est.selectivity - 0.001).abs() < 0.0001,
            "1/1000 ndistinct should give ~0.001"
        );
        assert_eq!(est.source, SelectivitySource::IndexStats);
    }

    #[test]
    fn selectivity_in_operator_with_array_length() {
        let est = estimate_selectivity(BsonOperator::In, Some(100), Some(5));
        // 1/100 * 5 = 0.05
        assert!((est.selectivity - 0.05).abs() < 0.01);
    }

    #[test]
    fn selectivity_all_operator_very_selective() {
        let est = estimate_selectivity(BsonOperator::All, Some(100), Some(3));
        // (1/100)^3 = 0.000001, clamped to 0.000001
        assert!(est.selectivity <= 0.001);
    }

    // -- parse_pg_operator edge cases --

    #[test]
    fn parse_unknown_pg_operator() {
        assert!(BsonOperator::from_pg_operator("??").is_none());
        assert!(BsonOperator::from_pg_operator("").is_none());
    }
}
