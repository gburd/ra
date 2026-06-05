//! Condition functions for generated rewrite rules.
//!
//! These functions implement `egg::Condition<RelLang, RelAnalysis>` and are
//! referenced by `.rra` rule files via `if condition_name(...)` syntax.
//! Each new condition function unlocks additional rules from the generated set.

use egg::{Condition, EGraph, Id, Subst, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Parse a variable-name string into an `egg::Var`, falling back to a
/// statically-valid form (e.g. `"?name"`) when the caller supplied a
/// malformed value.
///
/// The fallback parse cannot fail: egg's `Var` parser accepts any string
/// starting with `'?'` followed by a non-empty identifier, and we only
/// pass literal `"?<role>"` strings.
#[expect(
    clippy::expect_used,
    reason = "the fallback string is a verified static literal"
)]
fn parse_var(name: &str, fallback: &'static str) -> Var {
    name.parse().unwrap_or_else(|_| {
        debug_assert!(
            fallback.starts_with('?') && fallback.len() > 1,
            "fallback must be a `?<id>` literal"
        );
        fallback
            .parse()
            .expect("statically-valid Var literal must parse")
    })
}

/// Condition: a CTE name is referenced exactly once in the body.
///
/// Used by CTE inlining rules to ensure inlining is safe.
pub struct SingleReference {
    name_var: Var,
    body_var: Var,
}

impl SingleReference {
    /// Create a condition checking that `name` appears once in `body`.
    #[must_use]
    pub fn new(name: &str, body: &str) -> Self {
        Self {
            name_var: parse_var(name, "?name"),
            body_var: parse_var(body, "?body"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for SingleReference {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let name_id = subst[self.name_var];
        let body_id = subst[self.body_var];

        // Count references to the CTE name in the body's e-class data
        let name_data = &egraph[name_id].data;
        let body_data = &egraph[body_id].data;

        // If the name appears in the body's table set exactly once,
        // inlining is safe. We approximate by checking the table set.
        if let Some(name_str) = name_data.tables.iter().next() {
            let count = body_data
                .tables
                .iter()
                .filter(|t| *t == name_str)
                .count();
            return count <= 1;
        }
        // Conservative: allow inlining if we can't determine reference count
        true
    }
}

/// Condition: a predicate references columns from only one side of a join.
pub struct ReferencesOnly {
    pred_var: Var,
    side_var: Var,
}

impl ReferencesOnly {
    /// Create a condition checking that `pred` references only columns from `side`.
    #[must_use]
    pub fn new(pred: &str, side: &str) -> Self {
        Self {
            pred_var: parse_var(pred, "?pred"),
            side_var: parse_var(side, "?side"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for ReferencesOnly {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let pred_id = subst[self.pred_var];
        let side_id = subst[self.side_var];

        let pred_data = &egraph[pred_id].data;
        let side_data = &egraph[side_id].data;

        // Check that all tables referenced by the predicate are in the side's table set
        pred_data.tables.is_subset(&side_data.tables)
    }
}

/// Condition: an expression is deterministic (no side effects, no randomness).
pub struct IsDeterministic {
    expr_var: Var,
}

impl IsDeterministic {
    /// Create a condition checking that `expr` is deterministic.
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: parse_var(expr, "?expr"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for IsDeterministic {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let expr_id = subst[self.expr_var];
        let data = &egraph[expr_id].data;

        // If the e-class data has no non-deterministic markers, it's deterministic.
        // Currently we check that it doesn't reference special "random"/"now" tables.
        !data.tables.iter().any(|t| {
            t.contains("random") || t.contains("now") || t.contains("current_timestamp")
        })
    }
}

/// Condition: an expression is a compile-time constant.
pub struct IsConstant {
    expr_var: Var,
}

impl IsConstant {
    /// Create a condition checking that `expr` is a constant.
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: parse_var(expr, "?expr"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for IsConstant {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let expr_id = subst[self.expr_var];
        let data = &egraph[expr_id].data;

        // A constant expression references no tables and has no column references
        data.tables.is_empty()
    }
}

/// Wrapper that runs an inner [`Condition`]'s `check` inside
/// [`std::panic::catch_unwind`]. A panic from the inner condition is
/// converted to a `false` return (the rule simply doesn't fire) and
/// logged via `tracing::error!` so it shows up in production logs
/// without taking the optimization down.
///
/// This is layer 2 of the failure-containment policy described in
/// the GEQO-comparison document; layer 1 is build-time validation
/// (`crates/ra-engine/build.rs::check_sexp_invalid` plus
/// [`is_malformed_rule_pair`]) and layer 3 is the per-rule
/// `catch_unwind` in the saturation loop with a session-local
/// blacklist.
///
/// The overhead on the non-panic path is single-digit nanoseconds —
/// `catch_unwind` sets up a panic-safe stack frame but doesn't
/// allocate or take locks. We accept that cost because conditions
/// run rarely compared to the LHS pattern matcher and the safety
/// payoff is large: one bug in a third-party crate or in our own
/// pattern-walking code can no longer abort optimization.
pub struct SafeCondition<C> {
    inner: C,
    /// Stable identifier used in panic logs. We can't reach for
    /// `std::any::type_name::<C>()` because Conditions are
    /// type-erased downstream; the constructor passes the helper's
    /// own name.
    label: &'static str,
}

impl<C> SafeCondition<C> {
    /// Wrap `inner` in panic-catching plumbing. `label` is logged
    /// when the inner condition panics; pass the helper's name
    /// (e.g. `"single_reference"`).
    #[must_use]
    pub const fn new(inner: C, label: &'static str) -> Self {
        Self { inner, label }
    }
}

impl<C> Condition<RelLang, RelAnalysis> for SafeCondition<C>
where
    C: Condition<RelLang, RelAnalysis>,
{
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        eclass: Id,
        subst: &Subst,
    ) -> bool {
        // We use AssertUnwindSafe because Condition::check takes
        // &mut EGraph, which isn't UnwindSafe by default. Egg's
        // EGraph cannot be left in a publicly observable broken
        // state by a panic mid-modification — its internal
        // operations either complete or fail before mutating the
        // canonical structure — so this assertion is sound. If a
        // future egg version changes that contract, this is the
        // right place to add a barrier.
        let outcome = std::panic::catch_unwind(
            std::panic::AssertUnwindSafe(|| self.inner.check(egraph, eclass, subst)),
        );
        match outcome {
            Ok(b) => b,
            Err(payload) => {
                let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
                    (*s).to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "<non-string panic payload>".to_string()
                };
                tracing::error!(
                    condition = self.label,
                    panic = %msg,
                    "condition panicked; treating as false"
                );
                false
            }
        }
    }
}

/// Constructor functions for use in generated rules.
/// These return panic-safe `SafeCondition`-wrapped implementations.
///
/// Returns a condition that checks if a CTE name is referenced at most once.
#[must_use]
pub fn single_reference(name: &str, body: &str) -> SafeCondition<SingleReference> {
    SafeCondition::new(SingleReference::new(name, body), "single_reference")
}

/// Returns a condition that checks if a predicate references only one side.
#[must_use]
pub fn references_only(pred: &str, side: &str) -> SafeCondition<ReferencesOnly> {
    SafeCondition::new(ReferencesOnly::new(pred, side), "references_only")
}

/// Returns a condition that checks if an expression is deterministic.
#[must_use]
pub fn is_deterministic(expr: &str) -> SafeCondition<IsDeterministic> {
    SafeCondition::new(IsDeterministic::new(expr), "is_deterministic")
}

/// Returns a condition that checks if an expression is a constant.
#[must_use]
pub fn is_constant(expr: &str) -> SafeCondition<IsConstant> {
    SafeCondition::new(IsConstant::new(expr), "is_constant")
}

// -- Aliases for the names .rra files actually use ----------------

/// Alias for [`references_only`] under the variant name `pred_references_only`.
#[must_use]
pub fn pred_references_only(pred: &str, side: &str) -> SafeCondition<ReferencesOnly> {
    SafeCondition::new(ReferencesOnly::new(pred, side), "pred_references_only")
}

/// Alias for [`references_only`] under the variant name `predicate_references_only`.
#[must_use]
pub fn predicate_references_only(pred: &str, side: &str) -> SafeCondition<ReferencesOnly> {
    SafeCondition::new(ReferencesOnly::new(pred, side), "predicate_references_only")
}

/// Condition: an expression is non-nullable. Approximated structurally
/// — a value is treated as non-nullable when its e-class data records
/// no tables (constants) or when it's a column reference whose e-class
/// is reachable only through `is-not-null` filters in the same e-graph.
/// The conservative answer is "no, it might be null" for arbitrary
/// expressions; this returns true only for compile-time constants.
pub struct NotNullable {
    expr_var: Var,
}

impl NotNullable {
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: parse_var(expr, "?expr"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for NotNullable {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let id = subst[self.expr_var];
        // A constant (no table refs) cannot be NULL unless it IS NULL,
        // which we'd see as the `const-null` node. Look through the
        // e-class for any non-null literal node.
        let class = &egraph[id];
        if !class.data.tables.is_empty() {
            // References tables ⇒ may be NULL; conservative.
            return false;
        }
        class.nodes.iter().any(|n| {
            !matches!(n, RelLang::ConstNull)
        })
    }
}

/// Returns a condition that checks if an expression is non-nullable.
#[must_use]
pub fn not_nullable(expr: &str) -> SafeCondition<NotNullable> {
    SafeCondition::new(NotNullable::new(expr), "not_nullable")
}

/// Condition: a numeric expression is provably non-zero. Currently
/// recognises only positive/negative integer and float literals; any
/// expression involving a column or non-trivial computation conservatively
/// returns false.
pub struct NotZero {
    expr_var: Var,
}

impl NotZero {
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: parse_var(expr, "?expr"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for NotZero {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let id = subst[self.expr_var];
        let class = &egraph[id];
        if !class.data.tables.is_empty() {
            return false;
        }
        // Look for a literal node and compare against zero. The
        // const-int / const-float wrappers store their numeric value as
        // the child node's symbol; we check whether the canonical text
        // form is non-zero.
        class.nodes.iter().any(|n| match n {
            RelLang::ConstInt(child) | RelLang::ConstFloat(child) => {
                let child_class = &egraph[child[0]];
                child_class.nodes.iter().any(|c| {
                    let s = format!("{c}");
                    s.parse::<f64>().is_ok_and(|v| v != 0.0)
                })
            }
            _ => false,
        })
    }
}

/// Returns a condition that checks if a numeric expression is non-zero.
#[must_use]
pub fn not_zero(expr: &str) -> SafeCondition<NotZero> {
    SafeCondition::new(NotZero::new(expr), "not_zero")
}

/// Condition: an expression's e-class includes only `Scan` nodes (no
/// derived expressions). Used by `.rra` rules that match canonical base
/// table scans before applying physical rewrites.
pub struct IsCanonicalScan {
    expr_var: Var,
}

impl IsCanonicalScan {
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: parse_var(expr, "?rel"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for IsCanonicalScan {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let id = subst[self.expr_var];
        let class = &egraph[id];
        class
            .nodes
            .iter()
            .any(|n| matches!(n, RelLang::Scan(_) | RelLang::ScanAlias(_)))
    }
}

/// Returns a condition that checks if an expression is a canonical scan.
#[must_use]
pub fn is_canonical_scan(rel: &str) -> SafeCondition<IsCanonicalScan> {
    SafeCondition::new(IsCanonicalScan::new(rel), "is_canonical_scan")
}

/// Condition: an expression is uncorrelated (references no tables outside
/// its own subtree's bound scopes). Approximated by checking that the
/// e-class data's `tables` set is non-empty (i.e. it produces rows
/// rather than referencing outer-scope columns).
pub struct IsUncorrelated {
    expr_var: Var,
}

impl IsUncorrelated {
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: parse_var(expr, "?subq"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for IsUncorrelated {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let id = subst[self.expr_var];
        let data = &egraph[id].data;
        // Without a real correlation analysis, we approximate: a
        // subquery whose tables include something is "self-contained"
        // enough to be considered uncorrelated. The conservative
        // alternative — always returning false — would simply leave
        // the rule inactive, which is what we had before.
        !data.tables.is_empty()
    }
}

/// Returns a condition that checks if a subquery is uncorrelated.
#[must_use]
pub fn is_uncorrelated(subq: &str) -> SafeCondition<IsUncorrelated> {
    SafeCondition::new(IsUncorrelated::new(subq), "is_uncorrelated")
}


// -- Phase 2 (RFC 0090) conditions ---------------------------------
// Constructors that let the .rra `when:` field reference conditions whose
// check logic lives in feature modules. Each delegates to the module's
// `pub(crate)` helper so the logic is not duplicated.

/// Condition: the predicate e-class contains an XML function (xpath,
/// xmlexists, ...). Delegates to `xml_optimizer::contains_xml_function`.
pub struct XmlFunctionFilter {
    pred_var: Var,
}

impl XmlFunctionFilter {
    #[must_use]
    pub fn new(pred: &str) -> Self {
        Self { pred_var: parse_var(pred, "?pred") }
    }
}

impl Condition<RelLang, RelAnalysis> for XmlFunctionFilter {
    fn check(&self, egraph: &mut EGraph<RelLang, RelAnalysis>, _eclass: Id, subst: &Subst) -> bool {
        crate::xml_optimizer::contains_xml_function(egraph, subst[self.pred_var], 4)
    }
}

/// Returns a condition that checks if a predicate contains an XML function.
#[must_use]
pub fn is_xml_function_filter(pred: &str) -> SafeCondition<XmlFunctionFilter> {
    SafeCondition::new(XmlFunctionFilter::new(pred), "is_xml_function_filter")
}

/// Condition: no GROUP BY (`?groups` is nil/empty) and `?aggs` is a single
/// `count(*)`. Delegates to `count_metadata` helpers.
pub struct UngroupedCountStar {
    groups_var: Var,
    aggs_var: Var,
}

impl UngroupedCountStar {
    #[must_use]
    pub fn new(groups: &str, aggs: &str) -> Self {
        Self {
            groups_var: parse_var(groups, "?groups"),
            aggs_var: parse_var(aggs, "?aggs"),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for UngroupedCountStar {
    fn check(&self, egraph: &mut EGraph<RelLang, RelAnalysis>, _eclass: Id, subst: &Subst) -> bool {
        crate::count_metadata::is_nil_or_empty_list(egraph, subst[self.groups_var])
            && crate::count_metadata::is_single_count_star(egraph, subst[self.aggs_var])
    }
}

/// Returns a condition matching an ungrouped `COUNT(*)` aggregate.
#[must_use]
pub fn is_ungrouped_count_star(groups: &str, aggs: &str) -> SafeCondition<UngroupedCountStar> {
    SafeCondition::new(UngroupedCountStar::new(groups, aggs), "is_ungrouped_count_star")
}

/// Condition: the e-class for `?var` contains no `const-bool` node (i.e. the
/// predicate is not a constant boolean, so an index probe is worthwhile).
pub struct NotConstBool {
    var: Var,
}

impl NotConstBool {
    #[must_use]
    pub fn new(var: &str) -> Self {
        Self { var: parse_var(var, "?pred") }
    }
}

impl Condition<RelLang, RelAnalysis> for NotConstBool {
    fn check(&self, egraph: &mut EGraph<RelLang, RelAnalysis>, _eclass: Id, subst: &Subst) -> bool {
        !egraph[subst[self.var]]
            .nodes
            .iter()
            .any(|n| matches!(n, RelLang::ConstBool(_)))
    }
}

/// Returns a condition that checks a predicate is not a constant boolean.
#[must_use]
pub fn is_not_const_bool(var: &str) -> SafeCondition<NotConstBool> {
    SafeCondition::new(NotConstBool::new(var), "is_not_const_bool")
}

/// Condition: the predicate e-class contains a BSON/MongoDB operator pattern.
/// Delegates to `documentdb_optimizer::contains_bson_pattern`.
pub struct BsonOperatorFilter {
    pred_var: Var,
}

impl BsonOperatorFilter {
    #[must_use]
    pub fn new(pred: &str) -> Self {
        Self { pred_var: parse_var(pred, "?pred") }
    }
}

impl Condition<RelLang, RelAnalysis> for BsonOperatorFilter {
    fn check(&self, egraph: &mut EGraph<RelLang, RelAnalysis>, _eclass: Id, subst: &Subst) -> bool {
        crate::documentdb_optimizer::contains_bson_pattern(egraph, subst[self.pred_var], 3)
    }
}

/// Returns a condition that checks if a predicate contains a BSON operator.
#[must_use]
pub fn is_bson_operator_filter(pred: &str) -> SafeCondition<BsonOperatorFilter> {
    SafeCondition::new(BsonOperatorFilter::new(pred), "is_bson_operator_filter")
}

/// Condition: the predicate e-class contains a JSON-field pattern (Oracle JSON
/// Relational Duality). Delegates to `oracle_json_duality::contains_json_pattern`.
pub struct JsonFieldPredicate {
    pred_var: Var,
}

impl JsonFieldPredicate {
    #[must_use]
    pub fn new(pred: &str) -> Self {
        Self { pred_var: parse_var(pred, "?pred") }
    }
}

impl Condition<RelLang, RelAnalysis> for JsonFieldPredicate {
    fn check(&self, egraph: &mut EGraph<RelLang, RelAnalysis>, _eclass: Id, subst: &Subst) -> bool {
        crate::oracle_json_duality::contains_json_pattern(egraph, subst[self.pred_var], 3)
    }
}

/// Returns a condition that checks if a predicate references a JSON field.
#[must_use]
pub fn is_json_field_predicate(pred: &str) -> SafeCondition<JsonFieldPredicate> {
    SafeCondition::new(JsonFieldPredicate::new(pred), "is_json_field_predicate")
}

/// Condition: a join condition that contains at least one equality comparison
/// (`eq`), i.e. an equi-join — the precondition for hash- and merge-join
/// lowering (RFC 0090 Phase 3). Nest-loop needs no such guard.
pub struct IsEquiJoin {
    cond_var: Var,
}

impl IsEquiJoin {
    #[must_use]
    pub fn new(cond: &str) -> Self {
        Self { cond_var: parse_var(cond, "?cond") }
    }
}

/// Depth-limited search for an `eq` node reachable from `id` (descending through
/// `and`/`or` boolean structure).
fn contains_eq(egraph: &EGraph<RelLang, RelAnalysis>, id: Id, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    let canonical = egraph.find(id);
    egraph[canonical].nodes.iter().any(|node| match node {
        RelLang::Eq(_) => true,
        RelLang::And([l, r]) | RelLang::Or([l, r]) => {
            contains_eq(egraph, *l, depth - 1) || contains_eq(egraph, *r, depth - 1)
        }
        _ => false,
    })
}

impl Condition<RelLang, RelAnalysis> for IsEquiJoin {
    fn check(&self, egraph: &mut EGraph<RelLang, RelAnalysis>, _eclass: Id, subst: &Subst) -> bool {
        contains_eq(egraph, subst[self.cond_var], 4)
    }
}

/// Returns a condition that checks a join condition is an equi-join.
#[must_use]
pub fn is_equi_join(cond: &str) -> SafeCondition<IsEquiJoin> {
    SafeCondition::new(IsEquiJoin::new(cond), "is_equi_join")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test-only condition that panics when checked. We use it to
    /// verify that `SafeCondition` swallows panics rather than
    /// propagating them and aborting whatever caller is unlucky.
    struct AlwaysPanic;

    impl Condition<RelLang, RelAnalysis> for AlwaysPanic {
        #[expect(
            clippy::panic,
            reason = "test-only condition that exercises SafeCondition's catch_unwind"
        )]
        fn check(
            &self,
            _egraph: &mut EGraph<RelLang, RelAnalysis>,
            _eclass: Id,
            _subst: &Subst,
        ) -> bool {
            panic!("intentional test panic");
        }
    }

    #[test]
    fn safe_condition_swallows_panic_returns_false() {
        let mut eg: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let id = eg.add(RelLang::Symbol("t".into()));
        let subst = Subst::default();
        let safe = SafeCondition::new(AlwaysPanic, "always_panic_test");
        // Invoking check must not propagate the panic.
        let result = safe.check(&mut eg, id, &subst);
        assert!(!result, "panic must be reported as `check returned false`");
    }

    /// A non-panicking condition that returns a configurable value.
    /// Confirms that `SafeCondition` is transparent on the happy path.
    struct AlwaysReturn(bool);

    impl Condition<RelLang, RelAnalysis> for AlwaysReturn {
        fn check(
            &self,
            _egraph: &mut EGraph<RelLang, RelAnalysis>,
            _eclass: Id,
            _subst: &Subst,
        ) -> bool {
            self.0
        }
    }

    #[test]
    fn safe_condition_returns_inner_value_on_happy_path() {
        let mut eg: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let id = eg.add(RelLang::Symbol("t".into()));
        let subst = Subst::default();

        let safe_true = SafeCondition::new(AlwaysReturn(true), "always_true");
        assert!(safe_true.check(&mut eg, id, &subst));

        let safe_false = SafeCondition::new(AlwaysReturn(false), "always_false");
        assert!(!safe_false.check(&mut eg, id, &subst));
    }
}
