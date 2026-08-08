//! Machine-checkable proof obligations for rewrite rules (RA-STEERING §5.4).
//!
//! A [`ProofObligation`] declares *why* a rewrite is sound — distinct from a
//! [`crate::PreCondition`], which gates *when* a rule fires. §5.4 defines five
//! obligation classes plus an explicit opt-out:
//!
//! - **`null_rejection`** — for any predicate pushed through an OUTER join
//! - **`volatility`** — volatility-class constraints for a rewrite that
//!   duplicates, removes, or reorders expression evaluation
//! - **`error_behavior`** — error-behavior preservation for a rewrite that
//!   changes the row set an expression is evaluated over
//! - **`security_rls`** — security-barrier / RLS interaction for any pushdown
//! - **`uniqueness_fd`** — uniqueness / functional-dependency prerequisites for
//!   join elimination and distinct removal
//! - **`none`** — the rule declares, with a justification, that no obligation
//!   applies. This makes "declared none needed" distinguishable from the silent
//!   "missing" (undeclared) case the linter flags.
//!
//! "A rule that does not declare its obligations does not load." That hard
//! flip is the follow-on; today the [`crate::proof_obligation`] mapping plus the
//! CLI linter enforce it as a *ratchet* (`ra rules lint --check-obligations`).
//!
//! # Example (`.rra` frontmatter)
//!
//! ```yaml
//! proof_obligations:
//!   - type: null_rejection
//!     predicate: "?pred IS NOT NULL on preserved side"
//!     description: "Outer join collapses to inner only under a null-rejecting filter"
//!   - type: uniqueness_fd
//!     keys: "primary key of ?rel"
//! ```

use serde::{Deserialize, Serialize};

/// A machine-checkable declaration of *why* a rewrite is sound.
///
/// Serialized in `.rra` frontmatter under `proof_obligations:`, tagged by
/// `type` (`snake_case`), mirroring [`crate::PreCondition`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProofObligation {
    /// The predicate pushed through an OUTER join rejects NULLs on the
    /// preserved side (so the rewrite preserves the outer-join semantics).
    NullRejection {
        /// The null-rejecting predicate (metavariable or prose).
        #[serde(skip_serializing_if = "Option::is_none")]
        predicate: Option<String>,
        /// Human-readable justification.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    /// The maximum volatility class the rewrite tolerates, for a rewrite that
    /// duplicates, removes, or reorders expression evaluation.
    Volatility {
        /// Maximum volatility class: `immutable` | `stable` | `volatile`.
        max_class: String,
        /// Human-readable justification.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    /// Error-behavior preservation for a rewrite that changes the row set an
    /// expression is evaluated over (e.g. pushing a `/0`-prone expr under a
    /// filter that would have excluded the offending rows).
    ErrorBehavior {
        /// Human-readable justification.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    /// Security-barrier / RLS interaction for a pushdown (a predicate must not
    /// leapfrog a security barrier or RLS-qualified relation).
    SecurityRls {
        /// Human-readable justification.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    /// Uniqueness / functional-dependency prerequisite for join elimination or
    /// distinct removal (the key/FD that guarantees at-most-one match).
    UniquenessFd {
        /// The key or functional dependency relied upon.
        #[serde(skip_serializing_if = "Option::is_none")]
        keys: Option<String>,
        /// Human-readable justification.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    /// The rule declares, explicitly, that no obligation applies. Carries a
    /// justification so a genuinely-safe rewrite opts out on the record rather
    /// than silently. Note: `None` does *not* satisfy a *required* obligation
    /// kind — see [`ObligationKind::satisfied_by`].
    None {
        /// Why no obligation is needed.
        justification: String,
    },
}

impl ProofObligation {
    /// The [`ObligationKind`] this obligation declares.
    #[must_use]
    pub fn kind(&self) -> ObligationKind {
        match self {
            Self::NullRejection { .. } => ObligationKind::NullRejection,
            Self::Volatility { .. } => ObligationKind::Volatility,
            Self::ErrorBehavior { .. } => ObligationKind::ErrorBehavior,
            Self::SecurityRls { .. } => ObligationKind::SecurityRls,
            Self::UniquenessFd { .. } => ObligationKind::UniquenessFd,
            Self::None { .. } => ObligationKind::None,
        }
    }
}

/// The kind of a proof obligation, independent of its payload. Used by the
/// category→required mapping and the linter to check coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObligationKind {
    /// Null-rejection through an outer join.
    NullRejection,
    /// Volatility-class constraint.
    Volatility,
    /// Error-behavior preservation.
    ErrorBehavior,
    /// Security-barrier / RLS interaction.
    SecurityRls,
    /// Uniqueness / functional-dependency prerequisite.
    UniquenessFd,
    /// Explicit "no obligation needed".
    None,
}

impl ObligationKind {
    /// String label (matches the serde `type` tag).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NullRejection => "null_rejection",
            Self::Volatility => "volatility",
            Self::ErrorBehavior => "error_behavior",
            Self::SecurityRls => "security_rls",
            Self::UniquenessFd => "uniqueness_fd",
            Self::None => "none",
        }
    }

    /// Whether a declared obligation satisfies *this required* kind.
    ///
    /// A `None{..}` opt-out never satisfies a required obligation — a rule the
    /// mapping says *needs* null-rejection cannot discharge it by asserting
    /// "none needed".
    #[must_use]
    pub fn satisfied_by(self, declared: &ProofObligation) -> bool {
        declared.kind() == self
    }
}

/// Category → required-obligation-kind mapping — the enforcement rule for §5.4.
///
/// This is a deliberately **conservative** first cut: it requires an obligation
/// only where the category (and, for the outer-join case, the rule id) makes the
/// hazard clear. Under-requiring is correct for a ratchet — a check that fires on
/// everything is noise. Tighten it as the backlog is annotated.
///
/// Mapping:
/// - `join-elimination` / `distinct-elimination` / `distinct-removal` and
///   `count`/`aggregate` *-elimination* → **`uniqueness_fd`** (the rewrite drops a
///   relation or dedup step, sound only under a key/FD guarantee).
/// - `predicate-pushdown` / `filter-through*` **AND** the rule id/name names an
///   outer join (`outer`/`left`/`right`/`full`) → **`null_rejection`** (a predicate
///   crossing an outer join must reject NULLs to preserve semantics).
/// - `subquery-decorrelation` / `subquery-unnesting` / `common-subexpression` /
///   `cse` → **`volatility`** (decorrelation/CSE duplicate or reorder expression
///   evaluation; a volatile expr changes results).
///
/// Everything else has no *required* obligation in v1.
#[must_use]
pub fn required_obligations(category: &str, id: &str) -> Vec<ObligationKind> {
    let cat = category.to_ascii_lowercase();
    let rid = id.to_ascii_lowercase();
    let mut required = Vec::new();

    // "outer" specifically — NOT bare `-left`/`-right`, which in the core
    // pushdown/elimination rules mean "the left/right *child*" of an INNER
    // join, not an outer join. Keying off those produced pure false positives.
    let names_outer = |s: &str| {
        s.contains("outer")
            || s.contains("left-join")
            || s.contains("right-join")
            || s.contains("full-join")
            || s.contains("left-outer")
            || s.contains("right-outer")
            || s.contains("full-outer")
    };
    let outer = names_outer(&rid) || names_outer(&cat);

    // Uniqueness / FD: join elimination and distinct/dedup removal drop a
    // relation or a dedup step — sound only under a key/FD guarantee.
    let is_elimination = cat.contains("join-elimination")
        || cat.contains("distinct-elimination")
        || cat.contains("distinct-removal")
        || (cat.contains("distinct") && cat.contains("elimination"));

    // Null-rejection: an OUTER join turning into an inner join (or a predicate
    // crossing an outer join) is sound only when a null-rejecting predicate
    // negates the outer join's NULL-extension. This takes precedence over the
    // uniqueness/FD requirement for outer-join *elimination* rules, since the
    // soundness argument there is null-rejection, not a key.
    let is_pushdown = cat.contains("predicate-pushdown") || cat.contains("filter-through");
    if outer && (is_pushdown || cat.contains("join-elimination") || cat.contains("join-reordering"))
    {
        required.push(ObligationKind::NullRejection);
    } else if is_elimination {
        required.push(ObligationKind::UniquenessFd);
    }

    // Volatility: rewrites that duplicate/reorder expression evaluation.
    if cat.contains("subquery-decorrelation")
        || cat.contains("subquery-unnesting")
        || cat.contains("common-subexpression")
        || cat.contains("cse")
    {
        required.push(ObligationKind::Volatility);
    }

    required
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code uses unwrap for assertions")]
mod tests {
    use super::*;

    #[test]
    fn obligation_serde_round_trip() {
        let obs = vec![
            ProofObligation::NullRejection {
                predicate: Some("?pred IS NOT NULL".into()),
                description: Some("outer -> inner needs null rejection".into()),
            },
            ProofObligation::Volatility {
                max_class: "stable".into(),
                description: None,
            },
            ProofObligation::ErrorBehavior { description: None },
            ProofObligation::SecurityRls {
                description: Some("must not cross RLS barrier".into()),
            },
            ProofObligation::UniquenessFd {
                keys: Some("primary key".into()),
                description: None,
            },
            ProofObligation::None {
                justification: "structural rewrite, evaluates no expressions".into(),
            },
        ];
        let yaml = serde_yml::to_string(&obs).unwrap();
        let back: Vec<ProofObligation> = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(obs, back);
    }

    #[test]
    fn kind_matches_variant() {
        assert_eq!(
            ProofObligation::None {
                justification: "x".into()
            }
            .kind(),
            ObligationKind::None
        );
        assert_eq!(
            ProofObligation::UniquenessFd {
                keys: None,
                description: None
            }
            .kind(),
            ObligationKind::UniquenessFd
        );
    }

    #[test]
    fn none_does_not_satisfy_required() {
        let none = ProofObligation::None {
            justification: "x".into(),
        };
        assert!(!ObligationKind::NullRejection.satisfied_by(&none));
        assert!(ObligationKind::None.satisfied_by(&none));
    }

    #[test]
    fn mapping_join_elimination_requires_uniqueness() {
        let req = required_obligations("logical/join-elimination", "foreign-key-join-elimination");
        assert!(req.contains(&ObligationKind::UniquenessFd));
        assert!(!req.contains(&ObligationKind::NullRejection));
    }

    #[test]
    fn mapping_outer_join_elimination_requires_null_rejection() {
        // outer -> inner is a null-rejection argument, not a key argument.
        let req = required_obligations("logical/join-elimination", "outer-join-to-inner");
        assert_eq!(req, vec![ObligationKind::NullRejection]);
    }

    #[test]
    fn mapping_distinct_elimination_requires_uniqueness() {
        let req = required_obligations("logical/distinct-elimination", "distinct-over-primary-key");
        assert_eq!(req, vec![ObligationKind::UniquenessFd]);
    }

    #[test]
    fn mapping_outer_pushdown_requires_null_rejection() {
        // Outer join named in the id + a pushdown category.
        let req = required_obligations("logical/predicate-pushdown", "filter-through-outer-join");
        assert!(req.contains(&ObligationKind::NullRejection));
        // Inner pushdown: no requirement (conservative).
        let inner = required_obligations("logical/predicate-pushdown", "filter-through-join");
        assert!(inner.is_empty());
        // `-left`/`-right` name the child of an INNER join, not an outer join:
        // must NOT be flagged (regression against the earlier false positive).
        assert!(required_obligations(
            "logical/predicate-pushdown-core",
            "filter-through-join-left"
        )
        .is_empty());
        assert!(
            required_obligations("logical/predicate-pushdown-core", "filter-and-push-right")
                .is_empty()
        );
    }

    #[test]
    fn mapping_decorrelation_requires_volatility() {
        let req = required_obligations(
            "logical/subquery-decorrelation",
            "decorrelate-scalar-subquery",
        );
        assert_eq!(req, vec![ObligationKind::Volatility]);
    }

    #[test]
    fn mapping_plain_category_requires_nothing() {
        assert!(
            required_obligations("logical/projection-pushdown", "project-through-scan").is_empty()
        );
        assert!(required_obligations("physical/index-selection", "use-index").is_empty());
    }
}
