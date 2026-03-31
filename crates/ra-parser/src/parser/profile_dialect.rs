//! Profile-aware SQL dialect for sqlparser-rs.
//!
//! This module provides a custom dialect implementation that respects
//! the operators and syntax defined in a parser profile.

use crate::profile::ParserProfile;
use sqlparser::dialect::{Dialect, PostgreSqlDialect};

/// A custom SQL dialect configured from a parser profile.
///
/// This dialect wraps sqlparser's base dialect and adds support for
/// profile-specific operators and syntax extensions.
#[derive(Debug)]
pub struct ProfileDialect {
    profile: ParserProfile,
    base_dialect: PostgreSqlDialect,
}

impl ProfileDialect {
    /// Create a new profile-aware dialect.
    pub fn new(profile: ParserProfile) -> Self {
        Self {
            profile,
            base_dialect: PostgreSqlDialect {},
        }
    }
}

impl Dialect for ProfileDialect {
    fn is_delimited_identifier_start(&self, ch: char) -> bool {
        // Support backticks for MySQL compatibility if in profile
        if self.profile.syntax.get("backticks").map_or(false, |v| v == "true") {
            ch == '`' || self.base_dialect.is_delimited_identifier_start(ch)
        } else {
            self.base_dialect.is_delimited_identifier_start(ch)
        }
    }

    fn is_identifier_start(&self, ch: char) -> bool {
        self.base_dialect.is_identifier_start(ch)
    }

    fn is_identifier_part(&self, ch: char) -> bool {
        self.base_dialect.is_identifier_part(ch)
    }

    fn supports_filter_during_aggregation(&self) -> bool {
        self.base_dialect.supports_filter_during_aggregation()
    }

    fn supports_within_after_array_aggregation(&self) -> bool {
        self.base_dialect.supports_within_after_array_aggregation()
    }

    fn supports_group_by_expr(&self) -> bool {
        self.base_dialect.supports_group_by_expr()
    }

    fn supports_connect_by(&self) -> bool {
        // Enable for Oracle dialect
        self.profile.vendor.as_ref().map_or(false, |v| v == "oracle")
    }

    fn parse_prefix(&self, parser: &mut sqlparser::parser::Parser) -> Option<Result<sqlparser::ast::Expr, sqlparser::parser::ParserError>> {
        // Check for custom operators from profile
        let token = parser.peek_token();

        // Handle @ operators for BSON/JSONB
        if let Some(ref tok_str) = token.to_string().strip_prefix('@') {
            // Check if this operator is in our profile
            let op = format!("@{}", tok_str);
            if self.profile.operators.iter().any(|o| o.starts_with(&op)) {
                // Let the base parser handle it
                return self.base_dialect.parse_prefix(parser);
            }
        }

        self.base_dialect.parse_prefix(parser)
    }

    fn parse_infix(&self, parser: &mut sqlparser::parser::Parser, expr: &sqlparser::ast::Expr, precedence: u8) -> Option<Result<sqlparser::ast::Expr, sqlparser::parser::ParserError>> {
        self.base_dialect.parse_infix(parser, expr, precedence)
    }

    fn get_next_precedence(&self, parser: &sqlparser::parser::Parser) -> Option<Result<u8, sqlparser::parser::ParserError>> {
        // Check if next token is a custom operator from profile
        let token = parser.peek_token();
        let token_str = token.to_string();

        // For operators in our profile, assign appropriate precedence
        if self.profile.operators.iter().any(|op| *op == token_str) {
            // Use same precedence as comparison operators for @ operators
            if token_str.starts_with('@') {
                return Some(Ok(20));  // Comparison precedence
            }
        }

        self.base_dialect.get_next_precedence(parser)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_dialect_creation() {
        let profile = ParserProfile::universal();
        let _dialect = ProfileDialect::new(profile);
    }

    #[test]
    fn test_backtick_support() {
        let mut profile = ParserProfile::universal();
        profile.syntax.insert("backticks".to_string(), "true".to_string());

        let dialect = ProfileDialect::new(profile);
        assert!(dialect.is_delimited_identifier_start('`'));
    }
}
