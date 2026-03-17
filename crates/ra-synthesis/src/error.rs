//! Error types for the synthesis pipeline.

use thiserror::Error;

/// Errors produced during query synthesis.
#[derive(Debug, Error)]
pub enum SynthesisError {
    /// The natural language input could not be understood.
    #[error("could not parse intent from input: {0}")]
    IntentParseFailed(String),

    /// A table referenced in the query does not exist in the schema.
    #[error("unknown table: {0}")]
    UnknownTable(String),

    /// A column referenced in the query does not exist in the schema.
    #[error("unknown column `{column}` in table `{table}`")]
    UnknownColumn {
        /// The table being referenced.
        table: String,
        /// The column that was not found.
        column: String,
    },

    /// No tables were identified from the natural language input.
    #[error("no tables identified in input")]
    NoTablesIdentified,

    /// An ambiguous column reference could not be resolved.
    #[error("ambiguous column `{column}` -- found in tables: {tables}")]
    AmbiguousColumn {
        /// The ambiguous column name.
        column: String,
        /// Comma-separated list of tables containing this column.
        tables: String,
    },

    /// The generated query failed validation.
    #[error("validation failed: {0}")]
    ValidationFailed(String),
}
