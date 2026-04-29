//! sqlite-vec extension for `SQLite` - Vector similarity search.
//!
//! sqlite-vec is a `SQLite` extension that provides vector similarity search capabilities
//! similar to pgvector, enabling efficient semantic search in `SQLite` databases.
//!
//! # Key Features
//!
//! ## Vector Types
//!
//! ```sql
//! CREATE TABLE items (
//!   id INTEGER PRIMARY KEY,
//!   embedding BLOB  -- Store as BLOB, created with vec_f32()
//! );
//! ```
//!
//! ## Vector Functions
//!
//! ### Construction
//! - `vec_f32(text)` - Create float32 vector from JSON array string
//! - `vec_normalize(vector)` - Normalize vector to unit length
//!
//! ### Operations
//! - `vec_add(v1, v2)` - Element-wise vector addition
//! - `vec_sub(v1, v2)` - Element-wise vector subtraction
//! - `vec_slice(vector, start, end)` - Extract vector slice
//!
//! ### Distance Metrics
//! - `vec_distance_l2(v1, v2)` - Euclidean (L2) distance
//! - `vec_distance_cosine(v1, v2)` - Cosine distance (1 - cosine similarity)
//!
//! # Usage Examples
//!
//! ```sql
//! -- Insert vectors
//! INSERT INTO items (embedding)
//! VALUES (vec_f32('[1.0, 2.0, 3.0]'));
//!
//! -- K-nearest neighbors search
//! SELECT id, vec_distance_l2(embedding, vec_f32('[1,2,3]')) AS distance
//! FROM items
//! ORDER BY distance
//! LIMIT 10;
//!
//! -- Threshold-based search
//! SELECT id, vec_distance_cosine(embedding, vec_f32('[1,2,3]')) AS similarity
//! FROM items
//! WHERE vec_distance_cosine(embedding, vec_f32('[1,2,3]')) < 0.5;
//!
//! -- Combined filter and similarity
//! SELECT id, vec_distance_cosine(embedding, query_vec) AS similarity
//! FROM items
//! WHERE category = 'products'
//!   AND vec_distance_cosine(embedding, query_vec) < 0.5
//! ORDER BY similarity
//! LIMIT 10;
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// sqlite-vec extension for vector similarity search in `SQLite`.
pub struct SqliteVecExtension;

impl GrammarExtension for SqliteVecExtension {
    fn name(&self) -> &'static str {
        "sqlite-vec"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // No custom keywords - uses standard SQL with functions
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // sqlite-vec uses functions instead of operators
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Vector construction
            "vec_f32",
            "vec_int8",
            "vec_bit",
            // Distance functions
            "vec_distance_l2",
            "vec_distance_cosine",
            "vec_distance_l1",
            // Vector operations
            "vec_length",
            "vec_normalize",
            "vec_add",
            "vec_sub",
            "vec_slice",
            // Metadata
            "vec_to_json",
            "vec_type",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://github.com/asg017/sqlite-vec")
    }

    fn min_version(&self) -> Option<&str> {
        Some("0.1.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_vec_extension() {
        let ext = SqliteVecExtension;
        assert_eq!(ext.name(), "sqlite-vec");

        // Check functions
        let functions = ext.functions();
        assert!(functions.contains(&"vec_f32"));
        assert!(functions.contains(&"vec_distance_l2"));
        assert!(functions.contains(&"vec_distance_cosine"));
        assert!(functions.contains(&"vec_normalize"));
        assert!(functions.contains(&"vec_add"));
        assert!(functions.contains(&"vec_sub"));
        assert!(functions.contains(&"vec_slice"));
    }

    #[test]
    fn test_vector_functions() {
        let ext = SqliteVecExtension;
        let functions = ext.functions();

        // Construction functions
        assert!(functions.contains(&"vec_f32"));
        assert!(functions.contains(&"vec_int8"));
        assert!(functions.contains(&"vec_bit"));

        // Distance functions
        assert!(functions.contains(&"vec_distance_l2"));
        assert!(functions.contains(&"vec_distance_cosine"));
        assert!(functions.contains(&"vec_distance_l1"));

        // Operations
        assert!(functions.contains(&"vec_length"));
        assert!(functions.contains(&"vec_normalize"));
    }

    #[test]
    fn test_documentation() {
        let ext = SqliteVecExtension;
        assert!(ext.documentation_url().is_some());
        assert!(ext.min_version().is_some());
    }
}
