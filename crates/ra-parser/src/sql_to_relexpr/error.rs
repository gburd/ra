use thiserror::Error;

use crate::ffi::node::StructuredParseError;

/// Errors that can occur during SQL parsing and conversion.
#[derive(Debug, Error)]
pub enum SqlConversionError {
    /// SQL parsing failed (unstructured string errors).
    #[error("failed to parse SQL: {0}")]
    ParseError(String),

    /// SQL parsing failed with structured diagnostics.
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    StructuredParseErrors(Vec<StructuredParseError>),

    /// Unsupported SQL construct.
    #[error("unsupported SQL feature: {0}")]
    UnsupportedFeature(String),

    /// Invalid SQL semantics.
    #[error("invalid SQL: {0}")]
    InvalidSql(String),

    /// Invalid recursive CTE structure.
    #[error("invalid recursive CTE: {0}")]
    InvalidRecursiveCTE(String),
}
