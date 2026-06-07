//! Lazy rule compilation for efficient query optimization.
//!
//! This module implements on-demand rule loading based on query structure analysis.
//! For simple queries, loading only relevant rules reduces compilation overhead by ~41%
//! (206 → 122 rules for simple SELECT-FROM-WHERE queries).
//!
//! # Architecture
//!
//! 1. **`LazyQueryPattern`** - Analyzes query structure to determine complexity
//! 2. **`RuleCategory`** - Groups rules into 14 categories (Scan, Join, Filter, etc.)
//! 3. **`LazyRuleCompiler`** - Loads baseline + on-demand rules based on query pattern
//!
//! # Example
//!
//! ```rust,no_run
//! use ra_engine::lazy_rules::{LazyQueryPattern, LazyRuleCompiler};
//! use ra_core::algebra::RelExpr;
//!
//! let expr = RelExpr::scan("users");
//! let pattern = LazyQueryPattern::analyze(&expr);
//!
//! let compiler = LazyRuleCompiler::new();
//! let rules = compiler.compile(&pattern);
//! ```

use egg::Rewrite;
use std::collections::HashSet;

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use ra_core::algebra::RelExpr;

/// Query pattern classification for lazy rule loading.
///
/// Analyzes query structure to determine which rule categories are needed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[expect(clippy::struct_excessive_bools, reason = "boolean flags represent independent query features")]
pub struct LazyQueryPattern {
    /// Query contains joins (inner, left, right, full, cross)
    pub has_joins: bool,
    /// Query contains aggregates (sum, count, avg, min, max, group by)
    pub has_aggregates: bool,
    /// Query contains subqueries (in, exists, scalar subquery)
    pub has_subqueries: bool,
    /// Query contains set operations (union, intersect, except)
    pub has_set_ops: bool,
    /// Query contains window functions (`row_number`, rank, lag, lead)
    pub has_window_functions: bool,
    /// Query contains sorting (order by)
    pub has_sorting: bool,
    /// Query contains limits (limit, offset)
    pub has_limits: bool,
    /// Query contains distinct operations
    pub has_distinct: bool,
    /// Query accesses JSON fields (`BinOp::JsonAccess` or `FieldAccess`)
    pub has_json_access: bool,
    /// Query calls BSON-specific functions
    pub has_bson_func: bool,
    /// Query uses vector distance operators
    pub has_vector_distance: bool,
    /// Query uses full-text search matching
    pub has_fts_match: bool,
    /// Query calls XML/XPath/XQuery functions
    pub has_xml_func: bool,
    /// Query contains CTE definitions
    pub has_cte: bool,
    /// Query contains recursive CTEs
    pub has_recursive_cte: bool,
    /// Query uses type casts
    pub has_cast: bool,
    /// Number of tables involved
    pub table_count: usize,
    /// Maximum join depth (nested joins)
    pub join_depth: usize,
}

impl LazyQueryPattern {
    /// Analyze query structure to determine required rules.
    ///
    /// Recursively traverses the `RelExpr` tree to detect query features.
    #[must_use]
    pub fn analyze(expr: &RelExpr) -> Self {
        let mut pattern = Self::default();
        Self::analyze_recursive(expr, &mut pattern, 0);
        pattern
    }

    fn analyze_recursive(expr: &RelExpr, pattern: &mut Self, depth: usize) {
        match expr {
            // Joins
            RelExpr::Join {
                condition,
                left,
                right,
                ..
            } => {
                pattern.has_joins = true;
                pattern.join_depth = pattern.join_depth.max(depth + 1);
                Self::scan_expr_features(condition, pattern);
                Self::analyze_recursive(left, pattern, depth + 1);
                Self::analyze_recursive(right, pattern, depth + 1);
            }

            // Aggregates
            RelExpr::Aggregate { input, .. } => {
                pattern.has_aggregates = true;
                Self::analyze_recursive(input, pattern, depth);
            }

            // Set operations
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                pattern.has_set_ops = true;
                Self::analyze_recursive(left, pattern, depth);
                Self::analyze_recursive(right, pattern, depth);
            }

            // Sorting
            RelExpr::Sort { input, .. } => {
                pattern.has_sorting = true;
                Self::analyze_recursive(input, pattern, depth);
            }

            // Limits
            RelExpr::Limit { input, .. } => {
                pattern.has_limits = true;
                Self::analyze_recursive(input, pattern, depth);
            }

            // Distinct
            RelExpr::Distinct { input } => {
                pattern.has_distinct = true;
                Self::analyze_recursive(input, pattern, depth);
            }

            // Window functions
            RelExpr::Window { input, .. } => {
                pattern.has_window_functions = true;
                Self::analyze_recursive(input, pattern, depth);
            }

            // Filter: scan predicate for content-type features
            RelExpr::Filter { predicate, input } => {
                Self::scan_expr_features(predicate, pattern);
                Self::analyze_recursive(input, pattern, depth);
            }

            // Project: scan projection expressions
            RelExpr::Project { columns, input } => {
                for col in columns {
                    Self::scan_expr_features(&col.expr, pattern);
                }
                Self::analyze_recursive(input, pattern, depth);
            }

            // Scan (base table)
            RelExpr::Scan { .. } => {
                pattern.table_count += 1;
            }

            // CTEs
            RelExpr::CTE {
                definition, body, ..
            } => {
                pattern.has_subqueries = true;
                pattern.has_cte = true;
                Self::analyze_recursive(definition, pattern, depth);
                Self::analyze_recursive(body, pattern, depth);
            }

            RelExpr::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => {
                pattern.has_subqueries = true;
                pattern.has_cte = true;
                pattern.has_recursive_cte = true;
                Self::analyze_recursive(base_case, pattern, depth);
                Self::analyze_recursive(recursive_case, pattern, depth);
                Self::analyze_recursive(body, pattern, depth);
            }

            // Vector search operators
            RelExpr::TopK { input, .. } | RelExpr::VectorFilter { input, .. } => {
                pattern.has_vector_distance = true;
                Self::analyze_recursive(input, pattern, depth);
            }

            // Terminal nodes and unhandled variants
            _ => {}
        }
    }

    /// Scan an `Expr` subtree for content-type features.
    fn scan_expr_features(expr: &ra_core::expr::Expr, pattern: &mut Self) {
        use ra_core::expr::{BinOp, Expr};
        match expr {
            Expr::BinOp {
                op: BinOp::JsonAccess,
                left,
                right,
                ..
            } => {
                pattern.has_json_access = true;
                Self::scan_expr_features(left, pattern);
                Self::scan_expr_features(right, pattern);
            }
            Expr::BinOp { left, right, .. } => {
                Self::scan_expr_features(left, pattern);
                Self::scan_expr_features(right, pattern);
            }
            Expr::UnaryOp { operand, .. } => {
                Self::scan_expr_features(operand, pattern);
            }
            Expr::FieldAccess { expr: inner, .. } => {
                pattern.has_json_access = true;
                Self::scan_expr_features(inner, pattern);
            }
            Expr::Cast { expr: inner, .. } => {
                pattern.has_cast = true;
                Self::scan_expr_features(inner, pattern);
            }
            Expr::Function { name, args, .. } => {
                Self::classify_function(name, pattern);
                for arg in args {
                    Self::scan_expr_features(arg, pattern);
                }
            }
            Expr::FullTextMatch { .. } => {
                pattern.has_fts_match = true;
            }
            Expr::VectorDistance { column, target, .. } => {
                pattern.has_vector_distance = true;
                Self::scan_expr_features(column, pattern);
                Self::scan_expr_features(target, pattern);
            }
            Expr::Case {
                operand,
                when_clauses,
                else_result,
                ..
            } => {
                if let Some(op) = operand {
                    Self::scan_expr_features(op, pattern);
                }
                for (cond, val) in when_clauses {
                    Self::scan_expr_features(cond, pattern);
                    Self::scan_expr_features(val, pattern);
                }
                if let Some(el) = else_result {
                    Self::scan_expr_features(el, pattern);
                }
            }
            Expr::SubQuery {
                query, test_expr, ..
            } => {
                pattern.has_subqueries = true;
                Self::analyze_recursive(query, pattern, 0);
                if let Some(te) = test_expr {
                    Self::scan_expr_features(te, pattern);
                }
            }
            Expr::Array(items) => {
                for item in items {
                    Self::scan_expr_features(item, pattern);
                }
            }
            Expr::ArrayIndex(arr, idx) => {
                Self::scan_expr_features(arr, pattern);
                Self::scan_expr_features(idx, pattern);
            }
            Expr::ArraySlice { array, start, end } => {
                Self::scan_expr_features(array, pattern);
                if let Some(s) = start {
                    Self::scan_expr_features(s, pattern);
                }
                if let Some(e) = end {
                    Self::scan_expr_features(e, pattern);
                }
            }
            // Leaf nodes: Column, Const, PatternClassifier, etc.
            _ => {}
        }
    }

    /// Classify a function call by name to detect content-type features.
    fn classify_function(name: &str, pattern: &mut Self) {
        let lower = name.to_lowercase();

        // BSON functions (DocumentDB)
        if lower.starts_with("bson_")
            || lower.starts_with("documentdb_")
            || lower == "bsontype"
            || lower == "bsonsize"
        {
            pattern.has_bson_func = true;
            return;
        }

        // XML/XPath/XQuery functions
        if lower.starts_with("xpath")
            || lower.starts_with("xquery")
            || lower.starts_with("xml")
            || lower == "extractvalue"
            || lower == "existsnode"
            || lower == "xmlelement"
            || lower == "xmlforest"
            || lower == "xmlagg"
            || lower == "xmlparse"
            || lower == "xmlserialize"
        {
            pattern.has_xml_func = true;
            return;
        }

        // JSON functions (beyond operator-based access)
        if lower.starts_with("json_")
            || lower.starts_with("jsonb_")
            || lower == "json_value"
            || lower == "json_query"
            || lower == "json_exists"
            || lower == "json_array"
            || lower == "json_object"
        {
            pattern.has_json_access = true;
            return;
        }

        // Full-text search functions
        if lower == "to_tsvector"
            || lower == "to_tsquery"
            || lower == "plainto_tsquery"
            || lower == "phraseto_tsquery"
            || lower == "ts_rank"
            || lower == "ts_rank_cd"
            || lower == "match"
            || lower == "contains"
            || lower == "freetext"
        {
            pattern.has_fts_match = true;
        }
    }

    /// Classify query complexity level.
    #[must_use]
    pub fn complexity_level(&self) -> LazyQueryComplexity {
        if self.has_window_functions || self.has_subqueries || self.join_depth > 3 {
            LazyQueryComplexity::Complex
        } else if self.has_joins || self.has_aggregates || self.has_set_ops {
            LazyQueryComplexity::Medium
        } else if self.table_count <= 1 && !self.has_sorting {
            LazyQueryComplexity::Trivial
        } else {
            LazyQueryComplexity::Simple
        }
    }
}

/// Query complexity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LazyQueryComplexity {
    /// Single table, no joins, no sorting (SELECT * FROM t WHERE x > 10)
    Trivial,
    /// Multiple tables or simple sorting (SELECT * FROM t1, t2 WHERE t1.id = t2.id)
    Simple,
    /// Joins, aggregates, or set operations (SELECT COUNT(*) FROM t1 JOIN t2 GROUP BY x)
    Medium,
    /// Subqueries, window functions, or deep joins (SELECT * FROM (SELECT...))
    Complex,
}

/// Rule category for lazy compilation.
///
/// Groups related rewrite rules so they can be loaded on demand based on
/// query structure. Baseline categories are always loaded; others load
/// only when the query pattern requires them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleCategory {
    // === Baseline (always loaded) ===
    /// Filter pushdown and merging (baseline)
    FilterOptimization,
    /// Projection pushdown (baseline)
    ProjectionOptimization,
    /// Expression simplification and constant folding (baseline)
    ExpressionSimplification,
    /// Null handling and simplification (baseline)
    NullSimplification,

    // === On-demand categories ===
    /// Join reordering (commutativity, associativity)
    JoinReordering,
    /// Join elimination (redundant joins, foreign keys)
    JoinElimination,
    /// Join transformations (cross to inner, etc.)
    JoinTransformation,
    /// Semi-join and anti-join optimizations
    SemiJoinOptimization,
    /// Aggregate pushdown and optimization
    AggregateOptimization,
    /// Limit and sort optimization
    LimitSortOptimization,
    /// Set operations (union, intersect, except)
    SetOperationOptimization,
    /// Subquery decorrelation and optimization
    SubqueryOptimization,
    /// File format pushdown (Parquet, etc.)
    FileFormatOptimization,
    /// Metadata shortcuts (count, min/max index)
    MetadataShortcuts,
}

impl RuleCategory {
    /// Check if this category is always loaded (baseline).
    #[must_use]
    pub const fn is_baseline(self) -> bool {
        matches!(
            self,
            Self::FilterOptimization
                | Self::ProjectionOptimization
                | Self::ExpressionSimplification
                | Self::NullSimplification
        )
    }

    /// Determine which categories are needed for a query pattern.
    #[must_use]
    pub fn required_for_pattern(pattern: &LazyQueryPattern) -> HashSet<Self> {
        let mut categories = HashSet::new();

        // Baseline rules always included
        categories.insert(Self::FilterOptimization);
        categories.insert(Self::ProjectionOptimization);
        categories.insert(Self::ExpressionSimplification);
        categories.insert(Self::NullSimplification);

        // On-demand rules based on query features
        if pattern.has_joins {
            categories.insert(Self::JoinReordering);
            categories.insert(Self::JoinElimination);
            categories.insert(Self::JoinTransformation);
            categories.insert(Self::SemiJoinOptimization);
        }

        if pattern.has_aggregates {
            categories.insert(Self::AggregateOptimization);
        }

        if pattern.has_sorting || pattern.has_limits {
            categories.insert(Self::LimitSortOptimization);
        }

        if pattern.has_set_ops {
            categories.insert(Self::SetOperationOptimization);
        }

        if pattern.has_subqueries {
            categories.insert(Self::SubqueryOptimization);
        }

        // File format and metadata optimizations: always beneficial
        categories.insert(Self::FileFormatOptimization);
        categories.insert(Self::MetadataShortcuts);

        categories
    }
}

/// Lazy rule compiler that loads rules on demand.
///
/// Maintains a cache of rule groups and loads only the categories needed
/// for the current query pattern.
#[expect(clippy::type_complexity, reason = "Arc<RwLock<HashMap>> is inherent to concurrent cache")]
pub struct LazyRuleCompiler {
    /// Cache of compiled rule sets by category
    rule_cache: std::sync::Arc<
        std::sync::RwLock<
            std::collections::HashMap<RuleCategory, Vec<Rewrite<RelLang, RelAnalysis>>>,
        >,
    >,
}

impl LazyRuleCompiler {
    /// Create a new lazy rule compiler.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rule_cache: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    /// Compile rules for a specific query pattern.
    ///
    /// Returns only the rules needed for the query's features.
    /// Rules are cached for reuse across queries.
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    #[must_use]
    #[expect(clippy::expect_used, reason = "RwLock poisoning is unrecoverable")]
    pub fn compile(&self, pattern: &LazyQueryPattern) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        let required = RuleCategory::required_for_pattern(pattern);
        let mut rules = Vec::with_capacity(150); // Typical size for medium queries

        for category in required {
            // Check cache first
            {
                let cache = self.rule_cache.read().expect("rule_cache lock poisoned");
                if let Some(cached_rules) = cache.get(&category) {
                    rules.extend(cached_rules.clone());
                    continue;
                }
            }

            // Not cached, load and cache
            let category_rules = Self::load_category(category);
            {
                let mut cache = self.rule_cache.write().expect("rule_cache lock poisoned");
                cache.insert(category, category_rules.clone());
            }
            rules.extend(category_rules);
        }

        // Apply priority sorting
        crate::rule_priority::sort_rules_by_priority(rules)
    }

    /// Load rules for a specific category.
    fn load_category(category: RuleCategory) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        match category {
            // RFC 0090: predicate-pushdown is sourced from the .rra corpus.
            RuleCategory::FilterOptimization => {
                let mut rules = crate::rewrite::generated_logical_predicate_pushdown_core_rules();
                // RFC 0091 Option B: cost-driven scan-method lowering
                // (Filter(Scan) -> index-scan-choice, guarded by has_index_for);
                // the extractor chooses seq vs index by per-method cost under
                // live conditions.
                rules.extend(crate::rewrite::generated_physical_scan_lowering_core_rules());
                rules
            }
            RuleCategory::ProjectionOptimization => {
                crate::rewrite::generated_logical_projection_pushdown_core_rules()
            }
            RuleCategory::ExpressionSimplification => {
                let mut rules = crate::rewrite::generated_logical_expression_simplification_core_rules();
                rules.extend(crate::rewrite::generated_logical_constant_folding_core_rules());
                rules
            }
            RuleCategory::NullSimplification => {
                crate::rewrite::generated_logical_null_simplification_core_rules()
            }

            RuleCategory::JoinReordering => {
                let mut rules = crate::rewrite::generated_logical_join_reordering_core_rules();
                // RFC 0090 Phase 3 chunk 4: cost-driven physical join lowering
                // (join -> hash/merge/nest-loop) now runs in the production
                // e-graph path; the extractor chooses by per-method cost.
                rules.extend(crate::rewrite::generated_physical_join_lowering_core_rules());
                rules
            }
            RuleCategory::JoinElimination => {
                let mut rules = crate::rewrite::generated_logical_join_elimination_core_rules();
                rules.extend(crate::rewrite::generated_logical_redundant_join_core_rules());
                rules
            }
            RuleCategory::JoinTransformation => {
                crate::rewrite::generated_logical_join_transformation_core_rules()
            }
            RuleCategory::SemiJoinOptimization => crate::rewrite::generated_logical_semi_join_core_rules(),

            RuleCategory::AggregateOptimization => crate::rewrite::generated_logical_aggregate_optimization_core_rules(),
            RuleCategory::LimitSortOptimization => crate::rewrite::generated_logical_limit_sort_optimization_core_rules(),
            RuleCategory::SetOperationOptimization => crate::rewrite::generated_logical_set_operation_core_rules(),
            RuleCategory::SubqueryOptimization => crate::rewrite::generated_logical_subquery_optimization_core_rules(),

            RuleCategory::FileFormatOptimization => {
                crate::rewrite::generated_physical_parquet_pushdown_core_rules()
            }
            RuleCategory::MetadataShortcuts => {
                let mut rules = crate::rewrite::generated_logical_count_metadata_core_rules();
                rules.extend(crate::rewrite::generated_physical_min_max_index_core_rules());
                rules.extend(crate::rewrite::generated_physical_covering_index_core_rules());
                rules
            }
        }
    }

    /// Get statistics about rule loading.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    #[must_use]
    #[expect(clippy::expect_used, reason = "RwLock poisoning is unrecoverable")]
    pub fn stats(&self) -> LazyCompilerStats {
        let cache = self.rule_cache.read().expect("rule_cache lock poisoned");
        let cached_categories = cache.len();
        let total_cached_rules: usize = cache.values().map(Vec::len).sum();

        LazyCompilerStats {
            cached_categories,
            total_cached_rules,
        }
    }
}

impl Default for LazyRuleCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about lazy rule compilation.
#[derive(Debug, Clone)]
pub struct LazyCompilerStats {
    /// Number of cached rule categories
    pub cached_categories: usize,
    /// Total number of cached rules
    pub total_cached_rules: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trivial_query_pattern() {
        use ra_core::algebra::RelExpr;
        use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

        // Simple scan with filter: SELECT * FROM t WHERE x > 10
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef {
                    table: None,
                    column: "x".to_string(),
                })),
                right: Box::new(Expr::Const(Const::Int(10))),
            },
            input: Box::new(RelExpr::Scan {
                table: "t".to_string(),
                alias: None,
            }),
        };

        let pattern = LazyQueryPattern::analyze(&expr);
        assert!(!pattern.has_joins);
        assert!(!pattern.has_aggregates);
        assert_eq!(pattern.table_count, 1);
        assert_eq!(pattern.complexity_level(), LazyQueryComplexity::Trivial);
    }

    #[test]
    fn test_simple_join_pattern() {
        use ra_core::algebra::{JoinType, RelExpr};
        use ra_core::expr::{BinOp, ColumnRef, Expr};

        // Two-table join: SELECT * FROM t1 JOIN t2 ON t1.id = t2.id
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef {
                    table: Some("t1".to_string()),
                    column: "id".to_string(),
                })),
                right: Box::new(Expr::Column(ColumnRef {
                    table: Some("t2".to_string()),
                    column: "id".to_string(),
                })),
            },
            left: Box::new(RelExpr::Scan {
                table: "t1".to_string(),
                alias: None,
            }),
            right: Box::new(RelExpr::Scan {
                table: "t2".to_string(),
                alias: None,
            }),
        };

        let pattern = LazyQueryPattern::analyze(&expr);
        assert!(pattern.has_joins);
        assert!(!pattern.has_aggregates);
        assert_eq!(pattern.table_count, 2);
        assert_eq!(pattern.join_depth, 1);
        assert_eq!(pattern.complexity_level(), LazyQueryComplexity::Medium);
    }

    #[test]
    fn test_aggregate_pattern() {
        use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
        use ra_core::expr::{ColumnRef, Expr};

        // Aggregate query: SELECT COUNT(*) FROM t GROUP BY x
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef {
                table: None,
                column: "x".to_string(),
            })],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None, // COUNT(*) has no specific argument
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::Scan {
                table: "t".to_string(),
                alias: None,
            }),
        };

        let pattern = LazyQueryPattern::analyze(&expr);
        assert!(pattern.has_aggregates);
        assert_eq!(pattern.complexity_level(), LazyQueryComplexity::Medium);
    }

    #[test]
    fn test_complex_nested_join_pattern() {
        use ra_core::algebra::{JoinType, RelExpr};
        use ra_core::expr::{Const, Expr};

        // Three-way nested join
        let t1 = RelExpr::Scan {
            table: "t1".to_string(),
            alias: None,
        };
        let t2 = RelExpr::Scan {
            table: "t2".to_string(),
            alias: None,
        };
        let t3 = RelExpr::Scan {
            table: "t3".to_string(),
            alias: None,
        };

        let join12 = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(t1),
            right: Box::new(t2),
        };

        let join123 = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(join12),
            right: Box::new(t3),
        };

        let pattern = LazyQueryPattern::analyze(&join123);
        assert!(pattern.has_joins);
        assert_eq!(pattern.table_count, 3);
        assert_eq!(pattern.join_depth, 2);
    }

    #[test]
    fn test_set_operation_pattern() {
        use ra_core::algebra::RelExpr;

        // Union query
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::Scan {
                table: "t1".to_string(),
                alias: None,
            }),
            right: Box::new(RelExpr::Scan {
                table: "t2".to_string(),
                alias: None,
            }),
        };

        let pattern = LazyQueryPattern::analyze(&expr);
        assert!(pattern.has_set_ops);
        assert_eq!(pattern.complexity_level(), LazyQueryComplexity::Medium);
    }

    #[test]
    fn test_baseline_categories_always_included() {
        let pattern = LazyQueryPattern::default();
        let categories = RuleCategory::required_for_pattern(&pattern);

        assert!(categories.contains(&RuleCategory::FilterOptimization));
        assert!(categories.contains(&RuleCategory::ProjectionOptimization));
        assert!(categories.contains(&RuleCategory::ExpressionSimplification));
        assert!(categories.contains(&RuleCategory::NullSimplification));
    }

    #[test]
    fn test_join_categories_for_join_query() {
        let pattern = LazyQueryPattern {
            has_joins: true,
            ..LazyQueryPattern::default()
        };

        let categories = RuleCategory::required_for_pattern(&pattern);

        assert!(categories.contains(&RuleCategory::JoinReordering));
        assert!(categories.contains(&RuleCategory::JoinElimination));
        assert!(categories.contains(&RuleCategory::JoinTransformation));
    }

    #[test]
    fn test_lazy_compiler_basic() {
        let compiler = LazyRuleCompiler::new();
        let pattern = LazyQueryPattern::default();

        let rules = compiler.compile(&pattern);
        assert!(!rules.is_empty());

        // Should have baseline rules
        let stats = compiler.stats();
        assert!(stats.cached_categories > 0);
    }

    #[test]
    fn test_lazy_compiler_caching() {
        let compiler = LazyRuleCompiler::new();
        let pattern = LazyQueryPattern::default();

        // First compile
        let rules1 = compiler.compile(&pattern);
        let stats1 = compiler.stats();

        // Second compile should use cache
        let rules2 = compiler.compile(&pattern);
        let stats2 = compiler.stats();

        assert_eq!(rules1.len(), rules2.len());
        assert_eq!(stats1.cached_categories, stats2.cached_categories);
    }

    #[test]
    fn test_rule_reduction_for_simple_query() {
        use crate::rewrite::all_rules;

        let compiler = LazyRuleCompiler::new();

        // Simple query pattern
        let simple_pattern = LazyQueryPattern::default();
        let lazy_rules = compiler.compile(&simple_pattern);

        // Full rule set
        let all = all_rules();

        // Should load significantly fewer rules for simple queries
        assert!(lazy_rules.len() < all.len());

        // Expected ~40% reduction: 206 → ~122 rules
        let reduction_pct = ((all.len() - lazy_rules.len()) as f64 / all.len() as f64) * 100.0;
        assert!(
            reduction_pct > 30.0,
            "Expected >30% reduction, got {reduction_pct:.1}%",
        );
    }
}
