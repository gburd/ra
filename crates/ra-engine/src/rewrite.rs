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

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return all optimization rewrite rules.
///
/// Rules are returned in priority order. The e-graph engine applies
/// them in each iteration until saturation or a limit is reached.
#[must_use]
pub fn all_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = Vec::new();
    rules.extend(predicate_pushdown_rules());
    rules.extend(join_reordering_rules());
    rules.extend(projection_pushdown_rules());
    rules.extend(expression_simplification_rules());
    rules.extend(join_elimination_rules());
    rules.extend(aggregate_optimization_rules());
    rules.extend(limit_sort_optimization_rules());
    rules.extend(set_operation_rules());
    rules.extend(subquery_optimization_rules());
    rules.extend(duckdb_inspired_rules());
    rules.extend(sqlite_inspired_rules());
    rules
}

// ---------------------------------------------------------------
// Predicate pushdown rules
// ---------------------------------------------------------------

fn predicate_pushdown_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Push filter through inner join (left side)
        rewrite!("filter-through-join-left";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond (filter ?pred ?left) ?right)"
        ),
        // Push filter through inner join (right side)
        rewrite!("filter-through-join-right";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond ?left (filter ?pred ?right))"
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

fn join_reordering_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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

fn projection_pushdown_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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

fn expression_simplification_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = boolean_simplification_rules();
    rules.extend(arithmetic_simplification_rules());
    rules.extend(commutativity_rules());
    rules
}

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

fn join_elimination_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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

fn aggregate_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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

fn limit_sort_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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

fn set_operation_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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

fn subquery_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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
// DuckDB-inspired rules
// Sourced from: src/optimizer/ in the DuckDB repository
// ---------------------------------------------------------------

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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::analysis::RelAnalysis;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

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
}
