//! Type checking for rules.
//!
//! Validates that rule patterns and transformations are type-correct.
//! Ensures operator kinds referenced in reads/writes are consistent,
//! and detects rules with missing or invalid metadata.

use std::collections::HashSet;

use crate::analyzer::{OperatorKind, RuleInfo};

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Something that will cause incorrect behavior.
    Error,
    /// Something that may indicate a mistake.
    Warning,
}

/// A single validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Which rule this finding applies to.
    pub rule_name: String,
    /// Severity of the finding.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
}

/// Validates rules for structural and semantic correctness.
#[derive(Debug, Default)]
pub struct TypeChecker {
    diagnostics: Vec<Diagnostic>,
}

impl TypeChecker {
    /// Create a new, empty type checker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate a single rule, accumulating diagnostics.
    pub fn check_rule(&mut self, rule: &RuleInfo) {
        if rule.name.is_empty() {
            self.diagnostics.push(Diagnostic {
                rule_name: String::new(),
                severity: Severity::Error,
                message: "rule has empty name".into(),
            });
        }

        if rule.category.is_empty() {
            self.diagnostics.push(Diagnostic {
                rule_name: rule.name.clone(),
                severity: Severity::Error,
                message: "rule has empty category".into(),
            });
        }

        if rule.reads.is_empty() {
            self.diagnostics.push(Diagnostic {
                rule_name: rule.name.clone(),
                severity: Severity::Warning,
                message: "rule reads no operators (will never \
                          match anything)"
                    .into(),
            });
        }

        if rule.writes.is_empty() {
            self.diagnostics.push(Diagnostic {
                rule_name: rule.name.clone(),
                severity: Severity::Warning,
                message: "rule writes no operators (transformation \
                          has no effect)"
                    .into(),
            });
        }

        // Warn if a rule writes to operators it doesn't read.
        let write_only: HashSet<&OperatorKind> = rule.writes.difference(&rule.reads).collect();
        if !write_only.is_empty() {
            self.diagnostics.push(Diagnostic {
                rule_name: rule.name.clone(),
                severity: Severity::Warning,
                message: format!(
                    "rule writes to operators it does not read: \
                     {write_only:?}"
                ),
            });
        }
    }

    /// Validate a batch of rules.
    pub fn check_rules(&mut self, rules: &[RuleInfo]) {
        for rule in rules {
            self.check_rule(rule);
        }
    }

    /// Return all accumulated diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Return only error-level diagnostics.
    #[must_use]
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    /// Return only warning-level diagnostics.
    #[must_use]
    pub fn warnings(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect()
    }

    /// Whether any errors were found.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Reset all diagnostics.
    pub fn clear(&mut self) {
        self.diagnostics.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_rule() -> RuleInfo {
        RuleInfo {
            name: "test-rule".into(),
            category: "test-category".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter]),
        }
    }

    #[test]
    fn valid_rule_produces_no_diagnostics() {
        let mut checker = TypeChecker::new();
        checker.check_rule(&valid_rule());
        assert!(checker.diagnostics().is_empty());
        assert!(!checker.has_errors());
    }

    #[test]
    fn empty_name_is_error() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            name: String::new(),
            ..valid_rule()
        };
        checker.check_rule(&rule);
        assert!(checker.has_errors());
        assert_eq!(checker.errors().len(), 1);
        assert!(checker.errors()[0].message.contains("empty name"));
    }

    #[test]
    fn empty_category_is_error() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            category: String::new(),
            ..valid_rule()
        };
        checker.check_rule(&rule);
        assert!(checker.has_errors());
        assert!(checker.errors()[0].message.contains("empty category"));
    }

    #[test]
    fn empty_reads_is_warning() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            reads: HashSet::new(),
            ..valid_rule()
        };
        checker.check_rule(&rule);
        assert!(!checker.has_errors());
        let warnings = checker.warnings();
        assert!(warnings
            .iter()
            .any(|w| w.message.contains("reads no operators")));
    }

    #[test]
    fn empty_writes_is_warning() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            writes: HashSet::new(),
            ..valid_rule()
        };
        checker.check_rule(&rule);
        assert!(!checker.has_errors());
        let warnings = checker.warnings();
        assert!(warnings
            .iter()
            .any(|w| w.message.contains("writes no operators")));
    }

    #[test]
    fn write_without_read_is_warning() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter, OperatorKind::Join]),
            ..valid_rule()
        };
        checker.check_rule(&rule);
        assert!(!checker.has_errors());
        let warnings = checker.warnings();
        assert!(warnings
            .iter()
            .any(|w| w.message.contains("writes to operators it does not read")));
    }

    #[test]
    fn check_rules_batch() {
        let mut checker = TypeChecker::new();
        let rules = vec![valid_rule(), valid_rule()];
        checker.check_rules(&rules);
        assert!(checker.diagnostics().is_empty());
    }

    #[test]
    fn check_rules_batch_with_errors() {
        let mut checker = TypeChecker::new();
        let rules = vec![
            valid_rule(),
            RuleInfo {
                name: String::new(),
                ..valid_rule()
            },
        ];
        checker.check_rules(&rules);
        assert!(checker.has_errors());
        assert_eq!(checker.errors().len(), 1);
    }

    #[test]
    fn clear_resets_diagnostics() {
        let mut checker = TypeChecker::new();
        checker.check_rule(&RuleInfo {
            name: String::new(),
            ..valid_rule()
        });
        assert!(checker.has_errors());
        checker.clear();
        assert!(checker.diagnostics().is_empty());
        assert!(!checker.has_errors());
    }

    #[test]
    fn multiple_issues_on_same_rule() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            name: String::new(),
            category: String::new(),
            reads: HashSet::new(),
            writes: HashSet::new(),
        };
        checker.check_rule(&rule);
        // Empty name (error) + empty category (error) +
        // empty reads (warning) + empty writes (warning)
        assert_eq!(checker.errors().len(), 2);
        assert_eq!(checker.warnings().len(), 2);
        assert_eq!(checker.diagnostics().len(), 4);
    }

    #[test]
    fn read_write_superset_no_warning() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            reads: HashSet::from([OperatorKind::Filter, OperatorKind::Join]),
            writes: HashSet::from([OperatorKind::Filter]),
            ..valid_rule()
        };
        checker.check_rule(&rule);
        assert!(checker.diagnostics().is_empty());
    }

    #[test]
    fn diagnostic_rule_name_preserved() {
        let mut checker = TypeChecker::new();
        let rule = RuleInfo {
            category: String::new(),
            ..valid_rule()
        };
        checker.check_rule(&rule);
        assert_eq!(checker.errors()[0].rule_name, "test-rule");
    }
}
