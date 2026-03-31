//! pgvector extension for PostgreSQL - Vector similarity search.
//!
//! pgvector enables efficient vector similarity search in PostgreSQL, supporting
//! embedding-based semantic search for AI/ML applications.
//!
//! # Key Features
//!
//! ## Vector Data Type
//!
//! ```sql
//! CREATE TABLE items (
//!   id SERIAL PRIMARY KEY,
//!   embedding vector(1536)  -- OpenAI ada-002 dimensions
//! );
//! ```
//!
//! ## Similarity Operators
//!
//! - `<->` - L2 distance (Euclidean)
//! - `<#>` - Negative inner product (for maximizing dot product)
//! - `<=>` - Cosine distance
//!
//! ```sql
//! SELECT id, embedding <-> '[1,2,3]' AS distance
//! FROM items
//! ORDER BY embedding <-> '[1,2,3]'
//! LIMIT 10;
//! ```
//!
//! ## Index Types
//!
//! ```sql
//! -- IVFFlat index (faster build, less accurate)
//! CREATE INDEX ON items USING ivfflat (embedding vector_l2_ops) WITH (lists = 100);
//!
//! -- HNSW index (slower build, more accurate)
//! CREATE INDEX ON items USING hnsw (embedding vector_l2_ops);
//! ```
//!
//! # Distance Metrics
//!
//! - **L2 distance** (`<->`): Euclidean distance, good for general embeddings
//! - **Inner product** (`<#>`): Dot product similarity (negate for maximization)
//! - **Cosine distance** (`<=>`): Normalized dot product, good for text embeddings

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// pgvector extension for vector similarity search.
pub struct PgVectorExtension;

impl GrammarExtension for PgVectorExtension {
    fn name(&self) -> &str {
        "pgvector"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // Data type
            "vector",
            // Index types
            "ivfflat", "hnsw",
            // Operator classes
            "vector_l2_ops", "vector_ip_ops", "vector_cosine_ops",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            "<->",  // L2 distance (Euclidean)
            "<#>",  // Negative inner product
            "<=>",  // Cosine distance
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Vector construction
            "vector",
            // Distance functions
            "l2_distance",
            "inner_product",
            "cosine_distance",
            // Vector operations
            "vector_dims",
            "vector_norm",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://github.com/pgvector/pgvector")
    }

    fn min_version(&self) -> Option<&str> {
        Some("0.5.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pgvector_extension() {
        let ext = PgVectorExtension;
        assert_eq!(ext.name(), "pgvector");

        // Check vector keyword
        let keywords = ext.keywords();
        assert!(keywords.contains(&"vector"));
        assert!(keywords.contains(&"ivfflat"));
        assert!(keywords.contains(&"hnsw"));
    }

    #[test]
    fn test_similarity_operators() {
        let ext = PgVectorExtension;
        let operators = ext.operators();

        assert!(operators.contains(&"<->"));  // L2 distance
        assert!(operators.contains(&"<#>"));  // Inner product
        assert!(operators.contains(&"<=>"));  // Cosine distance
    }

    #[test]
    fn test_vector_functions() {
        let ext = PgVectorExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"vector_dims"));
        assert!(functions.contains(&"l2_distance"));
        assert!(functions.contains(&"cosine_distance"));
    }
}
