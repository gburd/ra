use thiserror::Error;

/// Errors that can occur during SQL parsing and conversion.
#[derive(Debug, Error)]
pub enum SqlConversionError {
    /// SQL parsing failed.
    #[error("failed to parse SQL: {0}")]
    ParseError(String),

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
