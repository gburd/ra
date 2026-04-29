//! `pg_trgm` extension for `PostgreSQL` - Trigram text search and similarity.
//!
//! `pg_trgm` provides text similarity measurement and fast text search using
//! trigram matching. Useful for fuzzy text search, typo tolerance, and
//! autocomplete features.
//!
//! # Key Features
//!
//! ## Similarity Operators
//!
//! ```sql
//! -- Similarity operator (%)
//! SELECT * FROM products WHERE name % 'iPhone';
//!
//! -- Distance operator (<->)
//! SELECT name, name <-> 'iPhone' AS distance
//! FROM products
//! ORDER BY name <-> 'iPhone'
//! LIMIT 10;
//! ```
//!
//! ## Pattern Matching
//!
//! ```sql
//! -- Fast LIKE/ILIKE with GIN indexes
//! CREATE INDEX idx_products_name_trgm ON products USING gin (name gin_trgm_ops);
//!
//! SELECT * FROM products WHERE name ILIKE '%phone%';
//! ```
//!
//! ## Similarity Threshold
//!
//! ```sql
//! -- Set similarity threshold (0.0 - 1.0, default 0.3)
//! SET pg_trgm.similarity_threshold = 0.5;
//!
//! -- Use % operator with threshold
//! SELECT * FROM products WHERE name % 'ipone';  -- Finds 'iPhone' with typo
//! ```
//!
//! # Use Cases
//!
//! - **Fuzzy search**: Find results despite typos
//! - **Autocomplete**: Suggest completions based on partial input
//! - **Duplicate detection**: Find similar strings
//! - **Name matching**: Match names with variations

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// `pg_trgm` extension for trigram text search.
pub struct PgTrgmExtension;

impl GrammarExtension for PgTrgmExtension {
    fn name(&self) -> &'static str {
        "pg_trgm"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // Operator classes
            "gin_trgm_ops",
            "gist_trgm_ops",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            "%",      // Similarity operator (text1 % text2 returns true if similar)
            "<->",    // Distance operator (lower = more similar)
            "<%",     // Word similarity (text % pattern)
            "<->>",   // Word distance
            "<<->",   // Strict word similarity
            "<<<->>", // Strict word distance
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Similarity functions
            "similarity",
            "word_similarity",
            "strict_word_similarity",
            // Trigram functions
            "show_trgm",
            "show_limit",
            "set_limit",
            // Index support functions
            "gin_trgm_ops",
            "gist_trgm_ops",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://www.postgresql.org/docs/current/pgtrgm.html")
    }

    fn min_version(&self) -> Option<&str> {
        Some("9.6")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pg_trgm_extension() {
        let ext = PgTrgmExtension;
        assert_eq!(ext.name(), "pg_trgm");

        // Check operator classes
        let keywords = ext.keywords();
        assert!(keywords.contains(&"gin_trgm_ops"));
        assert!(keywords.contains(&"gist_trgm_ops"));
    }

    #[test]
    fn test_similarity_operators() {
        let ext = PgTrgmExtension;
        let operators = ext.operators();

        assert!(operators.contains(&"%")); // Similarity
        assert!(operators.contains(&"<->")); // Distance
        assert!(operators.contains(&"<%")); // Word similarity
    }

    #[test]
    fn test_similarity_functions() {
        let ext = PgTrgmExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"similarity"));
        assert!(functions.contains(&"word_similarity"));
        assert!(functions.contains(&"show_trgm"));
    }
}
