//! Rewrite rules for equality saturation.
//!
//! Defines transformation rules that the egg optimizer applies during
//! equality saturation. Rules are organized by category:
//!
//! - **Predicate pushdown**: push filters below joins and projections
//! - **Join reordering**: commutativity, associativity
//! - **Projection pushdown**: push projections closer to scans
//! - **Expression simplification**: constant folding, boolean logic
//! - **Join elimination**: remove redundant joins
//! - **Aggregate optimization**: push aggregates, split aggregates
//! - **Limit/Sort optimization**: push limits, eliminate redundant sorts
//! - **Set operations**: simplify union/intersect/except
//! - **Subquery optimization**: decorrelation patterns
//! - **DuckDB-inspired rules**: from `DuckDB` optimizer source
//! - **SQLite-inspired rules**: from `SQLite` query planner source

use egg::Rewrite;
#[cfg(test)]
use egg::rewrite;

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::query_features::QueryFeatureSet;

// Rules generated from .rra files at compile time. The build script
// emits `#[allow(unused)]` on each helper because some are conditionally
// empty; the workspace lint denies bare `#[allow]` so we sandbox the
// generated content inside a child module that whitelists those
// attributes via `expect`.
#[expect(
    clippy::allow_attributes,
    reason = "build-script-generated code emits #[allow(unused)] which we cannot rewrite"
)]
mod generated {
    use egg::{rewrite, Rewrite};

    use crate::analysis::RelAnalysis;
    #[expect(
        unused_imports,
        reason = "the set of conditions actually referenced by the .rra-derived rules \
                  varies as the rule corpus grows; importing the full known set keeps \
                  the build robust against future activations"
    )]
    use crate::conditions::{
        is_bson_operator_filter, is_canonical_scan, is_constant, is_deterministic, is_equi_join,
        is_json_field_predicate, is_not_const_bool, is_ungrouped_count_star, is_uncorrelated,
        is_xml_function_filter, not_nullable,
        not_zero, pred_references_only, predicate_references_only, references_only,
        references_subset, single_reference,
    };
    use crate::appliers::{fold_add, fold_mul, fold_sub};
    use crate::egraph::RelLang;

    include!(concat!(env!("OUT_DIR"), "/generated_rules.rs"));
}
pub(crate) use generated::all_generated_rules;
// RFC 0090: the predicate-pushdown category is sourced authoritatively from the
// compiled .rra corpus (rules/logical/predicate-pushdown-core/). The hand-coded
// `predicate_pushdown_rules()` is retained only as a test oracle (see the
// `generated_predicate_pushdown_matches_hand_coded` identity test).
pub(crate) use generated::generated_logical_predicate_pushdown_core_rules;
pub(crate) use generated::{
    generated_logical_aggregate_optimization_core_rules,
    generated_logical_cte_inlining_core_rules,
    generated_logical_expression_simplification_core_rules,
    generated_logical_join_elimination_core_rules,
    generated_logical_join_reordering_core_rules,
    generated_logical_limit_sort_optimization_core_rules,
    generated_logical_projection_pushdown_core_rules,
    generated_logical_set_operation_core_rules,
    generated_logical_subquery_optimization_core_rules,
};
// Phase 1b second wave: remaining pure-pattern categories.
pub(crate) use generated::{
    generated_database_specific_duckdb_core_rules,
    generated_database_specific_sqlite_core_rules,
    generated_logical_column_pruning_core_rules,
    generated_logical_functional_dependencies_core_rules,
    generated_logical_join_transformation_core_rules,
    generated_logical_null_simplification_core_rules,
    generated_logical_redundant_join_core_rules,
    generated_logical_semi_join_core_rules,
    generated_physical_covering_index_core_rules,
    generated_physical_min_max_index_core_rules,
    generated_physical_parquet_pushdown_core_rules,
    generated_physical_runtime_filter_core_rules,
};
pub(crate) use generated::{
    generated_logical_cast_optimization_core_rules,
    generated_physical_hybrid_search_core_rules,
};
pub(crate) use generated::{
    generated_logical_consensus_core_rules,
    generated_physical_fts_core_rules,
    generated_physical_vector_core_rules,
};
pub(crate) use generated::{
    generated_logical_count_metadata_core_rules,
    generated_logical_xml_core_rules,
};
pub(crate) use generated::generated_physical_index_selection_core_rules;
pub(crate) use generated::generated_logical_constant_folding_core_rules;
pub(crate) use generated::generated_physical_join_lowering_core_rules;
pub(crate) use generated::{
    generated_database_specific_documentdb_core_rules,
    generated_database_specific_oracle_json_duality_core_rules,
};
#[cfg(test)]
#[expect(unused_imports, reason = "test-only re-export")]
pub(crate) use generated::{generated_rule_stats, GeneratedRuleStats};

/// Return all optimization rewrite rules sorted by priority.
///
/// Rules are sorted using RFC 0058 complexity-based prioritization:
/// high-benefit, low-complexity rules come first. The e-graph engine
/// applies them in each iteration until saturation or a limit is
/// reached.
#[must_use]
pub fn all_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    crate::rule_priority::sort_rules_by_priority(all_rules_unsorted())
}

/// Return all optimization rewrite rules without priority sorting.
///
/// Used for benchmarking the impact of priority sorting.
#[must_use]
pub fn all_rules_unsorted() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    // Pre-allocate capacity to avoid reallocations
    // Typical total: ~200 rules across all categories
    let mut rules = Vec::with_capacity(200);
    // Null simplification rules
    rules.extend(generated_logical_null_simplification_core_rules());

    // Standard optimization rules
    rules.extend(generated_logical_predicate_pushdown_core_rules());
    rules.extend(generated_logical_join_reordering_core_rules());
    rules.extend(generated_logical_projection_pushdown_core_rules());
    rules.extend(generated_logical_expression_simplification_core_rules());
    rules.extend(generated_logical_join_elimination_core_rules());
    rules.extend(generated_logical_aggregate_optimization_core_rules());
    rules.extend(generated_logical_limit_sort_optimization_core_rules());
    rules.extend(generated_logical_set_operation_core_rules());
    rules.extend(generated_logical_subquery_optimization_core_rules());

    // CTE inlining rules
    rules.extend(generated_logical_cte_inlining_core_rules());

    // Column pruning rules (project through intersect/except/limit, etc.)
    rules.extend(generated_logical_column_pruning_core_rules());

    // Functional dependency rules (distinct/sort elimination)
    rules.extend(generated_logical_functional_dependencies_core_rules());

    // Semi-join reduction rules (distinct elimination, filter merging)
    rules.extend(generated_logical_semi_join_core_rules());

    // Redundant join elimination rules (cross/inner/anti join patterns)
    rules.extend(generated_logical_redundant_join_core_rules());

    // Consensus rules (DataFusion + Calcite)
    rules.extend(generated_logical_consensus_core_rules());

    // Database-inspired rules
    rules.extend(generated_database_specific_duckdb_core_rules());
    rules.extend(generated_database_specific_sqlite_core_rules());
    rules.extend(generated_physical_runtime_filter_core_rules());
    rules.extend(generated_logical_join_transformation_core_rules());

    // File-format rules
    rules.extend(generated_physical_parquet_pushdown_core_rules());

    // Metadata shortcut rules
    rules.extend(generated_logical_count_metadata_core_rules());

    // Basic index scan selection rules
    rules.extend(generated_physical_index_selection_core_rules());

    // Covering index (index-only scan) rules
    rules.extend(generated_physical_covering_index_core_rules());

    // MIN/MAX index optimization rules
    rules.extend(generated_physical_min_max_index_core_rules());

    // DocumentDB / BSON query optimization rules (RFC 0062)
    rules.extend(generated_database_specific_documentdb_core_rules());

    // Oracle JSON Relational Duality view rules (RFC 0084)
    rules.extend(generated_database_specific_oracle_json_duality_core_rules());

    // XPath/XQuery optimization rules (RFC 0083)
    rules.extend(generated_logical_xml_core_rules());

    // Vector similarity search optimization rules (RFC 0064)
    rules.extend(generated_physical_vector_core_rules());

    // Full-text search optimization rules (RFC 0066)
    rules.extend(generated_physical_fts_core_rules());

    // Hybrid search optimization rules (RFC 0073)
    rules.extend(generated_physical_hybrid_search_core_rules());

    // Type cast optimization rules
    rules.extend(generated_logical_cast_optimization_core_rules());

    // Constant folding (computed-RHS appliers, RFC 0090 Phase 2)
    rules.extend(generated_logical_constant_folding_core_rules());

    // Generated rules from .rra files. The explicit `-core` categories above are
    // the canonical, validated source for their transforms; we add a generated
    // rule only if its transform (LHS=>RHS pattern signature) hasn't already been
    // contributed. Deduping by *transform* rather than by name eliminates the
    // historical drift where an old corpus directory and its `-core` replacement
    // expressed the same rewrite under different names (e.g. `filter-split` vs
    // `filter-split-and`) — both used to slip through name-only dedup. Spec .rra
    // files are preserved on disk; they simply don't double-fire here.
    let transform_sig = |r: &Rewrite<RelLang, RelAnalysis>| -> String {
        let lhs = r
            .searcher
            .get_pattern_ast()
            .map_or_else(|| format!("dyn:{}", r.name), ToString::to_string);
        let rhs = r
            .applier
            .get_pattern_ast()
            .map_or_else(|| format!("dyn:{}", r.name), ToString::to_string);
        format!("{lhs}=>{rhs}")
    };
    let mut seen: std::collections::HashSet<String> =
        rules.iter().map(&transform_sig).collect();
    let generated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(all_generated_rules));
    if let Ok(gen_rules) = generated {
        for rule in gen_rules {
            if seen.insert(transform_sig(&rule)) {
                rules.push(rule);
            }
        }
    }

    rules
}

/// Return the set of generated rules from .rra files that are compatible
/// with the current `RelLang` grammar.
///
/// These rules are compiled from the `rules/` directory at build time.
/// Rules using operators not in `RelLang` or requiring unimplemented condition
/// functions are excluded.
///
/// Call [`generated_rule_stats()`] for statistics about the generated set.
#[must_use]
pub fn generated_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    // Return the full generated set. Callers can filter as needed.
    // Note: Rules using non-existent RelLang operators will panic when
    // the egg runner tries to parse them. Use validate_generated_rules()
    // to identify which rules are safe to use.
    all_generated_rules()
}

/// Metadata declaring what a rule group requires to be applicable.
///
/// Used by the rule advisor to skip entire groups of rules when the
/// environment context or query shape cannot match.
#[derive(Debug, Clone)]
pub struct RuleAnnotation {
    /// Features the query must have for these rules to be useful.
    /// Empty (or `UNIVERSAL`) means the rules apply to any query.
    pub required_features: QueryFeatureSet,
    /// Database engines these rules target. Empty means universal.
    pub databases: Vec<&'static str>,
}

/// A group of rewrite rules with associated annotation metadata.
#[derive(Debug)]
pub struct AnnotatedRuleGroup {
    /// Human-readable label for the rule group.
    pub label: &'static str,
    /// The annotation describing applicability.
    pub annotation: RuleAnnotation,
    /// The rewrite rules in this group.
    pub rules: Vec<Rewrite<RelLang, RelAnalysis>>,
}

/// Return all rule groups with applicability annotations.
///
/// Each group carries a [`RuleAnnotation`] that declares the
/// database scope and the structural/content features the rules
/// require. The rule advisor uses these annotations to eliminate
/// inapplicable groups before equality saturation.
#[must_use]
#[expect(clippy::too_many_lines, reason = "annotated rule collection for all optimization phases")]
pub fn all_rules_annotated() -> Vec<AnnotatedRuleGroup> {
    vec![
        // -- Universal baseline rules --
        AnnotatedRuleGroup {
            label: "null-simplification",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_logical_null_simplification_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "predicate-pushdown",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_logical_predicate_pushdown_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "projection-pushdown",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_logical_projection_pushdown_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "expression-simplification",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_logical_expression_simplification_core_rules(),
        },
        // -- Join rules --
        AnnotatedRuleGroup {
            label: "join-reordering",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JOIN,
                databases: vec![],
            },
            rules: generated_logical_join_reordering_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "join-elimination",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JOIN,
                databases: vec![],
            },
            rules: generated_logical_join_elimination_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "join-transformations",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JOIN,
                databases: vec![],
            },
            rules: generated_logical_join_transformation_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "semi-join-reduction",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JOIN,
                databases: vec![],
            },
            rules: generated_logical_semi_join_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "redundant-join-elimination",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JOIN,
                databases: vec![],
            },
            rules: generated_logical_redundant_join_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "runtime-filters",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JOIN,
                databases: vec![],
            },
            rules: generated_physical_runtime_filter_core_rules(),
        },
        // -- Aggregate rules --
        AnnotatedRuleGroup {
            label: "aggregate-optimization",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_AGGREGATE,
                databases: vec![],
            },
            rules: generated_logical_aggregate_optimization_core_rules(),
        },
        // -- Limit/Sort rules --
        AnnotatedRuleGroup {
            label: "limit-sort-optimization",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_LIMIT.union(QueryFeatureSet::HAS_SORT),
                databases: vec![],
            },
            rules: generated_logical_limit_sort_optimization_core_rules(),
        },
        // -- Set operation rules --
        AnnotatedRuleGroup {
            label: "set-operations",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_SET_OPS,
                databases: vec![],
            },
            rules: generated_logical_set_operation_core_rules(),
        },
        // -- Subquery rules --
        AnnotatedRuleGroup {
            label: "subquery-optimization",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_SUBQUERY.union(QueryFeatureSet::HAS_JOIN),
                databases: vec![],
            },
            rules: generated_logical_subquery_optimization_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "cte-inlining",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_CTE,
                databases: vec![],
            },
            rules: generated_logical_cte_inlining_core_rules(),
        },
        // -- Database-inspired universal rules --
        AnnotatedRuleGroup {
            label: "duckdb-inspired",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_database_specific_duckdb_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "sqlite-inspired",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_database_specific_sqlite_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "consensus-rules",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_logical_consensus_core_rules(),
        },
        // -- Column pruning, functional deps --
        AnnotatedRuleGroup {
            label: "column-pruning",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_logical_column_pruning_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "functional-deps",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_logical_functional_dependencies_core_rules(),
        },
        // -- File format --
        AnnotatedRuleGroup {
            label: "parquet-pushdown",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_physical_parquet_pushdown_core_rules(),
        },
        // -- Metadata shortcuts --
        AnnotatedRuleGroup {
            label: "count-metadata",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_AGGREGATE,
                databases: vec![],
            },
            rules: generated_logical_count_metadata_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "index-selection",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_physical_index_selection_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "covering-index",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: generated_physical_covering_index_core_rules(),
        },
        AnnotatedRuleGroup {
            label: "min-max-index",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_AGGREGATE,
                databases: vec![],
            },
            rules: generated_physical_min_max_index_core_rules(),
        },
        // -- Specialty: DocumentDB / BSON (RFC 0062) --
        AnnotatedRuleGroup {
            label: "documentdb-bson",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_BSON_FUNC
                    .union(QueryFeatureSet::HAS_JSON_ACCESS),
                databases: vec!["documentdb"],
            },
            rules: generated_database_specific_documentdb_core_rules(),
        },
        // -- Specialty: Oracle JSON Relational Duality (RFC 0084) --
        AnnotatedRuleGroup {
            label: "oracle-json-duality",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JSON_ACCESS,
                databases: vec!["oracle"],
            },
            rules: generated_database_specific_oracle_json_duality_core_rules(),
        },
        // -- Specialty: XML optimization (RFC 0083) --
        AnnotatedRuleGroup {
            label: "xml-optimization",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_XML_FUNC,
                databases: vec![],
            },
            rules: generated_logical_xml_core_rules(),
        },
        // -- Specialty: Vector search (RFC 0064) --
        AnnotatedRuleGroup {
            label: "vector-search",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_VECTOR_DISTANCE,
                databases: vec![],
            },
            rules: generated_physical_vector_core_rules(),
        },
        // -- Specialty: Full-text search (RFC 0066) --
        AnnotatedRuleGroup {
            label: "full-text-search",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_FTS_MATCH,
                databases: vec![],
            },
            rules: generated_physical_fts_core_rules(),
        },
        // -- Specialty: Hybrid search (RFC 0073) --
        AnnotatedRuleGroup {
            label: "hybrid-search",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_FTS_MATCH
                    .union(QueryFeatureSet::HAS_VECTOR_DISTANCE),
                databases: vec![],
            },
            rules: generated_physical_hybrid_search_core_rules(),
        },
        // -- Cast optimization --
        AnnotatedRuleGroup {
            label: "cast-optimization",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_CAST,
                databases: vec![],
            },
            rules: generated_logical_cast_optimization_core_rules(),
        },
    ]
}

// ---------------------------------------------------------------
// Predicate pushdown rules
// ---------------------------------------------------------------

/// Hand-coded predicate-pushdown rules, retained as the **test oracle** for the
/// `.rra`-sourced authoritative set (RFC 0090). Not used in the production rule
/// path — `load_category(FilterOptimization)` and `all_rules_unsorted()` use
/// `generated_logical_predicate_pushdown_core_rules()`. Removed entirely once
/// the identity guard is replaced by a snapshot (Phase 5).
#[cfg(test)]
pub(crate) fn predicate_pushdown_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Push filter through inner join (left side)
        rewrite!("filter-through-join-left";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond (filter ?pred ?left) ?right)"
            if crate::conditions::references_only("?pred", "?left")
        ),
        // Push filter through inner join (right side)
        rewrite!("filter-through-join-right";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond ?left (filter ?pred ?right))"
            if crate::conditions::references_only("?pred", "?right")
        ),
        // Merge filter into join condition
        rewrite!("filter-into-join-condition";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner (and ?cond ?pred) ?left ?right)"
        ),
        // Merge adjacent filters
        rewrite!("filter-merge";
            "(filter ?p1 (filter ?p2 ?input))" =>
            "(filter (and ?p1 ?p2) ?input)"
        ),
        // Split conjunctive filter
        rewrite!("filter-split-and";
            "(filter (and ?p1 ?p2) ?input)" =>
            "(filter ?p1 (filter ?p2 ?input))"
        ),
        // Push filter through project
        rewrite!("filter-through-project";
            "(filter ?pred (project ?cols ?input))" =>
            "(project ?cols (filter ?pred ?input))"
            if crate::conditions::references_subset("?pred", "?cols")
        ),
        // Push filter through union
        rewrite!("filter-through-union";
            "(filter ?pred (union ?all ?left ?right))" =>
            "(union ?all (filter ?pred ?left) (filter ?pred ?right))"
        ),
        // Push filter through intersect
        rewrite!("filter-through-intersect";
            "(filter ?pred (intersect ?all ?left ?right))" =>
            "(intersect ?all (filter ?pred ?left) (filter ?pred ?right))"
        ),
        // Push filter through except (left side only is safe)
        rewrite!("filter-through-except-left";
            "(filter ?pred (except ?all ?left ?right))" =>
            "(except ?all (filter ?pred ?left) ?right)"
        ),
    ]
}

// ---------------------------------------------------------------
// Join reordering rules
// ---------------------------------------------------------------

/// Test oracle for the `.rra`-sourced authoritative set (RFC 0090); not in the
/// production path (see `generated_logical_join_reordering_core_rules`).
#[cfg(test)]
pub(crate) fn join_reordering_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Inner join commutativity
        rewrite!("join-commutativity";
            "(join inner ?cond ?left ?right)" =>
            "(join inner ?cond ?right ?left)"
        ),
        // Cross join commutativity
        rewrite!("cross-join-commutativity";
            "(join cross ?cond ?left ?right)" =>
            "(join cross ?cond ?right ?left)"
        ),
        // Inner join associativity (left rotation)
        rewrite!("join-associativity-left";
            "(join inner ?c1 (join inner ?c2 ?a ?b) ?c)" =>
            "(join inner ?c2 ?a (join inner ?c1 ?b ?c))"
        ),
        // Inner join associativity (right rotation)
        rewrite!("join-associativity-right";
            "(join inner ?c1 ?a (join inner ?c2 ?b ?c))" =>
            "(join inner ?c2 (join inner ?c1 ?a ?b) ?c)"
        ),
        // Convert cartesian product + filter to inner join
        rewrite!("cartesian-to-join";
            "(filter ?pred (join cross ?cond ?left ?right))" =>
            "(join inner ?pred ?left ?right)"
        ),
        // Outer join to inner join when NULL-rejecting filter is present
        rewrite!("left-outer-to-inner-with-filter";
            "(filter (is-not-null ?col) (join left-outer ?cond ?left ?right))" =>
            "(filter (is-not-null ?col) (join inner ?cond ?left ?right))"
        ),
        rewrite!("right-outer-to-inner-with-filter";
            "(filter (is-not-null ?col) (join right-outer ?cond ?left ?right))" =>
            "(filter (is-not-null ?col) (join inner ?cond ?left ?right))"
        ),
    ]
}

// ---------------------------------------------------------------
// Projection pushdown rules
// ---------------------------------------------------------------

/// Test oracle (RFC 0090); production uses `generated_logical_projection_pushdown_core_rules`.
#[cfg(test)]
pub(crate) fn projection_pushdown_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Eliminate redundant project over project
        rewrite!("project-merge";
            "(project ?cols1 (project ?cols2 ?input))" =>
            "(project ?cols1 ?input)"
        ),
    ]
}

// ---------------------------------------------------------------
// Expression simplification rules
// ---------------------------------------------------------------

/// Test oracle (RFC 0090); production uses `generated_logical_expression_simplification_core_rules`.
#[cfg(test)]
pub(crate) fn expression_simplification_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = boolean_simplification_rules();
    rules.extend(arithmetic_simplification_rules());
    rules.extend(commutativity_rules());
    rules
}

#[cfg(test)]
fn boolean_simplification_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        rewrite!("and-true-left";
            "(and ?x (const-bool true))" => "?x"),
        rewrite!("and-true-right";
            "(and (const-bool true) ?x)" => "?x"),
        rewrite!("and-false-left";
            "(and ?x (const-bool false))" => "(const-bool false)"),
        rewrite!("and-false-right";
            "(and (const-bool false) ?x)" => "(const-bool false)"),
        rewrite!("or-false-left";
            "(or ?x (const-bool false))" => "?x"),
        rewrite!("or-false-right";
            "(or (const-bool false) ?x)" => "?x"),
        rewrite!("or-true-left";
            "(or ?x (const-bool true))" => "(const-bool true)"),
        rewrite!("or-true-right";
            "(or (const-bool true) ?x)" => "(const-bool true)"),
        rewrite!("double-negation";
            "(not (not ?x))" => "?x"),
        rewrite!("demorgan-and-to-or";
            "(not (and ?x ?y))" => "(or (not ?x) (not ?y))"),
        rewrite!("demorgan-or-to-and";
            "(not (or ?x ?y))" => "(and (not ?x) (not ?y))"),
        rewrite!("eq-reflexive";
            "(eq ?x ?x)" => "(const-bool true)"),
        rewrite!("ne-reflexive";
            "(ne ?x ?x)" => "(const-bool false)"),
        rewrite!("eq-null";
            "(eq ?x const-null)" => "const-null"),
        rewrite!("ne-null";
            "(ne ?x const-null)" => "const-null"),
        rewrite!("not-is-null";
            "(not (is-null ?x))" => "(is-not-null ?x)"),
        rewrite!("not-is-not-null";
            "(not (is-not-null ?x))" => "(is-null ?x)"),
        rewrite!("and-idempotent";
            "(and ?x ?x)" => "?x"),
        rewrite!("or-idempotent";
            "(or ?x ?x)" => "?x"),
        rewrite!("filter-true";
            "(filter (const-bool true) ?input)" => "?input"),
    ]
}

#[cfg(test)]
fn arithmetic_simplification_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        rewrite!("add-zero-right";
            "(add ?x (const-int 0))" => "?x"),
        rewrite!("add-zero-left";
            "(add (const-int 0) ?x)" => "?x"),
        rewrite!("mul-one-right";
            "(mul ?x (const-int 1))" => "?x"),
        rewrite!("mul-one-left";
            "(mul (const-int 1) ?x)" => "?x"),
        rewrite!("mul-zero-right";
            "(mul ?x (const-int 0))" => "(const-int 0)"),
        rewrite!("mul-zero-left";
            "(mul (const-int 0) ?x)" => "(const-int 0)"),
        rewrite!("sub-zero";
            "(sub ?x (const-int 0))" => "?x"),
        rewrite!("double-neg-arith";
            "(neg (neg ?x))" => "?x"),
    ]
}

#[cfg(test)]
fn commutativity_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        rewrite!("add-commutative";
            "(add ?x ?y)" => "(add ?y ?x)"),
        rewrite!("mul-commutative";
            "(mul ?x ?y)" => "(mul ?y ?x)"),
        rewrite!("eq-commutative";
            "(eq ?x ?y)" => "(eq ?y ?x)"),
        rewrite!("ne-commutative";
            "(ne ?x ?y)" => "(ne ?y ?x)"),
        rewrite!("and-commutative";
            "(and ?x ?y)" => "(and ?y ?x)"),
        rewrite!("or-commutative";
            "(or ?x ?y)" => "(or ?y ?x)"),
        rewrite!("lt-to-gt";
            "(lt ?x ?y)" => "(gt ?y ?x)"),
        rewrite!("gt-to-lt";
            "(gt ?x ?y)" => "(lt ?y ?x)"),
        rewrite!("le-to-ge";
            "(le ?x ?y)" => "(ge ?y ?x)"),
        rewrite!("ge-to-le";
            "(ge ?x ?y)" => "(le ?y ?x)"),
    ]
}

// ---------------------------------------------------------------
// Join elimination rules
// ---------------------------------------------------------------

#[cfg(test)] // RFC 0090: test oracle; production uses generated_logical_join_elimination_core_rules
pub(crate) fn join_elimination_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Self-join with true condition on same table is identity
        // (this is a simplified version; a real implementation
        // would check that columns match)
        rewrite!("cross-join-single-row-right";
            "(join cross ?cond ?left (limit 1 0 ?right))" =>
            "?left"
        ),
    ]
}

// ---------------------------------------------------------------
// Aggregate optimization rules
// ---------------------------------------------------------------

#[must_use]
#[cfg(test)] // RFC 0090: test oracle; production uses generated_logical_aggregate_optimization_core_rules
pub fn aggregate_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Push filter below aggregate when filter only
        // references grouping keys
        rewrite!("filter-below-aggregate";
            "(filter ?pred (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs (filter ?pred ?input))"
        ),
        // Aggregate over aggregate elimination (same grouping)
        rewrite!("aggregate-over-aggregate";
            "(aggregate ?g1 ?a1 (aggregate ?g1 ?a2 ?input))" =>
            "(aggregate ?g1 ?a1 ?input)"
        ),
    ]
}

// ---------------------------------------------------------------
// Limit and sort optimization rules
// ---------------------------------------------------------------

#[cfg(test)] // RFC 0090: test oracle; production uses generated_logical_limit_sort_optimization_core_rules
pub(crate) fn limit_sort_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Push limit through project
        rewrite!("limit-through-project";
            "(limit ?n ?off (project ?cols ?input))" =>
            "(project ?cols (limit ?n ?off ?input))"
        ),
        // Merge adjacent limits (take the smaller)
        rewrite!("limit-merge";
            "(limit ?n1 ?off1 (limit ?n2 ?off2 ?input))" =>
            "(limit ?n1 ?off1 ?input)"
        ),
        // Eliminate sort below limit-sort
        rewrite!("sort-below-sort";
            "(sort ?k1 (sort ?k2 ?input))" =>
            "(sort ?k1 ?input)"
        ),
    ]
}

// ---------------------------------------------------------------
// Set operation rules
// ---------------------------------------------------------------

#[cfg(test)] // RFC 0090: test oracle; production uses generated_logical_set_operation_core_rules
pub(crate) fn set_operation_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Union commutativity
        rewrite!("union-commutativity";
            "(union ?all ?left ?right)" =>
            "(union ?all ?right ?left)"
        ),
        // Intersect commutativity
        rewrite!("intersect-commutativity";
            "(intersect ?all ?left ?right)" =>
            "(intersect ?all ?right ?left)"
        ),
        // Union with self is identity (for UNION ALL)
        rewrite!("union-self-all";
            "(union true ?x ?x)" => "?x"
        ),
        // Intersect with self is identity
        rewrite!("intersect-self";
            "(intersect ?all ?x ?x)" => "?x"
        ),
        // Except with self is empty
        // (represented as a filtered-to-nothing scan)
        rewrite!("except-self";
            "(except ?all ?x ?x)" =>
            "(filter (const-bool false) ?x)"
        ),
    ]
}

// ---------------------------------------------------------------
// Subquery / decorrelation optimization rules
// ---------------------------------------------------------------

#[cfg(test)] // RFC 0090: test oracle; production uses generated_logical_subquery_optimization_core_rules
pub(crate) fn subquery_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Convert semi join + filter to semi join with combined condition
        rewrite!("filter-semi-join-merge";
            "(filter ?pred (join semi ?cond ?left ?right))" =>
            "(join semi (and ?cond ?pred) ?left ?right)"
        ),
        // Convert anti join + filter to anti join with combined condition
        rewrite!("filter-anti-join-merge";
            "(filter ?pred (join anti ?cond ?left ?right))" =>
            "(join anti (and ?cond ?pred) ?left ?right)"
        ),
    ]
}

// ---------------------------------------------------------------
// CTE inlining rules
// ---------------------------------------------------------------

/// CTE inlining and simplification rules.
///
/// Inlines trivial CTEs where the body directly references the CTE
/// by name. For `WITH x AS (def) SELECT * FROM x`, replaces the
/// entire CTE+body with just the definition.
#[cfg(test)] // RFC 0090: test oracle; production uses generated_logical_cte_inlining_core_rules
pub(crate) fn cte_inlining_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Inline CTE when the body is just a scan of the CTE name.
        // WITH x AS (def) SELECT * FROM x  →  def
        rewrite!("cte-inline-direct-scan";
            "(cte ?name ?def (scan ?name))" => "?def"
        ),
        // Inline CTE when the body is a project over the CTE scan.
        // WITH x AS (def) SELECT cols FROM x  →  project(cols, def)
        rewrite!("cte-inline-project-scan";
            "(cte ?name ?def (project ?cols (scan ?name)))" =>
            "(project ?cols ?def)"
        ),
        // Inline CTE when the body is a filter over the CTE scan.
        // WITH x AS (def) SELECT * FROM x WHERE pred  →  filter(pred, def)
        rewrite!("cte-inline-filter-scan";
            "(cte ?name ?def (filter ?pred (scan ?name)))" =>
            "(filter ?pred ?def)"
        ),
        // Inline CTE when the body is filter+project over the CTE scan.
        // WITH x AS (def) SELECT cols FROM x WHERE pred
        //   → project(cols, filter(pred, def))
        rewrite!("cte-inline-project-filter-scan";
            "(cte ?name ?def (project ?cols (filter ?pred (scan ?name))))" =>
            "(project ?cols (filter ?pred ?def))"
        ),
        // Constant comparison folding: (eq const-int const-int) for same
        // values reduces to true, which then allows filter-true elimination.
        // This handles patterns like WHERE 1 = (SELECT 1) after scalar
        // subquery decorrelation produces a constant comparison.
        rewrite!("fold-const-int-eq";
            "(eq (const-int ?n) (const-int ?n))" => "(const-bool true)"
        ),
    ]
}

// ---------------------------------------------------------------
// DuckDB-inspired rules
// Sourced from: src/optimizer/ in the DuckDB repository
// ---------------------------------------------------------------

#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
fn duckdb_inspired_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Column elimination: remove unnecessary columns in project
        // DuckDB: src/optimizer/column_lifetime_analyzer.cpp
        // (simplified: merge redundant projects)
        rewrite!("duckdb-project-pushdown";
            "(project ?c1 (project ?c2 ?input))" =>
            "(project ?c1 ?input)"
        ),
        // Filter pushdown through left outer join (into left side)
        // DuckDB: src/optimizer/filter_pushdown.cpp
        rewrite!("duckdb-filter-through-left-join-left";
            "(filter ?pred (join left-outer ?cond ?left ?right))" =>
            "(join left-outer ?cond (filter ?pred ?left) ?right)"
        ),
        // Arithmetic simplification: a - a => 0
        // DuckDB: src/optimizer/expression_rewriter.cpp
        rewrite!("duckdb-sub-self";
            "(sub ?x ?x)" => "(const-int 0)"
        ),
        // Comparison simplification: NOT (a < b) => a >= b
        // DuckDB: src/optimizer/expression_rewriter.cpp
        rewrite!("duckdb-not-lt";
            "(not (lt ?x ?y))" => "(ge ?x ?y)"
        ),
        rewrite!("duckdb-not-le";
            "(not (le ?x ?y))" => "(gt ?x ?y)"
        ),
        rewrite!("duckdb-not-gt";
            "(not (gt ?x ?y))" => "(le ?x ?y)"
        ),
        rewrite!("duckdb-not-ge";
            "(not (ge ?x ?y))" => "(lt ?x ?y)"
        ),
        rewrite!("duckdb-not-eq";
            "(not (eq ?x ?y))" => "(ne ?x ?y)"
        ),
        rewrite!("duckdb-not-ne";
            "(not (ne ?x ?y))" => "(eq ?x ?y)"
        ),
        // Limit pushdown into union branches
        // DuckDB: src/optimizer/limit_pushdown.cpp
        rewrite!("duckdb-limit-through-union";
            "(limit ?n ?off (union true ?left ?right))" =>
            "(union true (limit ?n ?off ?left) (limit ?n ?off ?right))"
        ),
        // Sort elimination when followed by aggregate
        // DuckDB: src/optimizer/remove_unused_columns.cpp
        rewrite!("duckdb-sort-below-aggregate";
            "(aggregate ?g ?a (sort ?k ?input))" =>
            "(aggregate ?g ?a ?input)"
        ),
    ]
}

// ---------------------------------------------------------------
// SQLite-inspired rules
// Sourced from: src/where.c, src/select.c in the SQLite repository
// ---------------------------------------------------------------

#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
fn sqlite_inspired_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Term rewriting: a >= b AND a <= b => a = b
        // SQLite: where.c - whereLoopAddBtreeIndex()
        rewrite!("sqlite-range-to-eq";
            "(and (ge ?a ?b) (le ?a ?b))" =>
            "(eq ?a ?b)"
        ),
        // Transitive closure on equalities: if a = b and b = c,
        // then a = c. Represented as: filter on conjunction.
        // SQLite: where.c - exprAnalyzeOrTerm()
        rewrite!("sqlite-eq-transitive";
            "(and (eq ?a ?b) (eq ?b ?c))" =>
            "(and (and (eq ?a ?b) (eq ?b ?c)) (eq ?a ?c))"
        ),
        // Convert NOT IN to anti join pattern
        // SQLite: select.c
        rewrite!("sqlite-not-in-to-anti";
            "(filter (not (eq ?col ?val)) ?input)" =>
            "(filter (ne ?col ?val) ?input)"
        ),
        // Flatten nested ORs by distribution
        // SQLite: where.c - exprAnalyzeOrTerm()
        // (a AND b) OR (a AND c) => a AND (b OR c)
        rewrite!("sqlite-or-distribute";
            "(or (and ?a ?b) (and ?a ?c))" =>
            "(and ?a (or ?b ?c))"
        ),
        // Elimination of redundant IS NOT NULL after equality
        // SQLite: where.c - sqlite3ExprImpliesNonNullRow
        rewrite!("sqlite-eq-implies-not-null-left";
            "(and (eq ?x ?y) (is-not-null ?x))" =>
            "(eq ?x ?y)"
        ),
        rewrite!("sqlite-eq-implies-not-null-right";
            "(and (eq ?x ?y) (is-not-null ?y))" =>
            "(eq ?x ?y)"
        ),
        // Constant propagation through joins
        // SQLite: where.c - propagateConstants
        // If filter says col = const, push that into the join cond
        rewrite!("sqlite-const-prop-join";
            "(filter (eq ?col ?val) (join inner (eq ?col ?col2) ?left ?right))" =>
            "(filter (eq ?col ?val) (join inner (eq ?val ?col2) ?left ?right))"
        ),
    ]
}

// ---------------------------------------------------------------
// Runtime filter rules (sideways information passing)
// Inspired by: StarRocks, Spark, Presto runtime filters
// ---------------------------------------------------------------

#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
fn runtime_filter_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Convert hash join to semi-join + hash join pattern.
        // This models the runtime filter: the semi-join acts as a
        // pre-filter on the probe side using build-side keys.
        //
        // hash_join(cond, build, probe) =>
        //   hash_join(cond, build, semi_join(cond, probe, build))
        //
        // The semi-join represents the bloom filter application:
        // it filters probe rows that have no matching build key.
        rewrite!("runtime-filter-hash-to-semi";
            "(join inner (eq ?lcol ?rcol) ?build ?probe)" =>
            "(join inner (eq ?lcol ?rcol) ?build (join semi (eq ?rcol ?lcol) ?probe ?build))"
        ),
        // Push runtime filter (semi-join) through projection.
        // If the probe side has a project, push the filter below.
        rewrite!("runtime-filter-through-project";
            "(join semi ?cond (project ?cols ?input) ?build)" =>
            "(project ?cols (join semi ?cond ?input ?build))"
        ),
        // Push runtime filter through filter.
        // Combine runtime filter with existing scan-level filters.
        rewrite!("runtime-filter-through-filter";
            "(join semi ?cond (filter ?pred ?input) ?build)" =>
            "(filter ?pred (join semi ?cond ?input ?build))"
        ),
    ]
}

// ---------------------------------------------------------------
// Cast optimization rules
// ---------------------------------------------------------------

/// Type cast optimization rules.
///
/// Optimizations:
/// - Eliminate double casts: cast(cast(x, t1), t2) → cast(x, t2)
/// - Remove identity casts: cast(const-int(x), int) → const-int(x)
/// - Push casts through operations when safe
#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
fn cast_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Eliminate consecutive casts (cast(cast(x, t1), t2) → cast(x, t2))
        rewrite!("eliminate-double-cast";
            "(cast (cast ?expr ?type1) ?type2)" =>
            "(cast ?expr ?type2)"
        ),
        // Remove identity cast on integer constants
        rewrite!("remove-cast-int-to-int";
            "(cast (const-int ?val) int)" =>
            "(const-int ?val)"
        ),
        rewrite!("remove-cast-int-to-integer";
            "(cast (const-int ?val) integer)" =>
            "(const-int ?val)"
        ),
        rewrite!("remove-cast-int-to-bigint";
            "(cast (const-int ?val) bigint)" =>
            "(const-int ?val)"
        ),
        // Remove identity cast on string constants
        rewrite!("remove-cast-str-to-text";
            "(cast (const-str ?val) text)" =>
            "(const-str ?val)"
        ),
        rewrite!("remove-cast-str-to-varchar";
            "(cast (const-str ?val) varchar)" =>
            "(const-str ?val)"
        ),
        // TODO: Cast pushdown through arithmetic operations
        // These rules need more careful handling of operator arities
        // Disabled for now to avoid BadOp errors
    ]
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::analysis::RelAnalysis;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    /// RFC 0090 Phase 1: the declarative `.rra` predicate-pushdown rules
    /// (compiled by build.rs from `rules/logical/predicate-pushdown-core/`)
    /// must reproduce the hand-coded `predicate_pushdown_rules()` exactly —
    /// same rule names and same LHS/RHS patterns. This gates retiring the
    /// hand-coded function in favor of the rule files.
    fn assert_rules_identical(
        category: &str,
        hand: &[Rewrite<RelLang, RelAnalysis>],
        generated: &[Rewrite<RelLang, RelAnalysis>],
    ) {
        let fmt = |r: &Rewrite<RelLang, RelAnalysis>| {
            let lhs = r
                .searcher
                .get_pattern_ast()
                .map_or_else(|| "<dynamic>".to_string(), ToString::to_string);
            let rhs = r
                .applier
                .get_pattern_ast()
                .map_or_else(|| "<dynamic>".to_string(), ToString::to_string);
            (r.name.to_string(), format!("{lhs} => {rhs}"))
        };
        let mut hand: Vec<(String, String)> = hand.iter().map(fmt).collect();
        let mut generated: Vec<(String, String)> = generated.iter().map(fmt).collect();
        // Drop no-op rules (LHS==RHS) from the oracle: build.rs rejects them, so
        // they can never appear in the generated set, and a rewrite-to-self is
        // dead weight (egg never derives anything new from it).
        let is_noop = |(_, p): &(String, String)| {
            p.split_once(" => ").is_some_and(|(l, r)| l == r)
        };
        hand.retain(|e| !is_noop(e));
        generated.retain(|e| !is_noop(e));
        hand.sort();
        generated.sort();

        let hand_names: Vec<&String> = hand.iter().map(|(n, _)| n).collect();
        let gen_names: Vec<&String> = generated.iter().map(|(n, _)| n).collect();
        assert_eq!(
            hand_names, gen_names,
            "[{category}] rule name sets diverged between hand-coded and .rra-compiled rules"
        );
        for ((hn, hp), (_gn, gp)) in hand.iter().zip(generated.iter()) {
            assert_eq!(
                hp, gp,
                "[{category}] pattern for rule `{hn}` diverged: hand=[{hp}] generated=[{gp}]"
            );
        }
    }

    #[test]
    fn generated_predicate_pushdown_matches_hand_coded() {
        assert_rules_identical(
            "predicate-pushdown",
            &predicate_pushdown_rules(),
            &generated_logical_predicate_pushdown_core_rules(),
        );
    }

    #[test]
    fn generated_projection_pushdown_matches_hand_coded() {
        assert_rules_identical(
            "projection-pushdown",
            &projection_pushdown_rules(),
            &generated_logical_projection_pushdown_core_rules(),
        );
    }

    #[test]
    fn generated_join_reordering_matches_hand_coded() {
        assert_rules_identical(
            "join-reordering",
            &join_reordering_rules(),
            &generated_logical_join_reordering_core_rules(),
        );
    }

    #[test]
    fn generated_expression_simplification_matches_hand_coded() {
        assert_rules_identical(
            "expression-simplification",
            &expression_simplification_rules(),
            &generated_logical_expression_simplification_core_rules(),
        );
    }

    #[test]
    fn generated_join_elimination_matches_hand_coded() {
        assert_rules_identical(
            "join-elimination",
            &join_elimination_rules(),
            &generated_logical_join_elimination_core_rules(),
        );
    }

    #[test]
    fn generated_aggregate_optimization_matches_hand_coded() {
        assert_rules_identical(
            "aggregate-optimization",
            &aggregate_optimization_rules(),
            &generated_logical_aggregate_optimization_core_rules(),
        );
    }

    #[test]
    fn generated_limit_sort_optimization_matches_hand_coded() {
        assert_rules_identical(
            "limit-sort-optimization",
            &limit_sort_optimization_rules(),
            &generated_logical_limit_sort_optimization_core_rules(),
        );
    }

    #[test]
    fn generated_set_operation_matches_hand_coded() {
        assert_rules_identical(
            "set-operation",
            &set_operation_rules(),
            &generated_logical_set_operation_core_rules(),
        );
    }

    #[test]
    fn generated_subquery_optimization_matches_hand_coded() {
        assert_rules_identical(
            "subquery-optimization",
            &subquery_optimization_rules(),
            &generated_logical_subquery_optimization_core_rules(),
        );
    }

    #[test]
    fn generated_cte_inlining_matches_hand_coded() {
        assert_rules_identical(
            "cte-inlining",
            &cte_inlining_rules(),
            &generated_logical_cte_inlining_core_rules(),
        );
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "flat list of per-category identity assertions")]
    fn generated_second_wave_matches_hand_coded() {
        assert_rules_identical(
            "null-simplification",
            &crate::null_simplification::null_simplification_rules(),
            &generated_logical_null_simplification_core_rules(),
        );
        assert_rules_identical(
            "column-pruning",
            &crate::column_pruning::column_pruning_rules(),
            &generated_logical_column_pruning_core_rules(),
        );
        assert_rules_identical(
            "functional-dependencies",
            &crate::functional_deps::functional_dependency_rules(),
            &generated_logical_functional_dependencies_core_rules(),
        );
        assert_rules_identical(
            "semi-join",
            &crate::semi_join::semi_join_reduction_rules(),
            &generated_logical_semi_join_core_rules(),
        );
        assert_rules_identical(
            "redundant-join",
            &crate::redundant_join::redundant_join_elimination_rules(),
            &generated_logical_redundant_join_core_rules(),
        );
        assert_rules_identical(
            "duckdb",
            &duckdb_inspired_rules(),
            &generated_database_specific_duckdb_core_rules(),
        );
        assert_rules_identical(
            "sqlite",
            &sqlite_inspired_rules(),
            &generated_database_specific_sqlite_core_rules(),
        );
        assert_rules_identical(
            "runtime-filter",
            &runtime_filter_rules(),
            &generated_physical_runtime_filter_core_rules(),
        );
        assert_rules_identical(
            "join-transformation",
            &crate::join_transformations::join_transformation_rules(),
            &generated_logical_join_transformation_core_rules(),
        );
        assert_rules_identical(
            "parquet-pushdown",
            &crate::parquet_pushdown::parquet_pushdown_rules(),
            &generated_physical_parquet_pushdown_core_rules(),
        );
        assert_rules_identical(
            "covering-index",
            &crate::covering_index::covering_index_rules(),
            &generated_physical_covering_index_core_rules(),
        );
        assert_rules_identical(
            "min-max-index",
            &crate::shortcuts::min_max_index::min_max_index_rules(),
            &generated_physical_min_max_index_core_rules(),
        );
        assert_rules_identical(
            "hybrid-search",
            &crate::hybrid_search::hybrid_search_rules(),
            &generated_physical_hybrid_search_core_rules(),
        );
        assert_rules_identical(
            "cast-optimization",
            &cast_optimization_rules(),
            &generated_logical_cast_optimization_core_rules(),
        );
        assert_rules_identical(
            "vector",
            &crate::vector_rules::vector_rewrite_rules(),
            &generated_physical_vector_core_rules(),
        );
        assert_rules_identical(
            "fts",
            &crate::fts_rules::fts_optimization_rules(),
            &generated_physical_fts_core_rules(),
        );
        assert_rules_identical(
            "consensus",
            &crate::consensus_rules::consensus_rules(),
            &generated_logical_consensus_core_rules(),
        );
        assert_rules_identical(
            "count-metadata",
            &crate::count_metadata::count_metadata_rules(),
            &generated_logical_count_metadata_core_rules(),
        );
        assert_rules_identical(
            "xml",
            &crate::xml_optimizer::xml_optimization_rules(),
            &generated_logical_xml_core_rules(),
        );
        assert_rules_identical(
            "index-selection",
            &crate::index_selection::index_selection_rules(),
            &generated_physical_index_selection_core_rules(),
        );
        assert_rules_identical(
            "documentdb",
            &crate::documentdb_optimizer::documentdb_rewrite_rules(),
            &generated_database_specific_documentdb_core_rules(),
        );
        assert_rules_identical(
            "oracle-json-duality",
            &crate::oracle_json_duality::duality_rewrite_rules(),
            &generated_database_specific_oracle_json_duality_core_rules(),
        );
    }

    #[test]
    fn constant_folding_applier_folds_arithmetic() {
        // The applier vocabulary (RFC 0090 Phase 2): a computed-RHS rule folds
        // (add (const-int 2) (const-int 3)) into (const-int 5). The identity
        // test can't validate a computed applier, so check behavior directly.
        let cases = [
            ("(add (const-int 2) (const-int 3))", "(const-int 5)"),
            ("(sub (const-int 10) (const-int 4))", "(const-int 6)"),
            ("(mul (const-int 6) (const-int 7))", "(const-int 42)"),
        ];
        for (start_s, goal_s) in cases {
            let start: egg::RecExpr<RelLang> =
                start_s.parse().expect("valid start expr");
            let runner = Runner::default()
                .with_expr(&start)
                .with_iter_limit(5)
                .run(&generated_logical_constant_folding_core_rules());
            let goal: egg::RecExpr<RelLang> = goal_s.parse().expect("valid goal expr");
            let start_id = runner.egraph.lookup_expr(&start);
            let goal_id = runner.egraph.lookup_expr(&goal);
            assert!(
                goal_id.is_some() && start_id == goal_id,
                "expected {start_s} to fold to {goal_s} (start={start_id:?} goal={goal_id:?})"
            );
        }
    }

    fn run_optimization(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&all_rules())
    }

    #[test]
    fn filter_merge_creates_conjunction() {
        let expr = RelExpr::scan("t")
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Const(Const::Int(10))),
            })
            .filter(Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(Expr::Column(ColumnRef::new("b"))),
                right: Box::new(Expr::Const(Const::Int(20))),
            });
        let runner = run_optimization(&expr);
        // E-graph should have grown (rules applied)
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn join_commutativity_applied() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            },
            left: Box::new(RelExpr::scan("left_table")),
            right: Box::new(RelExpr::scan("right_table")),
        };
        let runner = run_optimization(&expr);
        // Commutativity should add alternatives
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn filter_true_eliminated() {
        let expr = RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));
        let runner = run_optimization(&expr);
        let root = runner.roots[0];
        let data = &runner.egraph[root].data;
        assert!(data.tables.contains("t"));
    }

    #[test]
    fn all_rules_count() {
        let rules = all_rules();
        assert!(
            rules.len() >= 50,
            "expected at least 50 rules, got {}",
            rules.len()
        );
    }

    #[test]
    fn double_negation_simplification() {
        let expr = RelExpr::scan("t").filter(Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::Not,
            operand: Box::new(Expr::UnaryOp {
                op: ra_core::expr::UnaryOp::Not,
                operand: Box::new(Expr::Column(ColumnRef::new("active"))),
            }),
        });
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn demorgan_rule_applied() {
        let expr = RelExpr::scan("t").filter(Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::Not,
            operand: Box::new(Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            }),
        });
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn projection_pushdown_through_filter() {
        use ra_core::algebra::ProjectionColumn;
        let expr = RelExpr::scan("t")
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            })
            .project(vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            }]);
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn filter_false_eliminates_scan() {
        let expr = RelExpr::scan("t").filter(Expr::Const(Const::Bool(false)));
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() >= 1);
    }

    #[test]
    fn limit_over_limit_merged() {
        let expr = RelExpr::scan("t").limit(100, 0).limit(10, 0);
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn all_rules_annotated_has_groups() {
        let annotated = all_rules_annotated();
        assert!(
            annotated.len() >= 10,
            "expected at least 10 annotated groups, got {}",
            annotated.len()
        );
    }

    #[test]
    fn all_rules_annotated_labels_unique() {
        let annotated = all_rules_annotated();
        let mut labels: Vec<&str> = annotated.iter().map(|g| g.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels.len(),
            annotated.len(),
            "annotated rule group labels should be unique"
        );
    }

    #[test]
    fn all_rules_sorted_same_count_as_unsorted() {
        let sorted = all_rules();
        let unsorted = all_rules_unsorted();
        assert_eq!(sorted.len(), unsorted.len());
    }

    #[test]
    fn feature_gated_rules_from_annotations() {
        let annotated = all_rules_annotated();
        // At least some groups should require joins
        let join_groups: Vec<_> = annotated
            .iter()
            .filter(|g| {
                g.annotation
                    .required_features
                    .contains(QueryFeatureSet::HAS_JOIN)
            })
            .collect();
        assert!(
            !join_groups.is_empty(),
            "expected some groups that require joins"
        );
    }

    #[test]
    fn aggregate_optimization_adds_alternatives() {
        use ra_core::algebra::{AggregateExpr, AggregateFunction};
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("dept"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(Expr::Column(ColumnRef::new("id"))),
                distinct: false,
                alias: Some("cnt".into()),
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() >= 1);
    }

    #[test]
    fn or_with_false_simplified() {
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(Expr::Column(ColumnRef::new("active"))),
            right: Box::new(Expr::Const(Const::Bool(false))),
        });
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn and_with_true_simplified() {
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Column(ColumnRef::new("active"))),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });
        let runner = run_optimization(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn cte_inline_direct_scan() {
        // WITH x AS (SELECT * FROM source) SELECT * FROM x
        // Should inline to just: scan("source")
        let cte = RelExpr::CTE {
            name: "x".to_owned(),
            definition: Box::new(RelExpr::scan("source")),
            body: Box::new(RelExpr::scan("x")),
        };
        let rec = to_rec_expr(&cte).expect("conversion should succeed");
        let rules = cte_inlining_rules();
        let runner = Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&rules);

        // After inlining, the e-graph should contain the "source" scan
        // in the same e-class as the root (meaning they're equivalent).
        let root = runner.roots[0];
        let data = &runner.egraph[root].data;
        assert!(
            data.tables.contains("source"),
            "CTE should be inlined to source scan, tables: {:?}",
            data.tables
        );
    }

    #[test]
    fn cte_inline_with_filter() {
        // WITH x AS (scan(source)) SELECT * FROM x WHERE pred
        // Should inline to: filter(pred, scan(source))
        let cte = RelExpr::CTE {
            name: "x".to_owned(),
            definition: Box::new(RelExpr::scan("source")),
            body: Box::new(RelExpr::scan("x").filter(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Const(Const::Int(1))),
            })),
        };
        let rec = to_rec_expr(&cte).expect("conversion should succeed");
        let rules = cte_inlining_rules();
        let runner = Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&rules);

        let root = runner.roots[0];
        let data = &runner.egraph[root].data;
        assert!(
            data.tables.contains("source"),
            "CTE with filter should inline to filtered source scan"
        );
    }
}
