//! QUEL language parser (experimental).
//!
//! QUEL was the original query language of INGRES (1976), the
//! predecessor to `PostgreSQL`. This crate provides a parser for
//! academic and historical interest.
//!
//! # Status
//!
//! This is a stub crate. QUEL parsing is not yet implemented.
//! Contributions welcome.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]

use ra_core::algebra::RelExpr;
use thiserror::Error;

/// Errors from QUEL parsing.
#[derive(Debug, Error)]
pub enum QuelError {
    /// QUEL parsing is not yet implemented.
    #[error("QUEL parser not yet implemented")]
    NotImplemented,
}

/// Parse a QUEL query string into a relational expression.
///
/// # Errors
///
/// Currently always returns [`QuelError::NotImplemented`].
pub fn parse_quel(_input: &str) -> Result<RelExpr, QuelError> {
    Err(QuelError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_implemented_yet() {
        let result = parse_quel("retrieve (emp.name)");
        assert!(result.is_err());
    }
}
