//! Rule traits and types for query optimization.
//!
//! Rules define transformations on relational expressions. Each rule
//! specifies a pattern it matches and the transformation it applies.
//! Rules are grouped into categories (logical, physical, implementation)
//! and can be scoped to specific database backends.

use serde::{Deserialize, Serialize};

use crate::algebra::RelExpr;
use crate::pattern::Pattern;
use crate::precondition::PreCondition;

/// A transformation rule that rewrites relational expressions.
///
/// Implementations provide a pattern to match against and a
/// transformation function that produces a new expression.
pub trait Rule: std::fmt::Debug + Send + Sync {
    /// Return metadata describing this rule.
    fn metadata(&self) -> &RuleMetadata;

    /// Return the pattern this rule matches against.
    fn pattern(&self) -> &Pattern;

    /// Check whether this rule applies to the given expression.
    ///
    /// This is a quick pre-check before attempting the full match.
    /// Returning `true` does not guarantee a match; returning `false`
    /// skips the rule entirely.
    fn matches(&self, expr: &RelExpr) -> bool;

    /// Apply the rule to produce a new expression.
    ///
    /// Returns `None` if the rule cannot actually transform the
    /// input, even though `matches` returned `true`.
    fn apply(&self, expr: &RelExpr) -> Option<RelExpr>;
}

/// Metadata describing a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleMetadata {
    /// Unique identifier for the rule.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// A brief description of what the rule does.
    pub description: String,
    /// The category this rule belongs to.
    pub category: RuleCategory,
    /// Database backends this rule is applicable to.
    /// An empty list means the rule is universal.
    pub databases: Vec<String>,
    /// Priority for ordering rules (lower runs first).
    pub priority: i32,
    /// Formal pre-conditions that must be satisfied for this rule to apply.
    /// These can reference system facts (statistics, hardware, schema).
    #[serde(default)]
    pub preconditions: Vec<PreCondition>,
}

/// Categories of optimization rules.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum RuleCategory {
    /// Logical-to-logical transformations (e.g., predicate pushdown).
    Logical,
    /// Logical-to-physical transformations (e.g., choose hash join).
    Physical,
    /// Implementation-specific rewrites for a target database.
    Implementation,
}

/// A record of a rule having been applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleApplication {
    /// The ID of the rule that was applied.
    pub rule_id: String,
    /// The expression before the rule was applied.
    pub before: RelExpr,
    /// The expression after the rule was applied.
    pub after: RelExpr,
}

impl std::fmt::Display for RuleCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Logical => write!(f, "Logical"),
            Self::Physical => write!(f, "Physical"),
            Self::Implementation => write!(f, "Implementation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{Const, Expr};

    #[test]
    fn rule_metadata_fields() {
        let meta = RuleMetadata {
            id: "push-filter".into(),
            name: "Push Filter Down".into(),
            description: "Pushes filters below joins.".into(),
            category: RuleCategory::Logical,
            databases: vec![],
            priority: 10,
            preconditions: vec![],
        };

        assert_eq!(meta.id, "push-filter");
        assert_eq!(meta.category, RuleCategory::Logical);
        assert!(meta.databases.is_empty());
        assert!(meta.preconditions.is_empty());
    }

    #[test]
    fn rule_category_display() {
        assert_eq!(RuleCategory::Logical.to_string(), "Logical");
        assert_eq!(RuleCategory::Physical.to_string(), "Physical");
        assert_eq!(
            RuleCategory::Implementation.to_string(),
            "Implementation"
        );
    }

    #[test]
    fn rule_application_roundtrip() {
        let before = RelExpr::scan("t");
        let after =
            RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));

        let app = RuleApplication {
            rule_id: "test-rule".into(),
            before: before.clone(),
            after: after.clone(),
        };

        let json = serde_json::to_string(&app)
            .expect("serialization should succeed");
        let deserialized: RuleApplication = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(deserialized.rule_id, "test-rule");
        assert_eq!(deserialized.before, before);
        assert_eq!(deserialized.after, after);
    }
}
