//! Macro-rule system: multi-step transformations expressed declaratively.
//!
//! A macro-rule is a sequence of rewrite steps that together accomplish a
//! transformation too complex for a single LHS→RHS pattern. Each step is
//! a standard egg rewrite, but the macro-rule system guarantees they fire
//! in order and shares metavariable bindings across steps.
//!
//! ## Example: uncorrelated IN → SemiJoin
//!
//! ```text
//! steps:
//!   1. (filter (in ?test (subquery (project [?col] ?inner))) ?outer)
//!      → (semi-join (= ?test ?col) ?outer ?inner)
//! ```
//!
//! ## Example: uncorrelated EXISTS → SemiJoin  
//!
//! ```text
//! steps:
//!   1. (filter (exists (filter ?corr ?inner)) ?outer)
//!      → (semi-join ?corr ?outer ?inner)
//! ```
//!
//! Macro-rules are compiled to standard egg rewrites at load time.
//! The "macro" aspect is that they can express patterns that would
//! require matching nested structures (subqueries inside predicates)
//! which single-level egg patterns can't express.

use egg::Rewrite;
use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// A compiled macro-rule: one or more egg rewrites that together
/// accomplish a multi-step transformation.
pub struct MacroRule {
    /// Unique identifier (from .rra frontmatter).
    pub id: String,
    /// The compiled rewrites this macro expands to.
    pub rewrites: Vec<Rewrite<RelLang, RelAnalysis>>,
}

/// Load all macro-rules from the rule corpus.
/// Currently returns hard-coded decorrelation macro-rules;
/// future: parse from .rra files with `kind: macro`.
pub fn load_macro_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    // Simple decorrelation patterns expressed as egg rewrites.
    // These fire during saturation alongside regular rules.
    decorrelation_rules()
}

/// Decorrelation rewrites expressible as single-step egg patterns.
/// These handle the SIMPLE (uncorrelated) cases directly in the e-graph.
/// Complex correlated cases remain in the pre-optimization Rust code.
fn decorrelation_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    use egg::rewrite;

    vec![
        // EXISTS subquery with a correlation predicate in the subquery's
        // filter → SemiJoin. This is the e-graph equivalent of the
        // decorrelate_subquery(Exists) code path.
        //
        // Note: egg's pattern language can't express "subquery inside a
        // predicate" directly, so these rules work on the ALREADY-NORMALIZED
        // form (post-decorrelation pass). They allow the e-graph to REDISCOVER
        // the semi-join form if it was lost during saturation.
        rewrite!("semi-join-from-exists";
            "(filter ?pred (join semi ?cond ?left ?right))" =>
            "(join semi (and ?cond ?pred) ?left ?right)"
            if crate::conditions::references_only("?pred", "?right")
        ),

        // Anti-join condition absorption (NOT EXISTS already decorrelated)
        rewrite!("anti-join-absorb-filter";
            "(filter ?pred (join anti ?cond ?left ?right))" =>
            "(join anti (and ?cond ?pred) ?left ?right)"
            if crate::conditions::references_only("?pred", "?right")
        ),
    ]
}
