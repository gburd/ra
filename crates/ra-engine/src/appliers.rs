//! Dynamic applier vocabulary for generated rewrite rules (RFC 0090 Phase 2).
//!
//! These appliers implement `egg::Applier<RelLang, RelAnalysis>` and are
//! referenced by `.rra` rule files via the `apply:` field, e.g.
//!
//! ```text
//! ## Rewrite
//! lhs: (add (const-int ?a) (const-int ?b))
//! apply: fold_add("?a", "?b")
//! ```
//!
//! Unlike a static `rhs:` pattern, an applier *computes* the replacement from
//! the matched substitution — required for rules whose right-hand side is not a
//! fixed shape (constant folding now; cost-based physical lowering in Phase 3).
//! Each new applier here, registered in `build.rs::KNOWN_APPLIERS`, unlocks the
//! `.rra` rules that reference it — keeping that logic in rules, not in engine
//! code.

use egg::{Applier, EGraph, Id, PatternAst, Subst, Symbol, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::parse_var;

/// The arithmetic operation a [`FoldArith`] applier evaluates.
#[derive(Clone, Copy, Debug)]
enum ArithOp {
    Add,
    Sub,
    Mul,
}

/// Applier that folds `op(const-int ?a, const-int ?b)` into a single
/// `const-int` holding the computed value. Declines (returns no rewrite) when
/// either operand is not an integer constant or the operation overflows, so it
/// is always sound.
pub struct FoldArith {
    op: ArithOp,
    a: Var,
    b: Var,
}

/// Read an `i64` from an e-class that contains a `Symbol` literal node (the
/// child of a `const-int`). Returns `None` if no integer symbol is present.
fn read_int(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Option<i64> {
    egraph[id].nodes.iter().find_map(|n| match n {
        RelLang::Symbol(s) => s.as_str().parse::<i64>().ok(),
        _ => None,
    })
}

impl Applier<RelLang, RelAnalysis> for FoldArith {
    fn apply_one(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        eclass: Id,
        subst: &Subst,
        _searcher_ast: Option<&PatternAst<RelLang>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let (Some(a), Some(b)) = (read_int(egraph, subst[self.a]), read_int(egraph, subst[self.b]))
        else {
            return Vec::new();
        };
        let folded = match self.op {
            ArithOp::Add => a.checked_add(b),
            ArithOp::Sub => a.checked_sub(b),
            ArithOp::Mul => a.checked_mul(b),
        };
        let Some(value) = folded else {
            return Vec::new(); // overflow: decline rather than wrap
        };
        let sym = egraph.add(RelLang::Symbol(Symbol::from(value.to_string())));
        let folded_id = egraph.add(RelLang::ConstInt([sym]));
        // Union the computed constant with the matched e-class so the folded
        // form is recognized as equivalent. Report the change for the runner.
        if egraph.union(eclass, folded_id) {
            vec![folded_id]
        } else {
            Vec::new()
        }
    }
}

/// Applier folding `(add (const-int ?a) (const-int ?b))` → `(const-int a+b)`.
#[must_use]
pub fn fold_add(a: &str, b: &str) -> FoldArith {
    FoldArith { op: ArithOp::Add, a: parse_var(a), b: parse_var(b) }
}

/// Applier folding `(sub (const-int ?a) (const-int ?b))` → `(const-int a-b)`.
#[must_use]
pub fn fold_sub(a: &str, b: &str) -> FoldArith {
    FoldArith { op: ArithOp::Sub, a: parse_var(a), b: parse_var(b) }
}

/// Applier folding `(mul (const-int ?a) (const-int ?b))` → `(const-int a*b)`.
#[must_use]
pub fn fold_mul(a: &str, b: &str) -> FoldArith {
    FoldArith { op: ArithOp::Mul, a: parse_var(a), b: parse_var(b) }
}
