/// Errors that can occur during e-graph optimization.
#[derive(Debug, thiserror::Error)]
pub enum EGraphError {
    /// Failed to convert a relational expression to the e-graph.
    #[error("failed to convert expression to e-graph: {0}")]
    ConversionError(String),

    /// Failed to extract a plan from the e-graph.
    #[error("failed to extract plan from e-graph: {0}")]
    ExtractionError(String),

    /// Resource budget was exceeded with Fail strategy.
    #[error("resource budget exceeded: {0}")]
    ResourceBudgetExceeded(String),
}
