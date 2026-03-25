//! Oracle JSON Relational Duality view optimization.
//!
//! Oracle 23ai introduced JSON Relational Duality views, which expose
//! normalized relational tables as JSON documents and vice versa. The
//! same data can be accessed through either a document API (JSON CRUD)
//! or a relational API (SQL joins). This module helps the optimizer
//! choose the cheapest access path and apply duality-aware rewrites.
//!
//! Key optimizations:
//! - Access path selection (document fetch vs relational decomposition)
//! - Predicate pushdown from JSON fields to relational columns
//! - Partial document assembly (skip joins for unreferenced fields)
//! - Update fan-out cost estimation
//! - E-graph rewrite rules for duality view patterns
//!
//! See: `rfcs/text/0084-oracle-json-relational-duality-optimization.md`

use std::collections::HashMap;

use egg::{rewrite, Id, Rewrite, Subst, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

// ------------------------------------------------------------------
// Duality view model
// ------------------------------------------------------------------

/// How a JSON field maps to relational storage.
#[derive(Debug, Clone, PartialEq)]
pub enum DualityFieldMapping {
    /// Scalar column in the root or a joined table.
    Column {
        table: String,
        column: String,
    },
    /// Nested JSON object assembled from a joined table.
    Nested {
        table: String,
        join_condition: String,
        fields: Vec<DualityField>,
    },
    /// JSON array assembled from a one-to-many joined table.
    Array {
        table: String,
        join_condition: String,
        fields: Vec<DualityField>,
    },
}

/// A single field within a duality view definition.
#[derive(Debug, Clone, PartialEq)]
pub struct DualityField {
    /// JSON field name (e.g. "customerName").
    pub json_path: String,
    /// How this field maps to relational storage.
    pub mapping: DualityFieldMapping,
}

/// Updatability flags for a duality view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Updatability {
    pub insert: bool,
    pub update: bool,
    pub delete: bool,
}

impl Updatability {
    #[must_use]
    pub fn read_only() -> Self {
        Self { insert: false, update: false, delete: false }
    }

    #[must_use]
    pub fn full() -> Self {
        Self { insert: true, update: true, delete: true }
    }
}

/// An Oracle JSON Relational Duality view definition.
#[derive(Debug, Clone, PartialEq)]
pub struct DualityView {
    /// View name (e.g. "orders_dv").
    pub name: String,
    /// Root table that anchors the document.
    pub root_table: String,
    /// Field mappings from JSON paths to relational columns.
    pub fields: Vec<DualityField>,
    /// DML permissions on this view.
    pub updatability: Updatability,
}

impl DualityView {
    /// Return all base tables referenced by this view.
    #[must_use]
    pub fn referenced_tables(&self) -> Vec<String> {
        let mut tables = vec![self.root_table.clone()];
        for field in &self.fields {
            collect_tables(&field.mapping, &mut tables);
        }
        tables.dedup();
        tables
    }

    /// Count the total number of leaf fields in the document.
    #[must_use]
    pub fn field_count(&self) -> usize {
        self.fields.iter().map(count_leaf_fields).sum()
    }

    /// Count joins required for full document assembly.
    #[must_use]
    pub fn join_count(&self) -> usize {
        self.fields.iter().map(count_joins).sum()
    }

    /// Return fields that require the given table.
    #[must_use]
    pub fn fields_from_table(
        &self,
        table: &str,
    ) -> Vec<&DualityField> {
        self.fields
            .iter()
            .filter(|f| field_uses_table(&f.mapping, table))
            .collect()
    }
}

fn collect_tables(
    mapping: &DualityFieldMapping,
    tables: &mut Vec<String>,
) {
    match mapping {
        DualityFieldMapping::Column { table, .. } => {
            if !tables.contains(table) {
                tables.push(table.clone());
            }
        }
        DualityFieldMapping::Nested { table, fields, .. }
        | DualityFieldMapping::Array { table, fields, .. } => {
            if !tables.contains(table) {
                tables.push(table.clone());
            }
            for field in fields {
                collect_tables(&field.mapping, tables);
            }
        }
    }
}

fn count_leaf_fields(field: &DualityField) -> usize {
    match &field.mapping {
        DualityFieldMapping::Column { .. } => 1,
        DualityFieldMapping::Nested { fields, .. }
        | DualityFieldMapping::Array { fields, .. } => {
            fields.iter().map(count_leaf_fields).sum()
        }
    }
}

fn count_joins(field: &DualityField) -> usize {
    match &field.mapping {
        DualityFieldMapping::Column { .. } => 0,
        DualityFieldMapping::Nested { fields, .. }
        | DualityFieldMapping::Array { fields, .. } => {
            1 + fields.iter().map(count_joins).sum::<usize>()
        }
    }
}

fn field_uses_table(
    mapping: &DualityFieldMapping,
    table: &str,
) -> bool {
    match mapping {
        DualityFieldMapping::Column { table: t, .. } => t == table,
        DualityFieldMapping::Nested { table: t, fields, .. }
        | DualityFieldMapping::Array { table: t, fields, .. } => {
            t == table
                || fields
                    .iter()
                    .any(|f| field_uses_table(&f.mapping, table))
        }
    }
}

// ------------------------------------------------------------------
// Access path selection
// ------------------------------------------------------------------

/// The two fundamental access strategies for a duality view query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPath {
    /// Fetch the pre-assembled JSON document.
    Document,
    /// Decompose into relational joins on base tables.
    Relational,
}

/// Cost parameters for duality view access.
#[derive(Debug, Clone)]
pub struct DualityCostParams {
    /// Base cost to fetch one JSON document (default: 5.0).
    pub document_fetch_cost: f64,
    /// Cost per field extracted from a JSON document (default: 0.2).
    pub json_field_extract_cost: f64,
    /// Cost to evaluate a JSON path predicate (default: 0.8).
    pub json_predicate_cost: f64,
    /// Cost per relational join (default: 3.0).
    pub relational_join_cost: f64,
    /// Cost to assemble a partial document from relational data
    /// (default: 1.5).
    pub partial_assembly_cost: f64,
    /// Per-table cost for maintaining consistency during updates
    /// (default: 2.0).
    pub update_fanout_cost: f64,
    /// Cost per row for a relational table scan (default: 1.0).
    pub relational_scan_cost_per_row: f64,
}

impl Default for DualityCostParams {
    fn default() -> Self {
        Self {
            document_fetch_cost: 5.0,
            json_field_extract_cost: 0.2,
            json_predicate_cost: 0.8,
            relational_join_cost: 3.0,
            partial_assembly_cost: 1.5,
            update_fanout_cost: 2.0,
            relational_scan_cost_per_row: 1.0,
        }
    }
}

/// Estimate the cost of the document access path.
///
/// When predicates are present, the document path must scan all
/// documents to evaluate JSON path predicates (unless a JSON
/// search index exists). The scan cost covers all rows; the
/// extraction cost covers only matching rows.
///
/// Cost = total_rows * pred_eval + matching * (fetch + extract)
#[must_use]
pub fn estimate_document_cost(
    total_rows: f64,
    selectivity: f64,
    n_fields_accessed: u32,
    n_predicates: u32,
    params: &DualityCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);

    let predicate_scan = if n_predicates > 0 {
        total_rows
            * f64::from(n_predicates)
            * params.json_predicate_cost
    } else {
        0.0
    };

    let per_match = params.document_fetch_cost
        + f64::from(n_fields_accessed)
            * params.json_field_extract_cost;
    let fetch_cost = matching * per_match;

    predicate_scan + fetch_cost
}

/// Estimate the cost of the relational decomposition path.
///
/// Cost = sum(table_scans) + join_overhead + assembly_per_row
///
/// The join overhead scales with the number of result rows (after
/// applying selectivity) rather than the full table size, because
/// index-based joins prune early when predicates are selective.
#[must_use]
pub fn estimate_relational_cost(
    table_row_counts: &[f64],
    selectivity: f64,
    n_joins: u32,
    n_fields_accessed: u32,
    total_fields: u32,
    params: &DualityCostParams,
) -> f64 {
    let scan_cost: f64 = table_row_counts
        .iter()
        .map(|rows| {
            rows * selectivity * params.relational_scan_cost_per_row
        })
        .sum();

    let root_rows = table_row_counts
        .first()
        .copied()
        .unwrap_or(1000.0);
    let result_rows = (root_rows * selectivity).max(1.0);
    let join_cost = f64::from(n_joins)
        * params.relational_join_cost
        * result_rows.sqrt();

    let assembly_rows = result_rows;
    let assembly = if n_fields_accessed < total_fields {
        params.partial_assembly_cost
            * f64::from(n_fields_accessed)
            * assembly_rows
    } else {
        params.partial_assembly_cost
            * f64::from(total_fields)
            * assembly_rows
    };

    scan_cost + join_cost + assembly
}

/// Estimate the cost of updating through a duality view.
///
/// Each updated JSON field may fan out to multiple base table rows.
/// Cost = n_affected_tables * fanout_cost * affected_rows
#[must_use]
pub fn estimate_update_cost(
    n_affected_tables: u32,
    affected_rows: f64,
    params: &DualityCostParams,
) -> f64 {
    f64::from(n_affected_tables) * params.update_fanout_cost * affected_rows
}

/// Choose the optimal access path for a duality view query.
///
/// Compares document fetch cost vs relational decomposition cost
/// and returns the cheaper option.
#[must_use]
pub fn choose_access_path(
    view: &DualityView,
    table_row_counts: &HashMap<String, f64>,
    selectivity: f64,
    n_fields_accessed: u32,
    n_predicates: u32,
    params: &DualityCostParams,
) -> AccessPathDecision {
    let total_fields = view.field_count() as u32;
    let root_rows = table_row_counts
        .get(&view.root_table)
        .copied()
        .unwrap_or(1000.0);

    let doc_cost = estimate_document_cost(
        root_rows,
        selectivity,
        n_fields_accessed,
        n_predicates,
        params,
    );

    let row_counts: Vec<f64> = view
        .referenced_tables()
        .iter()
        .map(|t| table_row_counts.get(t).copied().unwrap_or(1000.0))
        .collect();

    let n_joins = view.join_count() as u32;

    let rel_cost = estimate_relational_cost(
        &row_counts,
        selectivity,
        n_joins,
        n_fields_accessed,
        total_fields,
        params,
    );

    let path = if doc_cost <= rel_cost {
        AccessPath::Document
    } else {
        AccessPath::Relational
    };

    AccessPathDecision {
        path,
        document_cost: doc_cost,
        relational_cost: rel_cost,
        cost_ratio: if rel_cost > 0.0 {
            doc_cost / rel_cost
        } else {
            1.0
        },
    }
}

/// Result of access path selection with cost details.
#[derive(Debug, Clone)]
pub struct AccessPathDecision {
    /// Chosen access path.
    pub path: AccessPath,
    /// Estimated document path cost.
    pub document_cost: f64,
    /// Estimated relational path cost.
    pub relational_cost: f64,
    /// Ratio: document_cost / relational_cost.
    /// < 1.0 means document is cheaper.
    pub cost_ratio: f64,
}

// ------------------------------------------------------------------
// Partial assembly optimization
// ------------------------------------------------------------------

/// Determine which joins can be eliminated for partial field access.
///
/// When a query accesses only a subset of the duality view's fields,
/// joins to tables whose columns are not referenced can be skipped.
#[must_use]
pub fn eliminable_joins(
    view: &DualityView,
    accessed_fields: &[String],
) -> Vec<String> {
    let all_tables = view.referenced_tables();
    let mut needed_tables: Vec<String> =
        vec![view.root_table.clone()];

    for field_name in accessed_fields {
        for field in &view.fields {
            if field.json_path == *field_name {
                collect_tables(&field.mapping, &mut needed_tables);
            }
        }
    }

    all_tables
        .into_iter()
        .filter(|t| !needed_tables.contains(t))
        .collect()
}

/// Cost savings from eliminating unnecessary joins.
#[must_use]
pub fn join_elimination_savings(
    eliminated_tables: &[String],
    table_row_counts: &HashMap<String, f64>,
    params: &DualityCostParams,
) -> f64 {
    let mut savings = 0.0;
    for table in eliminated_tables {
        let rows = table_row_counts
            .get(table)
            .copied()
            .unwrap_or(1000.0);
        savings += params.relational_join_cost * rows.sqrt();
        savings += rows * params.relational_scan_cost_per_row * 0.1;
    }
    savings
}

// ------------------------------------------------------------------
// Predicate pushdown analysis
// ------------------------------------------------------------------

/// Where a predicate should be evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateTarget {
    /// Push to a specific base table column.
    RelationalColumn,
    /// Evaluate as a JSON path expression on the document.
    JsonPath,
    /// Cannot be pushed; evaluate after assembly.
    PostAssembly,
}

/// Decide where to evaluate a predicate on a duality view field.
///
/// Simple field predicates (equality, range) on scalar columns
/// are best pushed to the relational layer. Complex path
/// expressions or cross-field predicates stay in the JSON layer.
#[must_use]
pub fn predicate_target(
    view: &DualityView,
    field_path: &str,
    has_index_on_column: bool,
) -> PredicateTarget {
    for field in &view.fields {
        if field.json_path == field_path {
            return match &field.mapping {
                DualityFieldMapping::Column { .. } => {
                    if has_index_on_column {
                        PredicateTarget::RelationalColumn
                    } else {
                        PredicateTarget::RelationalColumn
                    }
                }
                DualityFieldMapping::Nested { .. }
                | DualityFieldMapping::Array { .. } => {
                    PredicateTarget::JsonPath
                }
            };
        }
    }
    PredicateTarget::PostAssembly
}

/// Estimate the selectivity benefit of pushing a predicate to
/// the relational layer vs evaluating it on JSON documents.
///
/// Relational pushdown with an index is typically 10-100x more
/// selective than a full document scan with JSON path evaluation.
#[must_use]
pub fn pushdown_selectivity_benefit(
    has_index: bool,
    base_selectivity: f64,
) -> f64 {
    if has_index {
        base_selectivity * 0.1
    } else {
        base_selectivity * 0.5
    }
}

// ------------------------------------------------------------------
// E-graph rewrite rules for duality view patterns
// ------------------------------------------------------------------

/// Return rewrite rules for Oracle JSON Relational Duality views.
///
/// These rules recognize duality view access patterns and rewrite
/// them for better plan quality:
///
/// 1. Push JSON field filters below joins in duality view queries
/// 2. Split compound JSON predicates for independent pushdown
/// 3. Merge adjacent JSON filters for batch evaluation
/// 4. Push JSON filters below projections
/// 5. Push JSON filters below aggregations
#[must_use]
pub fn duality_rewrite_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Rule 1: Push JSON field filter through inner join.
        //
        // Duality view queries assemble documents via joins. When a
        // filter on a JSON field applies to only one side of the
        // join, pushing it down reduces the join input size.
        rewrite!("duality-json-filter-through-join";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond (filter ?pred ?left) ?right)"
            if is_json_field_predicate(var("?pred"))
        ),
        // Rule 2: Split conjunctive JSON field filters.
        //
        // When multiple JSON field predicates are ANDed, splitting
        // them allows independent pushdown to different base tables.
        rewrite!("duality-split-json-filters";
            "(filter (and ?p1 ?p2) ?input)" =>
            "(filter ?p1 (filter ?p2 ?input))"
            if is_json_field_predicate(var("?p1"))
        ),
        // Rule 3: Merge adjacent JSON field filters.
        //
        // The inverse of rule 2: merging allows batch JSON path
        // evaluation when both predicates target the same document.
        rewrite!("duality-merge-json-filters";
            "(filter ?p1 (filter ?p2 ?input))" =>
            "(filter (and ?p1 ?p2) ?input)"
            if is_json_field_predicate(var("?p1"))
        ),
        // Rule 4: Push JSON filter below projection.
        //
        // Duality views often project a subset of fields. Pushing
        // the filter below the projection enables earlier filtering.
        rewrite!("duality-json-filter-below-project";
            "(filter ?pred (project ?cols ?input))" =>
            "(project ?cols (filter ?pred ?input))"
            if is_json_field_predicate(var("?pred"))
        ),
        // Rule 5: Push JSON filter below aggregate.
        //
        // When a duality view query groups by a JSON field, pushing
        // a filter on another JSON field below the aggregate reduces
        // the number of groups.
        rewrite!("duality-json-filter-below-aggregate";
            "(filter ?pred (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs (filter ?pred ?input))"
            if is_json_field_predicate(var("?pred"))
        ),
    ]
}

fn var(s: &str) -> Var {
    s.parse().unwrap_or_else(|_| panic!("bad var: {s}"))
}

/// Condition: check if a predicate involves JSON field access.
///
/// Duality view predicates use `json-access` nodes to extract
/// fields from the assembled JSON document. This condition checks
/// for the presence of `json-access` or `func` nodes that indicate
/// JSON field operations.
fn is_json_field_predicate(
    pred_var: Var,
) -> impl Fn(&mut egg::EGraph<RelLang, RelAnalysis>, Id, &Subst) -> bool
{
    move |egraph, _id, subst| {
        let pred_id = subst[pred_var];
        contains_json_pattern(egraph, pred_id, 3)
    }
}

/// Recursively check if an e-class contains JSON access patterns.
fn contains_json_pattern(
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
            RelLang::JsonAccess(_) | RelLang::Func(_) => {
                return true;
            }
            RelLang::Eq([l, r])
            | RelLang::Ne([l, r])
            | RelLang::Lt([l, r])
            | RelLang::Le([l, r])
            | RelLang::Gt([l, r])
            | RelLang::Ge([l, r]) => {
                if contains_json_pattern(egraph, *l, depth - 1)
                    || contains_json_pattern(
                        egraph, *r, depth - 1,
                    )
                {
                    return true;
                }
            }
            RelLang::And([l, r]) | RelLang::Or([l, r]) => {
                if contains_json_pattern(egraph, *l, depth - 1)
                    || contains_json_pattern(
                        egraph, *r, depth - 1,
                    )
                {
                    return true;
                }
            }
            RelLang::Not([inner]) => {
                if contains_json_pattern(
                    egraph, *inner, depth - 1,
                ) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

/// Errors specific to duality view optimization.
///
/// All errors are non-fatal: the optimizer falls back to treating
/// the query as a standard Oracle query.
#[derive(Debug, thiserror::Error)]
pub enum DualityError {
    /// Duality view definition could not be parsed.
    #[error(
        "failed to parse duality view {view}: {reason}; \
         falling back to standard optimization"
    )]
    ViewParseFailed {
        view: String,
        reason: String,
    },

    /// Field mapping not found in the duality view.
    #[error(
        "field {field} not found in duality view {view}; \
         skipping predicate pushdown"
    )]
    FieldNotFound {
        view: String,
        field: String,
    },

    /// Unsupported Oracle version for duality views.
    #[error(
        "Oracle version {version} does not support duality views; \
         minimum required: 23ai"
    )]
    UnsupportedVersion {
        version: String,
    },
}

// ------------------------------------------------------------------
// Cost factor for e-graph integration
// ------------------------------------------------------------------

/// Cost factor for a duality view document scan relative to a
/// full relational decomposition.
///
/// A document fetch on a well-indexed duality view is typically
/// 20-40% of the cost of full relational assembly.
#[must_use]
pub fn duality_document_scan_cost_factor() -> f64 {
    0.25
}

// ------------------------------------------------------------------
// Benchmark helpers
// ------------------------------------------------------------------

/// Compare document vs relational access for a workload pattern.
///
/// Returns (document_cost, relational_cost, speedup_ratio).
#[must_use]
pub fn benchmark_access_patterns(
    view: &DualityView,
    table_row_counts: &HashMap<String, f64>,
    selectivity: f64,
    n_fields: u32,
    n_predicates: u32,
    params: &DualityCostParams,
) -> (f64, f64, f64) {
    let decision = choose_access_path(
        view,
        table_row_counts,
        selectivity,
        n_fields,
        n_predicates,
        params,
    );

    let speedup = if decision.document_cost < decision.relational_cost
    {
        decision.relational_cost / decision.document_cost
    } else {
        decision.document_cost / decision.relational_cost
    };

    (decision.document_cost, decision.relational_cost, speedup)
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

    fn sample_orders_view() -> DualityView {
        DualityView {
            name: "orders_dv".to_string(),
            root_table: "orders".to_string(),
            fields: vec![
                DualityField {
                    json_path: "_id".to_string(),
                    mapping: DualityFieldMapping::Column {
                        table: "orders".to_string(),
                        column: "order_id".to_string(),
                    },
                },
                DualityField {
                    json_path: "status".to_string(),
                    mapping: DualityFieldMapping::Column {
                        table: "orders".to_string(),
                        column: "status".to_string(),
                    },
                },
                DualityField {
                    json_path: "customer".to_string(),
                    mapping: DualityFieldMapping::Nested {
                        table: "customers".to_string(),
                        join_condition:
                            "orders.customer_id = customers.id"
                                .to_string(),
                        fields: vec![
                            DualityField {
                                json_path: "name".to_string(),
                                mapping:
                                    DualityFieldMapping::Column {
                                        table: "customers"
                                            .to_string(),
                                        column: "name"
                                            .to_string(),
                                    },
                            },
                            DualityField {
                                json_path: "email".to_string(),
                                mapping:
                                    DualityFieldMapping::Column {
                                        table: "customers"
                                            .to_string(),
                                        column: "email"
                                            .to_string(),
                                    },
                            },
                        ],
                    },
                },
                DualityField {
                    json_path: "items".to_string(),
                    mapping: DualityFieldMapping::Array {
                        table: "order_items".to_string(),
                        join_condition:
                            "orders.order_id = order_items.order_id"
                                .to_string(),
                        fields: vec![
                            DualityField {
                                json_path: "product".to_string(),
                                mapping:
                                    DualityFieldMapping::Column {
                                        table: "order_items"
                                            .to_string(),
                                        column: "product_name"
                                            .to_string(),
                                    },
                            },
                            DualityField {
                                json_path: "qty".to_string(),
                                mapping:
                                    DualityFieldMapping::Column {
                                        table: "order_items"
                                            .to_string(),
                                        column: "quantity"
                                            .to_string(),
                                    },
                            },
                        ],
                    },
                },
            ],
            updatability: Updatability::full(),
        }
    }

    fn sample_row_counts() -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("orders".to_string(), 100_000.0);
        m.insert("customers".to_string(), 10_000.0);
        m.insert("order_items".to_string(), 500_000.0);
        m
    }

    // -- DualityView model tests --

    #[test]
    fn referenced_tables() {
        let view = sample_orders_view();
        let tables = view.referenced_tables();
        assert!(tables.contains(&"orders".to_string()));
        assert!(tables.contains(&"customers".to_string()));
        assert!(tables.contains(&"order_items".to_string()));
        assert_eq!(tables.len(), 3);
    }

    #[test]
    fn field_count() {
        let view = sample_orders_view();
        // _id + status + (name + email) + (product + qty) = 6
        assert_eq!(view.field_count(), 6);
    }

    #[test]
    fn join_count() {
        let view = sample_orders_view();
        // customers join + order_items join = 2
        assert_eq!(view.join_count(), 2);
    }

    #[test]
    fn fields_from_table() {
        let view = sample_orders_view();
        let order_fields = view.fields_from_table("orders");
        assert_eq!(order_fields.len(), 2);
        let customer_fields = view.fields_from_table("customers");
        assert_eq!(customer_fields.len(), 1);
    }

    // -- Access path selection tests --

    #[test]
    fn document_path_wins_for_full_document_fetch() {
        let view = sample_orders_view();
        let counts = sample_row_counts();
        let params = DualityCostParams::default();

        let decision = choose_access_path(
            &view,
            &counts,
            1.0,
            6,
            0,
            &params,
        );

        assert_eq!(
            decision.path,
            AccessPath::Document,
            "document path should win for full fetch \
             (doc={:.0}, rel={:.0})",
            decision.document_cost,
            decision.relational_cost
        );
    }

    #[test]
    fn relational_path_wins_for_selective_query() {
        let view = sample_orders_view();
        let counts = sample_row_counts();
        let params = DualityCostParams::default();

        let decision = choose_access_path(
            &view,
            &counts,
            0.001,
            2,
            1,
            &params,
        );

        assert_eq!(
            decision.path,
            AccessPath::Relational,
            "relational path should win for selective query \
             (doc={:.0}, rel={:.0})",
            decision.document_cost,
            decision.relational_cost
        );
    }

    #[test]
    fn document_cost_scales_with_rows() {
        let params = DualityCostParams::default();
        let small = estimate_document_cost(
            100.0, 1.0, 6, 1, &params,
        );
        let large = estimate_document_cost(
            100_000.0, 1.0, 6, 1, &params,
        );
        assert!(
            large > small * 900.0,
            "cost should scale roughly linearly: \
             small={small:.0}, large={large:.0}"
        );
    }

    #[test]
    fn relational_cost_scales_with_joins() {
        let params = DualityCostParams::default();
        let no_joins = estimate_relational_cost(
            &[1000.0],
            0.1,
            0,
            3,
            6,
            &params,
        );
        let with_joins = estimate_relational_cost(
            &[1000.0, 500.0, 2000.0],
            0.1,
            2,
            3,
            6,
            &params,
        );
        assert!(
            with_joins > no_joins,
            "joins should increase cost: \
             no_joins={no_joins:.1}, with_joins={with_joins:.1}"
        );
    }

    // -- Update cost tests --

    #[test]
    fn update_cost_increases_with_affected_tables() {
        let params = DualityCostParams::default();
        let single = estimate_update_cost(1, 100.0, &params);
        let multi = estimate_update_cost(3, 100.0, &params);
        assert!(
            multi > single * 2.5,
            "multi-table update should be more expensive"
        );
    }

    #[test]
    fn update_cost_increases_with_rows() {
        let params = DualityCostParams::default();
        let few = estimate_update_cost(2, 10.0, &params);
        let many = estimate_update_cost(2, 10_000.0, &params);
        assert!(
            many > few * 900.0,
            "more rows should increase update cost"
        );
    }

    // -- Partial assembly tests --

    #[test]
    fn eliminable_joins_for_root_fields_only() {
        let view = sample_orders_view();
        let eliminated = eliminable_joins(
            &view,
            &["_id".to_string(), "status".to_string()],
        );
        assert!(
            eliminated.contains(&"customers".to_string()),
            "customers table should be eliminable"
        );
        assert!(
            eliminated.contains(&"order_items".to_string()),
            "order_items table should be eliminable"
        );
    }

    #[test]
    fn no_eliminable_joins_for_full_access() {
        let view = sample_orders_view();
        let eliminated = eliminable_joins(
            &view,
            &[
                "_id".to_string(),
                "status".to_string(),
                "customer".to_string(),
                "items".to_string(),
            ],
        );
        assert!(
            eliminated.is_empty(),
            "no tables eliminable when all fields accessed"
        );
    }

    #[test]
    fn join_elimination_saves_cost() {
        let counts = sample_row_counts();
        let params = DualityCostParams::default();
        let eliminated = vec!["order_items".to_string()];
        let savings =
            join_elimination_savings(&eliminated, &counts, &params);
        assert!(
            savings > 0.0,
            "eliminating a join should save cost"
        );
    }

    // -- Predicate pushdown tests --

    #[test]
    fn scalar_field_pushes_to_relational() {
        let view = sample_orders_view();
        let target =
            predicate_target(&view, "status", true);
        assert_eq!(target, PredicateTarget::RelationalColumn);
    }

    #[test]
    fn nested_field_stays_as_json() {
        let view = sample_orders_view();
        let target =
            predicate_target(&view, "customer", false);
        assert_eq!(target, PredicateTarget::JsonPath);
    }

    #[test]
    fn unknown_field_is_post_assembly() {
        let view = sample_orders_view();
        let target =
            predicate_target(&view, "nonexistent", false);
        assert_eq!(target, PredicateTarget::PostAssembly);
    }

    #[test]
    fn pushdown_benefit_with_index() {
        let benefit =
            pushdown_selectivity_benefit(true, 0.01);
        assert!(
            benefit < 0.01,
            "indexed pushdown should improve selectivity"
        );
    }

    #[test]
    fn pushdown_benefit_without_index() {
        let benefit =
            pushdown_selectivity_benefit(false, 0.01);
        assert!(
            benefit < 0.01,
            "non-indexed pushdown should still help"
        );
        let indexed =
            pushdown_selectivity_benefit(true, 0.01);
        assert!(
            indexed < benefit,
            "indexed pushdown should be better"
        );
    }

    // -- E-graph rewrite rule tests --

    fn run_with_duality_rules(
        expr: &RelExpr,
    ) -> Runner<RelLang, RelAnalysis> {
        let rec =
            to_rec_expr(expr).expect("conversion should succeed");
        let rules = duality_rewrite_rules();
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules)
    }

    fn make_json_predicate(
        field: &str,
        value: &str,
    ) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::BinOp {
                op: BinOp::JsonAccess,
                left: Box::new(Expr::Column(
                    ColumnRef::new("document"),
                )),
                right: Box::new(Expr::Const(Const::String(
                    field.to_string(),
                ))),
            }),
            right: Box::new(Expr::Const(Const::String(
                value.to_string(),
            ))),
        }
    }

    #[test]
    fn json_filter_pushes_through_join() {
        let left = RelExpr::scan("orders");
        let right = RelExpr::scan("customers");
        let joined = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(left),
            right: Box::new(right),
        };
        let filtered = RelExpr::Filter {
            predicate: make_json_predicate(
                "status", "active",
            ),
            input: Box::new(joined),
        };

        let runner = run_with_duality_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 5,
            "e-graph should grow from JSON filter pushdown"
        );
    }

    #[test]
    fn conjunctive_json_filters_split() {
        let scan = RelExpr::scan("orders_dv");
        let pred = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(make_json_predicate(
                "status",
                "shipped",
            )),
            right: Box::new(make_json_predicate(
                "priority",
                "high",
            )),
        };
        let filtered = RelExpr::Filter {
            predicate: pred,
            input: Box::new(scan),
        };

        let runner = run_with_duality_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 5,
            "e-graph should grow from conjunctive filter split"
        );
    }

    #[test]
    fn json_filter_pushes_below_project() {
        use ra_core::algebra::ProjectionColumn;

        let scan = RelExpr::scan("orders_dv");
        let projected = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("document")),
                alias: None,
            }],
            input: Box::new(scan),
        };
        let filtered = RelExpr::Filter {
            predicate: make_json_predicate(
                "status", "active",
            ),
            input: Box::new(projected),
        };

        let runner = run_with_duality_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 4,
            "e-graph should grow from JSON filter below project"
        );
    }

    #[test]
    fn non_json_filter_not_rewritten() {
        let scan = RelExpr::scan("plain_table");
        let filtered = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(
                    ColumnRef::new("id"),
                )),
                right: Box::new(Expr::Const(Const::Int(42))),
            },
            input: Box::new(scan),
        };

        let rec = to_rec_expr(&filtered)
            .expect("conversion should succeed");
        let initial_classes = {
            let mut eg = egg::EGraph::<
                RelLang,
                RelAnalysis,
            >::default();
            eg.add_expr(&rec);
            eg.number_of_classes()
        };

        let runner = run_with_duality_rules(&filtered);
        assert_eq!(
            runner.egraph.number_of_classes(),
            initial_classes,
            "plain SQL filter should not trigger duality rules"
        );
    }

    #[test]
    fn json_filter_below_aggregate() {
        use ra_core::algebra::{
            AggregateExpr, AggregateFunction,
        };

        let scan = RelExpr::scan("orders_dv");
        let agg = RelExpr::Aggregate {
            group_by: vec![Expr::Column(
                ColumnRef::new("region"),
            )],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".to_string()),
            }],
            input: Box::new(scan),
        };
        let filtered = RelExpr::Filter {
            predicate: make_json_predicate(
                "status", "active",
            ),
            input: Box::new(agg),
        };

        let runner = run_with_duality_rules(&filtered);
        assert!(
            runner.egraph.number_of_classes() > 6,
            "JSON filter should push below aggregate"
        );
    }

    // -- Error type tests --

    #[test]
    fn error_messages_are_actionable() {
        let err = DualityError::ViewParseFailed {
            view: "orders_dv".to_string(),
            reason: "missing root table".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("orders_dv"));
        assert!(msg.contains("falling back"));

        let err = DualityError::FieldNotFound {
            view: "orders_dv".to_string(),
            field: "unknown_field".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("unknown_field"));
        assert!(msg.contains("skipping"));

        let err = DualityError::UnsupportedVersion {
            version: "21c".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("21c"));
        assert!(msg.contains("23ai"));
    }

    // -- Cost factor test --

    #[test]
    fn duality_cost_factor_is_reasonable() {
        let factor = duality_document_scan_cost_factor();
        assert!(
            factor > 0.0 && factor < 1.0,
            "duality document scan should be cheaper than \
             full relational assembly"
        );
    }

    // -- Benchmark helper test --

    #[test]
    fn benchmark_returns_positive_speedup() {
        let view = sample_orders_view();
        let counts = sample_row_counts();
        let params = DualityCostParams::default();
        let (doc, rel, speedup) = benchmark_access_patterns(
            &view, &counts, 0.01, 3, 1, &params,
        );
        assert!(doc > 0.0, "document cost should be positive");
        assert!(rel > 0.0, "relational cost should be positive");
        assert!(
            speedup >= 1.0,
            "speedup should be >= 1.0: {speedup:.2}"
        );
    }

    // -- Updatability tests --

    #[test]
    fn read_only_view() {
        let up = Updatability::read_only();
        assert!(!up.insert);
        assert!(!up.update);
        assert!(!up.delete);
    }

    #[test]
    fn full_updatability() {
        let up = Updatability::full();
        assert!(up.insert);
        assert!(up.update);
        assert!(up.delete);
    }

    // -- Single-table duality view (edge case) --

    #[test]
    fn single_table_view_has_no_joins() {
        let view = DualityView {
            name: "simple_dv".to_string(),
            root_table: "users".to_string(),
            fields: vec![
                DualityField {
                    json_path: "_id".to_string(),
                    mapping: DualityFieldMapping::Column {
                        table: "users".to_string(),
                        column: "id".to_string(),
                    },
                },
                DualityField {
                    json_path: "name".to_string(),
                    mapping: DualityFieldMapping::Column {
                        table: "users".to_string(),
                        column: "name".to_string(),
                    },
                },
            ],
            updatability: Updatability::full(),
        };

        assert_eq!(view.join_count(), 0);
        assert_eq!(view.field_count(), 2);
        assert_eq!(view.referenced_tables().len(), 1);
    }

    // -- Cost param sensitivity --

    #[test]
    fn high_doc_cost_favors_relational() {
        let view = sample_orders_view();
        let counts = sample_row_counts();
        let mut params = DualityCostParams::default();
        params.document_fetch_cost = 50.0;

        let decision = choose_access_path(
            &view,
            &counts,
            0.01,
            3,
            1,
            &params,
        );

        assert_eq!(
            decision.path,
            AccessPath::Relational,
            "high document fetch cost should favor relational"
        );
    }

    #[test]
    fn high_join_cost_favors_document() {
        let view = sample_orders_view();
        let counts = sample_row_counts();
        let mut params = DualityCostParams::default();
        params.relational_join_cost = 100.0;

        let decision = choose_access_path(
            &view,
            &counts,
            1.0,
            6,
            0,
            &params,
        );

        assert_eq!(
            decision.path,
            AccessPath::Document,
            "high join cost should favor document path"
        );
    }

    // -- Eliminable joins with nested field access --

    #[test]
    fn eliminable_joins_keeps_needed_nested_table() {
        let view = sample_orders_view();
        let eliminated = eliminable_joins(
            &view,
            &[
                "_id".to_string(),
                "customer".to_string(),
            ],
        );
        // Customers table is needed for "customer" field
        assert!(
            !eliminated.contains(&"customers".to_string()),
            "customers should not be eliminated"
        );
        // But order_items is not needed
        assert!(
            eliminated.contains(&"order_items".to_string()),
            "order_items should be eliminable"
        );
    }

    // -- Eliminable joins with only items accessed --

    #[test]
    fn eliminable_joins_keeps_array_table() {
        let view = sample_orders_view();
        let eliminated = eliminable_joins(
            &view,
            &["_id".to_string(), "items".to_string()],
        );
        assert!(
            !eliminated.contains(&"order_items".to_string()),
            "order_items should not be eliminated"
        );
        assert!(
            eliminated.contains(&"customers".to_string()),
            "customers should be eliminable"
        );
    }

    // -- Join elimination savings for empty list --

    #[test]
    fn join_elimination_savings_zero_for_empty() {
        let counts = sample_row_counts();
        let params = DualityCostParams::default();
        let savings =
            join_elimination_savings(&[], &counts, &params);
        assert!(
            savings.abs() < f64::EPSILON,
            "no eliminated tables should mean zero savings"
        );
    }

    // -- Predicate target on array field --

    #[test]
    fn array_field_pushes_to_json() {
        let view = sample_orders_view();
        let target =
            predicate_target(&view, "items", false);
        assert_eq!(target, PredicateTarget::JsonPath);
    }

    // -- Cost ratio < 1.0 means document is cheaper --

    #[test]
    fn cost_ratio_below_one_means_document_wins() {
        let view = sample_orders_view();
        let counts = sample_row_counts();
        let params = DualityCostParams::default();

        let decision = choose_access_path(
            &view, &counts, 1.0, 6, 0, &params,
        );

        if decision.path == AccessPath::Document {
            assert!(
                decision.cost_ratio <= 1.0,
                "cost_ratio should be <= 1.0 when document wins"
            );
        } else {
            assert!(
                decision.cost_ratio > 1.0,
                "cost_ratio should be > 1.0 when relational wins"
            );
        }
    }

    // -- Zero relational cost edge case --

    #[test]
    fn access_path_with_empty_row_counts() {
        let view = sample_orders_view();
        let counts = HashMap::new();
        let params = DualityCostParams::default();

        let decision = choose_access_path(
            &view, &counts, 0.5, 3, 1, &params,
        );
        // Should not panic; uses default of 1000.0
        assert!(decision.document_cost > 0.0);
        assert!(decision.relational_cost > 0.0);
    }

    // -- Document cost zero predicates --

    #[test]
    fn document_cost_zero_predicates_no_scan() {
        let params = DualityCostParams::default();
        let cost = estimate_document_cost(
            100_000.0, 1.0, 6, 0, &params,
        );
        // No predicate scan cost, only fetch + extract
        let expected_per_row = params.document_fetch_cost
            + 6.0 * params.json_field_extract_cost;
        assert!(
            (cost - 100_000.0 * expected_per_row).abs()
                < 1.0,
            "cost without predicates should be \
             rows * (fetch + extract)"
        );
    }

    // -- Relational cost with empty table row counts --

    #[test]
    fn relational_cost_empty_tables_defaults() {
        let params = DualityCostParams::default();
        let cost = estimate_relational_cost(
            &[], 0.5, 0, 3, 6, &params,
        );
        // Empty table list -> no scan cost, but assembly
        // uses default 1000 for root
        assert!(
            cost > 0.0,
            "cost should be positive even with empty tables"
        );
    }

    // -- DualityCostParams default values test --

    #[test]
    fn duality_cost_params_default_values() {
        let params = DualityCostParams::default();
        assert!((params.document_fetch_cost - 5.0).abs() < f64::EPSILON);
        assert!((params.json_field_extract_cost - 0.2).abs() < f64::EPSILON);
        assert!((params.json_predicate_cost - 0.8).abs() < f64::EPSILON);
        assert!((params.relational_join_cost - 3.0).abs() < f64::EPSILON);
        assert!((params.partial_assembly_cost - 1.5).abs() < f64::EPSILON);
        assert!((params.update_fanout_cost - 2.0).abs() < f64::EPSILON);
        assert!((params.relational_scan_cost_per_row - 1.0).abs() < f64::EPSILON);
    }
}
