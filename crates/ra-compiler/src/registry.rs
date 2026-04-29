//! Rule registry.
//!
//! Manages the collection of loaded rules, providing a unified
//! interface that combines the analyzer, type checker, and index.

use std::collections::HashSet;

use crate::analyzer::{Analyzer, OperatorKind, RuleInfo};
use crate::checker::TypeChecker;
use crate::index::RuleIndex;

/// Error returned when a rule cannot be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The rule has an empty name.
    EmptyName,
    /// A rule with this name is already registered.
    DuplicateName(String),
    /// The rule failed type checking with errors.
    ValidationFailed(Vec<String>),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "rule name must not be empty"),
            Self::DuplicateName(name) => {
                write!(f, "rule '{name}' is already registered")
            }
            Self::ValidationFailed(msgs) => {
                write!(f, "validation errors: {}", msgs.join("; "))
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// Central registry managing loaded rules.
///
/// Combines rule storage, indexing, validation, and dependency
/// analysis. Rules must pass type checking before they are accepted.
#[derive(Debug)]
pub struct RuleRegistry {
    analyzer: Analyzer,
    index: RuleIndex,
    strict: bool,
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self {
            analyzer: Analyzer::new(),
            index: RuleIndex::new(),
            strict: false,
        }
    }
}

impl RuleRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry in strict mode where warnings are also
    /// treated as errors.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            strict: true,
            ..Self::default()
        }
    }

    /// Register a rule, validating it first.
    ///
    /// # Errors
    ///
    /// Returns `RegistryError` if the rule has an empty name,
    /// is a duplicate, or fails type checking.
    pub fn register(&mut self, rule: RuleInfo) -> Result<(), RegistryError> {
        if rule.name.is_empty() {
            return Err(RegistryError::EmptyName);
        }

        if self.index.get(&rule.name).is_some() {
            return Err(RegistryError::DuplicateName(rule.name.clone()));
        }

        let mut checker = TypeChecker::new();
        checker.check_rule(&rule);

        let issues: Vec<String> = if self.strict {
            checker
                .diagnostics()
                .iter()
                .map(|d| d.message.clone())
                .collect()
        } else {
            checker
                .errors()
                .into_iter()
                .map(|d| d.message.clone())
                .collect()
        };

        if !issues.is_empty() {
            return Err(RegistryError::ValidationFailed(issues));
        }

        self.analyzer.add_rule(rule.clone());
        self.index.insert(rule);
        Ok(())
    }

    /// Remove a rule from the registry.
    pub fn unregister(&mut self, name: &str) -> Option<RuleInfo> {
        self.index.remove(name)
    }

    /// Get a rule by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RuleInfo> {
        self.index.get(name)
    }

    /// Number of registered rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Find rules by category.
    #[must_use]
    pub fn by_category(&self, category: &str) -> Vec<&str> {
        self.index.by_category(category)
    }

    /// Find rules that read a given operator.
    #[must_use]
    pub fn readers_of(&self, op: &OperatorKind) -> Vec<&str> {
        self.index.readers_of(op)
    }

    /// Find rules that write a given operator.
    #[must_use]
    pub fn writers_of(&self, op: &OperatorKind) -> Vec<&str> {
        self.index.writers_of(op)
    }

    /// Compute rule dependencies using the embedded analyzer.
    #[must_use]
    pub fn dependencies(&self) -> Vec<crate::analyzer::RuleDependency> {
        self.analyzer.compute_dependencies()
    }

    /// Find rules affected by changes to the named rule.
    #[must_use]
    pub fn affected_by(&self, rule_name: &str) -> Vec<String> {
        self.analyzer.affected_by(rule_name)
    }

    /// Detect potential conflicts between registered rules.
    #[must_use]
    pub fn conflicts(&self) -> Vec<(String, String, HashSet<OperatorKind>)> {
        self.analyzer.detect_conflicts()
    }

    /// Return all category names.
    #[must_use]
    pub fn categories(&self) -> Vec<&str> {
        self.index.categories()
    }

    /// Return all rule names.
    #[must_use]
    pub fn rule_names(&self) -> Vec<&str> {
        self.index.rule_names()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_rule(name: &str) -> RuleInfo {
        RuleInfo {
            name: name.into(),
            category: "test".into(),
            reads: HashSet::from([OperatorKind::Filter]),
            writes: HashSet::from([OperatorKind::Filter]),
        }
    }

    #[test]
    fn new_registry_is_empty() {
        let reg = RuleRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_valid_rule() {
        let mut reg = RuleRegistry::new();
        assert!(reg.register(valid_rule("r1")).is_ok());
        assert_eq!(reg.len(), 1);
        assert!(reg.get("r1").is_some());
    }

    #[test]
    fn register_empty_name_fails() {
        let mut reg = RuleRegistry::new();
        let result = reg.register(RuleInfo {
            name: String::new(),
            ..valid_rule("_")
        });
        assert_eq!(result, Err(RegistryError::EmptyName));
    }

    #[test]
    fn register_duplicate_name_fails() {
        let mut reg = RuleRegistry::new();
        assert!(reg.register(valid_rule("r1")).is_ok());
        let result = reg.register(valid_rule("r1"));
        assert_eq!(result, Err(RegistryError::DuplicateName("r1".into())));
    }

    #[test]
    fn register_validation_error_in_strict_mode() {
        let mut reg = RuleRegistry::strict();
        // A rule with empty reads gets a warning. In strict mode
        // warnings become errors.
        let rule = RuleInfo {
            name: "no-reads".into(),
            category: "test".into(),
            reads: HashSet::new(),
            writes: HashSet::from([OperatorKind::Filter]),
        };
        let result = reg.register(rule);
        assert!(matches!(result, Err(RegistryError::ValidationFailed(_))));
    }

    #[test]
    fn register_warning_allowed_in_normal_mode() {
        let mut reg = RuleRegistry::new();
        let rule = RuleInfo {
            name: "no-reads".into(),
            category: "test".into(),
            reads: HashSet::new(),
            writes: HashSet::from([OperatorKind::Filter]),
        };
        assert!(reg.register(rule).is_ok());
    }

    #[test]
    fn unregister_removes_rule() {
        let mut reg = RuleRegistry::new();
        assert!(reg.register(valid_rule("r1")).is_ok());
        let removed = reg.unregister("r1");
        assert!(removed.is_some());
        assert!(reg.is_empty());
        assert!(reg.get("r1").is_none());
    }

    #[test]
    fn unregister_nonexistent_returns_none() {
        let mut reg = RuleRegistry::new();
        assert!(reg.unregister("missing").is_none());
    }

    #[test]
    fn by_category_after_registration() {
        let mut reg = RuleRegistry::new();
        assert!(reg
            .register(RuleInfo {
                name: "a".into(),
                category: "cat1".into(),
                reads: HashSet::from([OperatorKind::Scan]),
                writes: HashSet::from([OperatorKind::Scan]),
            })
            .is_ok());
        assert!(reg
            .register(RuleInfo {
                name: "b".into(),
                category: "cat2".into(),
                reads: HashSet::from([OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Join]),
            })
            .is_ok());

        assert_eq!(reg.by_category("cat1").len(), 1);
        assert_eq!(reg.by_category("cat2").len(), 1);
        assert!(reg.by_category("cat3").is_empty());
    }

    #[test]
    fn readers_and_writers() {
        let mut reg = RuleRegistry::new();
        assert!(reg
            .register(RuleInfo {
                name: "rw".into(),
                category: "test".into(),
                reads: HashSet::from([OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Filter]),
            })
            .is_ok());

        assert!(reg.readers_of(&OperatorKind::Join).contains(&"rw"));
        assert!(reg.writers_of(&OperatorKind::Filter).contains(&"rw"));
        assert!(reg.readers_of(&OperatorKind::Filter).is_empty());
    }

    #[test]
    fn dependencies_with_two_rules() {
        let mut reg = RuleRegistry::new();
        assert!(reg
            .register(RuleInfo {
                name: "producer".into(),
                category: "test".into(),
                reads: HashSet::from([OperatorKind::Scan]),
                writes: HashSet::from([OperatorKind::Filter]),
            })
            .is_ok());
        assert!(reg
            .register(RuleInfo {
                name: "consumer".into(),
                category: "test".into(),
                reads: HashSet::from([OperatorKind::Filter]),
                writes: HashSet::from([OperatorKind::Project]),
            })
            .is_ok());

        let deps = reg.dependencies();
        assert!(deps
            .iter()
            .any(|d| d.producer == "producer" && d.consumer == "consumer"));
    }

    #[test]
    fn affected_by_integration() {
        let mut reg = RuleRegistry::new();
        assert!(reg
            .register(RuleInfo {
                name: "writer".into(),
                category: "test".into(),
                reads: HashSet::from([OperatorKind::Scan]),
                writes: HashSet::from([OperatorKind::Filter]),
            })
            .is_ok());
        assert!(reg
            .register(RuleInfo {
                name: "reader".into(),
                category: "test".into(),
                reads: HashSet::from([OperatorKind::Filter]),
                writes: HashSet::from([OperatorKind::Project]),
            })
            .is_ok());

        let affected = reg.affected_by("writer");
        assert!(affected.contains(&"reader".to_owned()));
    }

    #[test]
    fn conflicts_detected() {
        let mut reg = RuleRegistry::new();
        assert!(reg
            .register(RuleInfo {
                name: "a".into(),
                category: "test".into(),
                reads: HashSet::from([OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Join]),
            })
            .is_ok());
        assert!(reg
            .register(RuleInfo {
                name: "b".into(),
                category: "test".into(),
                reads: HashSet::from([OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Join]),
            })
            .is_ok());

        let conflicts = reg.conflicts();
        assert!(!conflicts.is_empty());
    }

    #[test]
    fn categories_and_rule_names() {
        let mut reg = RuleRegistry::new();
        assert!(reg.register(valid_rule("alpha")).is_ok());
        assert!(reg
            .register(RuleInfo {
                name: "beta".into(),
                category: "other".into(),
                reads: HashSet::from([OperatorKind::Join]),
                writes: HashSet::from([OperatorKind::Join]),
            })
            .is_ok());

        let cats = reg.categories();
        assert_eq!(cats.len(), 2);

        let names = reg.rule_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn error_display_messages() {
        assert_eq!(
            RegistryError::EmptyName.to_string(),
            "rule name must not be empty"
        );
        assert_eq!(
            RegistryError::DuplicateName("x".into()).to_string(),
            "rule 'x' is already registered"
        );
        assert_eq!(
            RegistryError::ValidationFailed(vec!["a".into(), "b".into()]).to_string(),
            "validation errors: a; b"
        );
    }

    #[test]
    fn default_trait() {
        let reg = RuleRegistry::default();
        assert!(reg.is_empty());
    }

    #[test]
    fn register_after_unregister_same_name() {
        let mut reg = RuleRegistry::new();
        assert!(reg.register(valid_rule("r")).is_ok());
        reg.unregister("r");
        // Re-registering should work since the duplicate check
        // uses the index which was cleaned.
        assert!(reg.register(valid_rule("r")).is_ok());
        assert_eq!(reg.len(), 1);
    }
}
