//! Error types for dialect translation.

use crate::dialect::Dialect;
use std::fmt;

/// Errors that can occur during dialect translation.
#[derive(Debug, thiserror::Error)]
pub enum TranslationError {
    /// SQL parsing failed.
    #[error("failed to parse SQL: {0}")]
    Parse(String),

    /// A feature required by the source SQL is not supported
    /// by the target dialect.
    #[error("unsupported feature in {dialect}: {feature}")]
    UnsupportedFeature {
        /// The target dialect.
        dialect: Dialect,
        /// Description of the unsupported feature.
        feature: String,
    },

    /// The SQL statement type is not supported for translation.
    #[error("unsupported statement type: {0}")]
    UnsupportedStatement(String),

    /// Transpilation failed in the backend.
    #[error("transpilation failed: {0}")]
    TranspilationFailed(String),
}

/// A warning generated during translation when the target
/// dialect handles something differently or may lose semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationWarning {
    /// Warning severity.
    pub severity: WarningSeverity,
    /// Human-readable warning message.
    pub message: String,
    /// Optional hint for the user.
    pub hint: Option<String>,
}

impl fmt::Display for TranslationWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.severity, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " (hint: {hint})")?;
        }
        Ok(())
    }
}

/// Severity levels for translation warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WarningSeverity {
    /// Informational: a transformation was applied that
    /// changes syntax but preserves semantics.
    Info,
    /// Warning: the translation may change behavior in
    /// edge cases.
    Warning,
    /// Error-level: the feature cannot be faithfully
    /// translated; the output is a best-effort
    /// approximation.
    Error,
}

impl fmt::Display for WarningSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_display_without_hint() {
        let w = TranslationWarning {
            severity: WarningSeverity::Info,
            message: "LIMIT translated to FETCH".into(),
            hint: None,
        };
        assert_eq!(w.to_string(), "[INFO] LIMIT translated to FETCH");
    }

    #[test]
    fn warning_display_with_hint() {
        let w = TranslationWarning {
            severity: WarningSeverity::Warning,
            message: "ILIKE not supported".into(),
            hint: Some("Using LOWER() + LIKE instead".into()),
        };
        assert!(w.to_string().contains("hint:"));
    }

    #[test]
    fn severity_display() {
        assert_eq!(WarningSeverity::Info.to_string(), "INFO");
        assert_eq!(WarningSeverity::Warning.to_string(), "WARN");
        assert_eq!(WarningSeverity::Error.to_string(), "ERROR");
    }

    #[test]
    fn parse_error_conversion() {
        let te = TranslationError::Parse("bad sql".into());
        assert!(te.to_string().contains("bad sql"));
    }
}
