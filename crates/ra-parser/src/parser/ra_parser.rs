//! Main RaParser facade for profile-based SQL parsing.
//!
//! The `RaParser` provides a unified interface for parsing SQL queries with
//! automatic dialect detection and profile-based grammar extensions.

use crate::profile::ParserProfile;
use ra_core::RelExpr;
use sqlparser::ast::Statement;
use std::error::Error;
use std::fmt;

/// Main parser facade with profile support.
///
/// # Examples
///
/// ```ignore
/// use ra_parser::RaParser;
///
/// // Parse with automatic dialect detection
/// let parser = RaParser::universal();
/// let expr = parser.parse("SELECT * FROM users WHERE id = 1")?;
///
/// // Parse with specific profile
/// let parser = RaParser::with_profile("postgresql-17")?;
/// let expr = parser.parse("SELECT ARRAY[1,2,3]::int[]")?;
/// ```
pub struct RaParser {
    profile: ParserProfile,
}

/// Parser errors.
#[derive(Debug)]
pub enum ParserError {
    /// Profile not found or invalid
    InvalidProfile(String),
    /// SQL parsing failed
    ParseError(String),
    /// Unsupported SQL feature for the current profile
    UnsupportedFeature(String),
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParserError::InvalidProfile(msg) => write!(f, "Invalid profile: {}", msg),
            ParserError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            ParserError::UnsupportedFeature(msg) => write!(f, "Unsupported feature: {}", msg),
        }
    }
}

impl Error for ParserError {}

/// Result type for parser operations.
pub type Result<T> = std::result::Result<T, ParserError>;

impl RaParser {
    /// Create a parser with the universal profile (parses any SQL dialect).
    ///
    /// This is the most permissive profile and will attempt to parse SQL from
    /// any vendor or standard.
    pub fn universal() -> Self {
        Self {
            profile: ParserProfile::universal(),
        }
    }

    /// Create a parser with a specific named profile.
    ///
    /// # Arguments
    ///
    /// * `name` - Profile name (e.g., "postgresql-17", "mysql-8.4", "universal")
    ///
    /// # Errors
    ///
    /// Returns `InvalidProfile` if the profile name is not found.
    pub fn with_profile(name: &str) -> Result<Self> {
        let profile = ParserProfile::load(name)
            .map_err(|e| ParserError::InvalidProfile(e.to_string()))?;
        Ok(Self { profile })
    }

    /// Create a parser by inferring the dialect from the SQL text.
    ///
    /// Uses probabilistic dialect detection based on syntax features,
    /// keywords, and function names.
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL query text to analyze
    ///
    /// # Returns
    ///
    /// Returns a parser configured for the detected dialect with a confidence score.
    pub fn auto_detect(sql: &str) -> Result<(Self, f64)> {
        let (profile, confidence) = ParserProfile::infer(sql)
            .map_err(|e| ParserError::InvalidProfile(e.to_string()))?;
        Ok((Self { profile }, confidence))
    }

    /// Parse SQL text into a relational algebra expression.
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL query text
    ///
    /// # Returns
    ///
    /// Returns a `RelExpr` representing the query's relational algebra.
    ///
    /// # Errors
    ///
    /// Returns `ParseError` if the SQL cannot be parsed or contains syntax errors.
    pub fn parse(&self, sql: &str) -> Result<RelExpr> {
        // TODO: Implement actual parsing with profile support
        // For now, delegate to existing sql_to_relexpr
        crate::sql_to_relexpr(sql)
            .map_err(|e| ParserError::ParseError(e.to_string()))
    }

    /// Parse SQL text into sqlparser AST with profile-specific grammar.
    ///
    /// This is a lower-level interface that returns the sqlparser AST directly,
    /// useful for tooling that needs to inspect or transform SQL before
    /// converting to relational algebra.
    pub fn parse_to_ast(&self, sql: &str) -> Result<Vec<Statement>> {
        use crate::parser::ProfileDialect;
        use sqlparser::parser::Parser;

        // Use profile-aware dialect that supports custom operators
        let dialect = ProfileDialect::new(self.profile.clone());
        Parser::parse_sql(&dialect, sql)
            .map_err(|e| ParserError::ParseError(e.to_string()))
    }

    /// Get the current profile being used by this parser.
    pub fn profile(&self) -> &ParserProfile {
        &self.profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_parser_creation() {
        let parser = RaParser::universal();
        assert_eq!(parser.profile().name(), "universal");
    }

    #[test]
    fn test_parse_simple_select() {
        let parser = RaParser::universal();
        let result = parser.parse("SELECT id, name FROM users WHERE active = true");
        assert!(result.is_ok(), "Failed to parse simple SELECT");
    }

    #[test]
    fn test_parse_to_ast() {
        let parser = RaParser::universal();
        let result = parser.parse_to_ast("SELECT 1");
        assert!(result.is_ok());
        let statements = result.unwrap();
        assert_eq!(statements.len(), 1);
    }
}
