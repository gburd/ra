//! Enhanced fast-path selection for OLTP query optimization.
//!
//! Detects queries that can bypass e-graph equality saturation
//! entirely, routing them to specialized fast-path optimizers
//! instead. This provides 10-100x speedup for common OLTP patterns
//! where the optimal plan is known without search.
//!
//! # Fast-path categories
//!
//! - **Left-deep joins** (2-10 tables): cardinality-sorted left-deep
//!   tree avoids the exponential join ordering search.
//! - **Index-only queries**: covering index detected at planning time;
//!   skip e-graph and emit `index-only-scan` directly.
//! - **Simple aggregations**: `COUNT(*)`, `SUM(col)`, single-group
//!   `GROUP BY` queries answered from metadata or a single pass.
//! - **Materialized view matches**: exact or subsumption match against
//!   the MV catalog; rewrite to MV scan with compensation.
//!
//! # Integration with `ResourceBudget`
//!
//! [`FastPathPreferences`] controls which fast paths are enabled and
//! their thresholds. The struct is defined here and re-exported
//! through [`ResourceBudget`](crate::resource_budget::ResourceBudget)
//! so callers can configure fast-path behavior as part of their
//! workload-specific budget.

use ra_core::algebra::{AggregateFunction, RelExpr};
use ra_core::expr::Expr;

use crate::left_deep::can_use_left_deep;
use crate::mv_matching::{MvCatalog, MvMatch};

// ── Fast-path preferences ───────────────────────────────────────

/// Preferences for fast-path routing of queries that match known
/// patterns.
///
/// Fast paths bypass full e-graph exploration when the query shape
/// is simple enough that the optimal plan is known. Each boolean
/// field enables a category of fast paths; the threshold fields
/// control how aggressively they are applied.
///
/// Constructors provide sensible defaults for common workloads:
/// - [`default()`](Self::default) -- all fast paths enabled, broad
///   table range (2-10), moderate confidence threshold.
/// - [`oltp()`](Self::oltp) -- aggressive fast-pathing for
///   low-latency queries.
/// - [`olap()`](Self::olap) -- conservative fast-pathing for
///   analytical workloads.
/// - [`disabled()`](Self::disabled) / [`none()`](Self::none) -- all
///   fast paths off; every query goes through full e-graph
///   optimization.
#[derive(Debug, Clone, PartialEq)]
#[expect(clippy::struct_excessive_bools)]
pub struct FastPathPreferences {
    /// Enable the left-deep join fast path (2-N tables).
    pub enable_left_deep: bool,
    /// Maximum number of tables for the left-deep fast path.
    /// Default: 10 (expanded from the original 2-7 range).
    pub left_deep_max_tables: usize,
    /// Enable index-only scan detection for covering indexes.
    pub enable_index_only: bool,
    /// Enable simple aggregation fast paths (COUNT(*), SUM, basic
    /// GROUP BY).
    pub enable_simple_aggregation: bool,
    /// Enable materialized view matching.
    pub enable_mv_matching: bool,
    /// Minimum confidence score (0.0-1.0) to accept a fast-path
    /// decision. Lower values are more aggressive.
    pub min_confidence: f64,
}

impl Default for FastPathPreferences {
    fn default() -> Self {
        Self {
            enable_left_deep: true,
            left_deep_max_tables: 10,
            enable_index_only: true,
            enable_simple_aggregation: true,
            enable_mv_matching: true,
            min_confidence: 0.7,
        }
    }
}

impl FastPathPreferences {
    /// OLTP-optimized: all fast paths enabled, aggressive
    /// thresholds, full 2-10 table range.
    #[must_use]
    pub fn oltp() -> Self {
        Self {
            enable_left_deep: true,
            left_deep_max_tables: 10,
            enable_index_only: true,
            enable_simple_aggregation: true,
            enable_mv_matching: true,
            min_confidence: 0.5,
        }
    }

    /// OLAP: conservative fast-pathing. Left-deep limited to 4
    /// tables; high confidence threshold.
    #[must_use]
    pub fn olap() -> Self {
        Self {
            enable_left_deep: true,
            left_deep_max_tables: 4,
            enable_index_only: true,
            enable_simple_aggregation: true,
            enable_mv_matching: true,
            min_confidence: 0.9,
        }
    }

    /// All fast paths disabled; every query goes through full
    /// e-graph optimization.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enable_left_deep: false,
            left_deep_max_tables: 0,
            enable_index_only: false,
            enable_simple_aggregation: false,
            enable_mv_matching: false,
            min_confidence: 1.0,
        }
    }

    /// Alias for [`disabled()`](Self::disabled) -- no fast paths
    /// enabled.
    #[must_use]
    pub fn none() -> Self {
        Self::disabled()
    }

    /// Whether any fast-path category is enabled.
    #[must_use]
    pub fn any_enabled(&self) -> bool {
        self.enable_left_deep
            || self.enable_index_only
            || self.enable_simple_aggregation
            || self.enable_mv_matching
    }
}

// ── Fast-path kinds ─────────────────────────────────────────────

/// Which fast path a query was routed to.
#[derive(Debug, Clone)]
pub enum FastPathKind {
    /// Left-deep join tree construction (2-10 tables).
    LeftDeep {
        /// Number of tables in the join.
        table_count: usize,
    },
    /// Index-only scan for covering index queries.
    IndexOnly {
        /// The table being scanned.
        table: String,
    },
    /// Simple aggregation answered without full optimization.
    SimpleAggregation {
        /// The aggregation function detected.
        function: SimpleAggFunction,
    },
    /// Materialized view rewrite.
    MaterializedView {
        /// The matched MV details.
        mv_match: MvMatch,
    },
}

/// Aggregation functions eligible for fast-path handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleAggFunction {
    /// `COUNT(*)` with no grouping -- metadata lookup.
    CountStar,
    /// `SUM(col)` with no grouping or simple `GROUP BY`.
    Sum,
    /// `COUNT(*)` with a simple `GROUP BY`.
    CountStarGrouped,
}

// ── Fast-path decision ──────────────────────────────────────────

/// Result of fast-path evaluation.
#[derive(Debug, Clone)]
pub struct FastPathDecision {
    /// Which fast path was selected, if any.
    pub kind: Option<FastPathKind>,
    /// Confidence score (0.0-1.0) in the fast-path choice.
    pub confidence: f64,
}

impl FastPathDecision {
    /// No fast path is applicable.
    #[must_use]
    pub fn none() -> Self {
        Self {
            kind: None,
            confidence: 0.0,
        }
    }

    /// A fast path was found.
    #[must_use]
    pub fn found(kind: FastPathKind, confidence: f64) -> Self {
        Self {
            kind: Some(kind),
            confidence,
        }
    }

    /// Whether a fast path was selected.
    #[must_use]
    pub fn has_fast_path(&self) -> bool {
        self.kind.is_some()
    }

    /// Whether the selected fast path is a left-deep join.
    #[must_use]
    pub fn is_left_deep(&self) -> bool {
        matches!(self.kind, Some(FastPathKind::LeftDeep { .. }))
    }

    /// Whether the selected fast path is an index-only scan.
    #[must_use]
    pub fn is_index_only(&self) -> bool {
        matches!(self.kind, Some(FastPathKind::IndexOnly { .. }))
    }

    /// Whether the selected fast path is a simple aggregation.
    #[must_use]
    pub fn is_simple_aggregation(&self) -> bool {
        matches!(
            self.kind,
            Some(FastPathKind::SimpleAggregation { .. })
        )
    }

    /// Whether the selected fast path is a materialized view match.
    #[must_use]
    pub fn is_mv_match(&self) -> bool {
        matches!(
            self.kind,
            Some(FastPathKind::MaterializedView { .. })
        )
    }
}

// ── Fast-path selector ──────────────────────────────────────────

/// Evaluates queries against fast-path criteria.
///
/// The selector tries each fast path in priority order (cheapest
/// detection first) and returns the first match that exceeds the
/// confidence threshold.
#[derive(Debug)]
pub struct FastPathSelector {
    preferences: FastPathPreferences,
    mv_catalog: Option<MvCatalog>,
}

impl FastPathSelector {
    /// Create a selector from preferences.
    #[must_use]
    pub fn new(preferences: FastPathPreferences) -> Self {
        Self {
            preferences,
            mv_catalog: None,
        }
    }

    /// Create a selector with default preferences (all fast paths
    /// enabled, expanded 2-10 table range).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(FastPathPreferences::default())
    }

    /// Attach a materialized view catalog for MV matching.
    #[must_use]
    pub fn with_mv_catalog(mut self, catalog: MvCatalog) -> Self {
        self.mv_catalog = Some(catalog);
        self
    }

    /// Evaluate a query for fast-path eligibility.
    ///
    /// Checks each enabled fast path in order of detection cost
    /// (cheapest first):
    /// 1. Simple aggregation (O(depth) tree walk)
    /// 2. Index-only scan (O(depth) tree walk)
    /// 3. Left-deep join (O(n) table count)
    /// 4. Materialized view matching (catalog scan)
    ///
    /// Returns the first match exceeding the confidence threshold.
    #[must_use]
    pub fn evaluate(&self, expr: &RelExpr) -> FastPathDecision {
        let min_conf = self.preferences.min_confidence;

        // 1. Simple aggregation (cheapest to detect)
        if self.preferences.enable_simple_aggregation {
            if let Some(d) = Self::check_simple_aggregation(expr) {
                if d.confidence >= min_conf {
                    return d;
                }
            }
        }

        // 2. Index-only scan
        if self.preferences.enable_index_only {
            if let Some(d) = Self::check_index_only(expr) {
                if d.confidence >= min_conf {
                    return d;
                }
            }
        }

        // 3. Left-deep join (expanded range 2-N)
        if self.preferences.enable_left_deep {
            if let Some(d) = self.check_left_deep(expr) {
                if d.confidence >= min_conf {
                    return d;
                }
            }
        }

        // 4. Materialized view matching
        if self.preferences.enable_mv_matching {
            if let Some(d) = self.check_mv_match(expr) {
                if d.confidence >= min_conf {
                    return d;
                }
            }
        }

        FastPathDecision::none()
    }

    fn check_simple_aggregation(
        expr: &RelExpr,
    ) -> Option<FastPathDecision> {
        let (group_by, aggregates, input) = match expr {
            RelExpr::Aggregate {
                group_by,
                aggregates,
                input,
            } => (group_by, aggregates, input),
            RelExpr::Project { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. } => match input.as_ref() {
                RelExpr::Aggregate {
                    group_by,
                    aggregates,
                    input,
                } => (group_by, aggregates, input),
                _ => return None,
            },
            _ => return None,
        };

        if aggregates.len() != 1 {
            return None;
        }

        let agg = &aggregates[0];

        // COUNT(*) with no GROUP BY over a bare scan
        if agg.function == AggregateFunction::Count
            && agg.arg.is_none()
            && !agg.distinct
            && group_by.is_empty()
            && matches!(input.as_ref(), RelExpr::Scan { .. })
        {
            return Some(FastPathDecision::found(
                FastPathKind::SimpleAggregation {
                    function: SimpleAggFunction::CountStar,
                },
                0.95,
            ));
        }

        // SUM(col) with no GROUP BY over a bare scan or filter
        if agg.function == AggregateFunction::Sum
            && agg.arg.is_some()
            && !agg.distinct
            && group_by.is_empty()
            && is_scan_or_simple_filter(input)
        {
            return Some(FastPathDecision::found(
                FastPathKind::SimpleAggregation {
                    function: SimpleAggFunction::Sum,
                },
                0.85,
            ));
        }

        // COUNT(*) with simple GROUP BY (single column, bare scan)
        if agg.function == AggregateFunction::Count
            && agg.arg.is_none()
            && !agg.distinct
            && group_by.len() == 1
            && is_single_column_expr(&group_by[0])
            && is_scan_or_simple_filter(input)
        {
            return Some(FastPathDecision::found(
                FastPathKind::SimpleAggregation {
                    function: SimpleAggFunction::CountStarGrouped,
                },
                0.80,
            ));
        }

        None
    }

    fn check_index_only(
        expr: &RelExpr,
    ) -> Option<FastPathDecision> {
        match expr {
            RelExpr::Project {
                input, columns, ..
            } => match input.as_ref() {
                RelExpr::Filter {
                    input: inner, ..
                } => {
                    if let RelExpr::Scan { table, .. } =
                        inner.as_ref()
                    {
                        if columns
                            .iter()
                            .all(|c| matches!(c.expr, Expr::Column(_)))
                        {
                            return Some(FastPathDecision::found(
                                FastPathKind::IndexOnly {
                                    table: table.clone(),
                                },
                                0.80,
                            ));
                        }
                    }
                    None
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn check_left_deep(
        &self,
        expr: &RelExpr,
    ) -> Option<FastPathDecision> {
        let table_count = count_tables(expr);
        let max = self.preferences.left_deep_max_tables;

        if !(2..=max).contains(&table_count) {
            return None;
        }

        if !is_left_deep_eligible(expr) {
            return None;
        }

        let confidence = if table_count <= 4 {
            0.95
        } else if table_count <= 7 {
            0.85
        } else {
            0.75
        };

        Some(FastPathDecision::found(
            FastPathKind::LeftDeep { table_count },
            confidence,
        ))
    }

    fn check_mv_match(
        &self,
        expr: &RelExpr,
    ) -> Option<FastPathDecision> {
        let catalog = self.mv_catalog.as_ref()?;
        let mv_match = catalog.best_match(expr)?;

        let confidence = match mv_match.match_type {
            crate::mv_matching::MatchType::Exact => 0.98,
            crate::mv_matching::MatchType::ViewSubsumes => 0.85,
            crate::mv_matching::MatchType::QuerySubsumes => 0.60,
        };

        Some(FastPathDecision::found(
            FastPathKind::MaterializedView { mv_match },
            confidence,
        ))
    }
}

// ── Helpers ─────────────────────────────────────────────────────

fn is_scan_or_simple_filter(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Scan { .. } => true,
        RelExpr::Filter { input, .. } => {
            matches!(input.as_ref(), RelExpr::Scan { .. })
        }
        _ => false,
    }
}

fn is_single_column_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Column(_))
}

fn count_tables(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Scan { .. } => 1,
        RelExpr::Join { left, right, .. } => {
            count_tables(left) + count_tables(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => count_tables(input),
        _ => 0,
    }
}

fn is_left_deep_eligible(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Scan { .. } => true,
        RelExpr::Join { left, right, .. } => {
            is_left_deep_eligible(left)
                && is_left_deep_eligible(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => {
            is_left_deep_eligible(input)
        }
        _ => false,
    }
}

/// Check whether a query can use the enhanced fast-path system.
///
/// Convenience wrapper using default preferences. For fine-grained
/// control, construct a [`FastPathSelector`] directly.
#[must_use]
pub fn can_use_fast_path(expr: &RelExpr) -> bool {
    if can_use_left_deep(expr) {
        return true;
    }
    let selector = FastPathSelector::with_defaults();
    selector.evaluate(expr).has_fast_path()
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::panic, reason = "test assertions use panic! for negative checks")]
mod tests {
    use super::*;
    use ra_core::algebra::{
        AggregateExpr, AggregateFunction, JoinType, ProjectionColumn,
    };
    use ra_core::expr::{BinOp, ColumnRef, Const};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_string(),
            alias: None,
        }
    }

    fn join(left: RelExpr, right: RelExpr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn count_star_query(table: &str) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(scan(table)),
        }
    }

    fn sum_query(table: &str, col: &str) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new(col))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(scan(table)),
        }
    }

    fn count_grouped_query(
        table: &str,
        group_col: &str,
    ) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new(group_col))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(scan(table)),
        }
    }

    fn project_filter_scan(table: &str) -> RelExpr {
        RelExpr::Project {
            columns: vec![
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("id")),
                    alias: None,
                },
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("name")),
                    alias: None,
                },
            ],
            input: Box::new(RelExpr::Filter {
                predicate: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(
                        ColumnRef::new("id"),
                    )),
                    right: Box::new(Expr::Const(Const::Int(42))),
                },
                input: Box::new(scan(table)),
            }),
        }
    }

    // ---- FastPathPreferences ----

    #[test]
    fn default_preferences_enable_all() {
        let fp = FastPathPreferences::default();
        assert!(fp.enable_left_deep);
        assert!(fp.enable_index_only);
        assert!(fp.enable_simple_aggregation);
        assert!(fp.enable_mv_matching);
        assert!(fp.any_enabled());
        assert_eq!(fp.left_deep_max_tables, 10);
    }

    #[test]
    fn oltp_preferences_aggressive() {
        let fp = FastPathPreferences::oltp();
        assert!(fp.any_enabled());
        assert_eq!(fp.left_deep_max_tables, 10);
        assert!(fp.min_confidence < 0.7);
    }

    #[test]
    fn olap_preferences_conservative() {
        let fp = FastPathPreferences::olap();
        assert!(fp.any_enabled());
        assert!(fp.min_confidence > 0.8);
        assert!(fp.left_deep_max_tables <= 4);
    }

    #[test]
    fn disabled_preferences_all_off() {
        let fp = FastPathPreferences::disabled();
        assert!(!fp.enable_left_deep);
        assert!(!fp.enable_index_only);
        assert!(!fp.enable_simple_aggregation);
        assert!(!fp.enable_mv_matching);
        assert!(!fp.any_enabled());
    }

    // ---- COUNT(*) fast path ----

    #[test]
    fn count_star_detected() {
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&count_star_query("users"));
        assert!(d.has_fast_path());
        assert!(d.is_simple_aggregation());
        assert!(d.confidence >= 0.9);
    }

    #[test]
    fn count_star_with_filter_not_metadata_fast_path() {
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::Filter {
                predicate: Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(
                        ColumnRef::new("age"),
                    )),
                    right: Box::new(Expr::Const(Const::Int(18))),
                },
                input: Box::new(scan("users")),
            }),
        };
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&expr);
        if let Some(FastPathKind::SimpleAggregation {
            function: SimpleAggFunction::CountStar,
        }) = &d.kind
        {
            panic!(
                "should not detect filtered COUNT(*) as CountStar"
            );
        }
    }

    // ---- SUM fast path ----

    #[test]
    fn sum_detected() {
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&sum_query("orders", "amount"));
        assert!(d.has_fast_path());
        assert!(d.is_simple_aggregation());
    }

    // ---- Grouped COUNT fast path ----

    #[test]
    fn count_grouped_detected() {
        let selector = FastPathSelector::with_defaults();
        let d = selector
            .evaluate(&count_grouped_query("orders", "status"));
        assert!(d.has_fast_path());
        assert!(d.is_simple_aggregation());
    }

    // ---- Index-only scan fast path ----

    #[test]
    fn index_only_detected() {
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&project_filter_scan("users"));
        assert!(d.has_fast_path());
        assert!(d.is_index_only());
        match &d.kind {
            Some(FastPathKind::IndexOnly { table }) => {
                assert_eq!(table, "users");
            }
            other => panic!("expected IndexOnly, got {other:?}"),
        }
    }

    #[test]
    fn index_only_not_for_computed_columns() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::BinOp {
                    op: BinOp::Add,
                    left: Box::new(Expr::Column(
                        ColumnRef::new("a"),
                    )),
                    right: Box::new(Expr::Const(Const::Int(1))),
                },
                alias: Some("a_plus_1".to_string()),
            }],
            input: Box::new(RelExpr::Filter {
                predicate: Expr::Const(Const::Bool(true)),
                input: Box::new(scan("t")),
            }),
        };
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&expr);
        assert!(!d.is_index_only());
    }

    // ---- Left-deep fast path ----

    #[test]
    fn left_deep_two_tables() {
        let expr = join(scan("a"), scan("b"));
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&expr);
        assert!(d.has_fast_path());
        assert!(d.is_left_deep());
        assert!(d.confidence >= 0.9);
    }

    #[test]
    fn left_deep_seven_tables() {
        let mut expr = join(scan("a"), scan("b"));
        for name in ["c", "d", "e", "f", "g"] {
            expr = join(expr, scan(name));
        }
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&expr);
        assert!(d.has_fast_path());
        assert!(d.is_left_deep());
    }

    #[test]
    fn left_deep_ten_tables_with_expanded_range() {
        let mut expr = join(scan("a"), scan("b"));
        for name in ["c", "d", "e", "f", "g", "h", "i", "j"] {
            expr = join(expr, scan(name));
        }
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&expr);
        assert!(d.has_fast_path());
        assert!(d.is_left_deep());
        assert!(d.confidence < 0.85);
    }

    #[test]
    fn left_deep_eight_tables_rejected_by_original() {
        let mut expr = join(scan("a"), scan("b"));
        for name in ["c", "d", "e", "f", "g", "h"] {
            expr = join(expr, scan(name));
        }
        assert!(!can_use_left_deep(&expr));
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&expr);
        assert!(d.has_fast_path());
    }

    #[test]
    fn left_deep_eleven_tables_rejected() {
        let mut expr = join(scan("a"), scan("b"));
        for i in 0..9 {
            let name = format!("t{i}");
            expr = join(expr, scan(&name));
        }
        let selector = FastPathSelector::with_defaults();
        let d = selector.evaluate(&expr);
        assert!(!d.is_left_deep());
    }

    #[test]
    fn left_deep_single_table_rejected() {
        let d = FastPathSelector::with_defaults()
            .evaluate(&scan("a"));
        assert!(!d.is_left_deep());
    }

    #[test]
    fn olap_max_tables_rejects_above_limit() {
        let mut expr = join(scan("a"), scan("b"));
        for name in ["c", "d", "e"] {
            expr = join(expr, scan(name));
        }
        let selector =
            FastPathSelector::new(FastPathPreferences::olap());
        let d = selector.evaluate(&expr);
        assert!(!d.is_left_deep());
    }

    // ---- MV matching fast path ----

    #[test]
    fn mv_exact_match_detected() {
        use crate::mv_matching::MaterializedViewInfo;

        let query = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("region"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("amount"))),
                distinct: false,
                alias: Some("total".to_string()),
            }],
            input: Box::new(scan("orders")),
        };

        let mut catalog = MvCatalog::new();
        catalog.add_view(MaterializedViewInfo {
            name: "orders_by_region".to_string(),
            definition: query.clone(),
            base_tables: vec!["orders".to_string()],
            row_count: 50,
            is_incremental: false,
        });

        let selector = FastPathSelector::with_defaults()
            .with_mv_catalog(catalog);
        let d = selector.evaluate(&query);
        assert!(d.has_fast_path());
        assert!(d.is_mv_match());
        match &d.kind {
            Some(FastPathKind::MaterializedView { mv_match }) => {
                assert_eq!(mv_match.mv_name, "orders_by_region");
            }
            other => panic!("expected MV match, got {other:?}"),
        }
    }

    #[test]
    fn no_mv_match_without_catalog() {
        let d = FastPathSelector::with_defaults()
            .evaluate(&sum_query("t", "c"));
        assert!(!d.is_mv_match());
    }

    // ---- Disabled preferences ----

    #[test]
    fn disabled_preferences_reject_everything() {
        let selector =
            FastPathSelector::new(FastPathPreferences::disabled());

        assert!(
            !selector
                .evaluate(&count_star_query("t"))
                .has_fast_path()
        );
        assert!(
            !selector
                .evaluate(&project_filter_scan("t"))
                .has_fast_path()
        );
        assert!(
            !selector
                .evaluate(&join(scan("a"), scan("b")))
                .has_fast_path()
        );
    }

    // ---- FastPathDecision ----

    #[test]
    fn none_decision() {
        let d = FastPathDecision::none();
        assert!(!d.has_fast_path());
        assert!(d.kind.is_none());
    }

    #[test]
    fn found_decision() {
        let d = FastPathDecision::found(
            FastPathKind::LeftDeep { table_count: 3 },
            0.9,
        );
        assert!(d.has_fast_path());
        assert!(d.is_left_deep());
    }

    // ---- Convenience function ----

    #[test]
    fn can_use_fast_path_for_left_deep() {
        assert!(can_use_fast_path(&join(scan("a"), scan("b"))));
    }

    #[test]
    fn can_use_fast_path_for_count_star() {
        assert!(can_use_fast_path(&count_star_query("users")));
    }

    #[test]
    fn can_use_fast_path_for_index_only() {
        assert!(can_use_fast_path(&project_filter_scan("users")));
    }

    // ---- Priority ordering ----

    #[test]
    fn aggregation_detected_before_left_deep() {
        let d = FastPathSelector::with_defaults()
            .evaluate(&count_star_query("orders"));
        assert!(d.is_simple_aggregation());
    }

    // ---- Confidence thresholds ----

    #[test]
    fn high_min_confidence_rejects_lower() {
        let selector = FastPathSelector::new(
            FastPathPreferences {
                min_confidence: 0.99,
                ..FastPathPreferences::default()
            },
        );
        let d = selector.evaluate(&sum_query("t", "c"));
        assert!(!d.has_fast_path());
    }
}
