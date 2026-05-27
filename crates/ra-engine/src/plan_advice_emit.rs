//! Emit plan advice from a finished [`RelExpr`].
//!
//! This is the inverse of `ra_plan_advice::parse_advice`. Walking
//! the optimized expression yields an `Advice` document that, when
//! fed back to the optimizer with the same inputs, reproduces the
//! same plan.
//!
//! # What we emit
//!
//! Ra's `RelExpr` is a *logical* tree. It does not carry physical
//! choices (index vs. seq scan, hash vs. merge join). The emitter
//! therefore covers the subset of advice tags that are well-defined
//! over a logical plan:
//!
//! - **`JOIN_ORDER(...)`** — derived from the shape of the join
//!   tree. Outer-deep chains emit a flat target list; bushy joins
//!   emit `(...)` ordered sublists. We never emit `{...}`
//!   unordered sublists because we always know exactly which side
//!   of each join is which.
//! - **`SEQ_SCAN(t1 t2 ...)`** — every base table scan. PG omits
//!   the tag when the planner had no other choice, but Ra's
//!   pragma is round-trip safety: we always emit `SEQ_SCAN` so
//!   that re-feeding the advice deterministically constrains the
//!   scan strategy when the consumer is a physical-plan-aware
//!   optimizer (PG, or a future Ra variant).
//!
//! # What we don't emit (yet)
//!
//! - `INDEX_SCAN` / `INDEX_ONLY_SCAN` — `RelExpr` doesn't carry the
//!   chosen index. This is a logical-vs-physical gap, not a port
//!   gap.
//! - `HASH_JOIN` / `MERGE_JOIN_*` / `NESTED_LOOP_*` — same reason.
//! - `GATHER` / `GATHER_MERGE` — Ra has no parallel-query
//!   representation in `RelExpr`.
//! - `PARTITIONWISE`, `SEMIJOIN_*`, `FOREIGN_JOIN` — not yet
//!   exercised by Ra's e-graph rules in a round-trippable way.
//! - `DO_NOT_SCAN` — used by PG to disambiguate
//!   `AlternativeSubPlan` choices that Ra doesn't model.
//!
//! When Ra grows physical operators, this module is the right place
//! to extend.

use ra_core::algebra::RelExpr;
use ra_plan_advice::ast::{
    Advice, AdviceItem, AdviceTag, AdviceTarget, RelationIdentifier,
};

/// Walk `expr` and emit an [`Advice`] document.
///
/// The output reflects only the structural decisions Ra made
/// (join order, set of base tables); see the module docs for what
/// is and isn't covered.
#[must_use]
pub fn emit_advice(expr: &RelExpr) -> Advice {
    let mut out = Advice::new();

    // JOIN_ORDER for every top-level join problem in the
    // expression tree. The walker below collects join problems
    // and recurses into subqueries, mirroring how PG groups
    // advice by PlannerInfo.
    let mut join_orders = Vec::new();
    collect_join_orders(expr, &mut join_orders);
    for jo in join_orders {
        if jo.children.is_empty() {
            continue;
        }
        out.push(AdviceItem {
            tag: AdviceTag::JoinOrder,
            targets: vec![jo],
        });
    }

    // SEQ_SCAN(t1 t2 ...) for every base relation reachable in
    // the expression. We render duplicates with #N occurrence
    // numbers, matching PG's behavior for queries that mention
    // the same alias more than once.
    let scans = collect_scans(expr);
    if !scans.is_empty() {
        out.push(AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: scans
                .into_iter()
                .map(AdviceTarget::identifier)
                .collect(),
        });
    }

    out
}

/// Walk `expr`, pushing one [`AdviceTarget`] per top-level join
/// problem. A "join problem" is a maximal subtree whose root is
/// `RelExpr::Join`. We don't descend below set operations, CTE
/// boundaries, or aggregates because those start new join
/// problems in PG's `PlannerInfo` model.
fn collect_join_orders(expr: &RelExpr, out: &mut Vec<AdviceTarget>) {
    match expr {
        RelExpr::Join { .. } => {
            // A join is the root of a join problem. Render its
            // tree shape and stop here — child Join nodes inside
            // the same problem are folded into the structure.
            if let Some(target) = render_join(expr) {
                out.push(target);
            }
        }
        // Operators that wrap a single subexpression — recurse.
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => collect_join_orders(input, out),
        // Set ops introduce new join problems on each branch.
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_join_orders(left, out);
            collect_join_orders(right, out);
        }
        RelExpr::CTE { definition, body, .. } => {
            collect_join_orders(definition, out);
            collect_join_orders(body, out);
        }
        RelExpr::RecursiveCTE { base_case, recursive_case, body, .. } => {
            collect_join_orders(base_case, out);
            collect_join_orders(recursive_case, out);
            collect_join_orders(body, out);
        }
        // Leaves: nothing to emit.
        RelExpr::Scan { .. }
        | RelExpr::Values { .. }
        | RelExpr::Unnest { .. }
        | RelExpr::MultiUnnest { .. }
        | RelExpr::TableFunction { .. } => {}
        // Anything else: fall back to recursing through children
        // by descending into the obvious pieces. RelExpr has many
        // variants (RowPattern, IncrementalSort, etc.) that don't
        // typically appear in join problems but might still
        // contain joins beneath them.
        other => {
            for child in other.children() {
                collect_join_orders(child, out);
            }
        }
    }
}

/// Render one join subtree as an [`AdviceTarget`].
///
/// - A leaf scan becomes a bare [`AdviceTargetKind::Identifier`].
/// - A join with a leaf inner side and any outer side renders as
///   `(outer inner)` flattened: the outer side's children are
///   inlined when the outer is itself a join, mirroring PG's
///   "outer-deep" canonical form.
/// - A join with a non-leaf inner side renders as `(outer (...inner))`
///   — bushy join trees use parenthesised sublists.
fn render_join(expr: &RelExpr) -> Option<AdviceTarget> {
    match expr {
        RelExpr::Join { left, right, .. } => {
            let mut children = Vec::new();
            // Outer side: inline children if it's an outer-deep
            // join, otherwise recurse as a sublist.
            inline_outer_into(left, &mut children);
            // Inner side: always one slot.
            if let Some(inner_target) = render_join_member(right) {
                children.push(inner_target);
            }
            // PG's grammar requires JOIN_ORDER targets to have at
            // least two children when they're sublists, so an
            // empty join (which shouldn't happen for legal queries)
            // is silently dropped here.
            if children.len() < 2 {
                return None;
            }
            Some(AdviceTarget::ordered(children))
        }
        _ => render_join_member(expr),
    }
}

/// Inline the children of an outer-deep join chain into `out`.
///
/// `JOIN_ORDER(a b c)` is the outer-deep form of
/// `((a JOIN b) JOIN c)`. We detect that shape by recursing on
/// the left side until we hit a non-Join, then appending the
/// inner side at each level.
fn inline_outer_into(expr: &RelExpr, out: &mut Vec<AdviceTarget>) {
    if let RelExpr::Join { left, right, .. } = expr {
        inline_outer_into(left, out);
        if let Some(inner_target) = render_join_member(right) {
            out.push(inner_target);
        }
    } else if let Some(t) = render_join_member(expr) {
        out.push(t);
    }
}

/// One side of a join. A bare scan becomes an identifier; an
/// inner join becomes a parenthesised sublist.
fn render_join_member(expr: &RelExpr) -> Option<AdviceTarget> {
    match expr {
        RelExpr::Scan { table, alias } => {
            let alias_name = alias.clone().unwrap_or_else(|| table.clone());
            Some(AdviceTarget::identifier(RelationIdentifier::simple(alias_name)))
        }
        RelExpr::Join { .. } => render_join(expr),
        // Operators that wrap a single subexpression — for join
        // membership purposes treat as the underlying base. This
        // is best-effort: a `Filter` over a `Scan` round-trips as
        // the scan alias; a `Filter` over a `Join` flattens to a
        // sublist.
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => render_join_member(input),
        _ => None,
    }
}

/// Walk `expr` and collect every base-table scan, in
/// left-to-right order. Each occurrence of an alias gets a
/// distinct `#N` occurrence number when the same alias appears
/// more than once.
fn collect_scans(expr: &RelExpr) -> Vec<RelationIdentifier> {
    fn walk(
        expr: &RelExpr,
        out: &mut Vec<RelationIdentifier>,
        counts: &mut std::collections::HashMap<String, u32>,
    ) {
        match expr {
            RelExpr::Scan { table, alias } => {
                let name = alias.clone().unwrap_or_else(|| table.clone());
                let count = counts.entry(name.clone()).or_insert(0);
                *count += 1;
                out.push(RelationIdentifier {
                    alias_name: name,
                    occurrence: *count,
                    partition_schema: None,
                    partition_name: None,
                    plan_name: None,
                });
            }
            other => {
                for child in other.children() {
                    walk(child, out, counts);
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut counts = std::collections::HashMap::new();
    walk(expr, &mut out, &mut counts);
    out
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
mod tests {
    use super::*;
    use ra_core::algebra::JoinType;
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan { table: name.into(), alias: None }
    }

    fn eq_join(left: RelExpr, right: RelExpr, l: &str, r: &str) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified(l, "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified(r, "id"))),
            },
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn single_scan_emits_only_seq_scan() {
        let advice = emit_advice(&scan("t"));
        assert_eq!(advice.len(), 1);
        assert_eq!(advice[0].tag, AdviceTag::SeqScan);
        let id = advice[0].targets[0].identifier.as_ref().unwrap();
        assert_eq!(id.alias_name, "t");
    }

    #[test]
    fn outer_deep_join_chain_emits_flat_join_order() {
        let q = eq_join(
            eq_join(scan("a"), scan("b"), "a", "b"),
            scan("c"),
            "a",
            "c",
        );
        let advice = emit_advice(&q);

        // Item 0: JOIN_ORDER(a b c)
        assert_eq!(advice[0].tag, AdviceTag::JoinOrder);
        let inner = &advice[0].targets[0];
        assert_eq!(inner.children.len(), 3);
        assert_eq!(
            inner.children[0].identifier.as_ref().unwrap().alias_name,
            "a"
        );
        assert_eq!(
            inner.children[1].identifier.as_ref().unwrap().alias_name,
            "b"
        );
        assert_eq!(
            inner.children[2].identifier.as_ref().unwrap().alias_name,
            "c"
        );

        // Item 1: SEQ_SCAN(a b c)
        assert_eq!(advice[1].tag, AdviceTag::SeqScan);
        assert_eq!(advice[1].targets.len(), 3);
    }

    #[test]
    fn bushy_join_emits_nested_sublist() {
        // (a JOIN b) JOIN (c JOIN d) — bushy.
        let bc = eq_join(scan("c"), scan("d"), "c", "d");
        let q = eq_join(eq_join(scan("a"), scan("b"), "a", "b"), bc, "a", "c");
        let advice = emit_advice(&q);

        assert_eq!(advice[0].tag, AdviceTag::JoinOrder);
        let inner = &advice[0].targets[0];
        // We expect a, b, (c d) — outer-deep flatten of (a JOIN b)
        // followed by the bushy inner sublist.
        assert_eq!(inner.children.len(), 3);
        assert_eq!(
            inner.children[2].kind,
            ra_plan_advice::ast::AdviceTargetKind::OrderedList
        );
        assert_eq!(inner.children[2].children.len(), 2);
    }

    #[test]
    fn rendered_advice_round_trips_through_parser() {
        let q = eq_join(
            eq_join(scan("a"), scan("b"), "a", "b"),
            scan("c"),
            "a",
            "c",
        );
        let advice = emit_advice(&q);
        let s = ra_plan_advice::render_advice(&advice);
        let reparsed = ra_plan_advice::parse_advice(&s).unwrap();
        assert_eq!(advice, reparsed);
    }

    #[test]
    fn duplicate_alias_gets_occurrence_numbers() {
        // SELECT * FROM users u1 JOIN users u2 ON u1.id = u2.id
        // — both scan "users" but neither has an explicit alias
        // in this minimal example, so they share alias_name and
        // get #1 / #2.
        let q = eq_join(scan("users"), scan("users"), "users", "users");
        let advice = emit_advice(&q);
        let scans_item = advice
            .iter()
            .find(|i| i.tag == AdviceTag::SeqScan)
            .unwrap();
        assert_eq!(scans_item.targets.len(), 2);
        let first = scans_item.targets[0].identifier.as_ref().unwrap();
        let second = scans_item.targets[1].identifier.as_ref().unwrap();
        assert_eq!(first.alias_name, "users");
        assert_eq!(first.occurrence, 1);
        assert_eq!(second.alias_name, "users");
        assert_eq!(second.occurrence, 2);
    }
}
