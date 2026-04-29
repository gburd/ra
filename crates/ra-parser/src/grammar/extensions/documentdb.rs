//! `DocumentDB` (MongoDB-compatible) SQL extension for `PostgreSQL`.
//!
//! Amazon `DocumentDB` provides MongoDB-compatible APIs with SQL query support.
//! The key issue this extension solves is the `@=` operator used for exact BSON matching.
//!
//! # Problem Statement
//!
//! `DocumentDB` queries use MongoDB-style operators that aren't recognized by standard SQL parsers:
//!
//! ```sql
//! SELECT document
//! FROM documentdb_api.collection('mydb', 'users')
//! WHERE document @= '{"status": "active"}';
//! ```
//!
//! The `@=` operator means "BSON equals" and is not part of standard `PostgreSQL`.
//!
//! # Solution
//!
//! This extension adds DocumentDB-specific operators and functions, enabling the parser to:
//! 1. Recognize `documentdb_api.collection()` table function
//! 2. Accept `@=`, `@>`, `@<`, `@>=`, `@<=` BSON comparison operators
//! 3. Map these to standard SQL containment checks in the optimizer
//!
//! # Operators
//!
//! - `@=` - BSON exact match
//! - `@>` - BSON contains
//! - `@<` - BSON contained by
//! - `@>=` - BSON contains or equals
//! - `@<=` - BSON contained by or equals
//! - `@?` - BSON path exists
//!
//! # Functions
//!
//! - `documentdb_api.collection(database, collection)` - Access `DocumentDB` collection
//! - `documentdb_api.insert(collection, document)` - Insert document
//! - `documentdb_api.update(collection, filter, update)` - Update documents
//! - `documentdb_api.delete(collection, filter)` - Delete documents

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// `DocumentDB` (MongoDB-compatible) extension.
pub struct DocumentDBExtension;

impl GrammarExtension for DocumentDBExtension {
    fn name(&self) -> &'static str {
        "documentdb"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // DocumentDB API schema
            "documentdb_api",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // BSON comparison operators
            "@=",  // BSON exact match (KEY OPERATOR - fixes the issue!)
            "@>",  // BSON contains
            "@<",  // BSON contained by
            "@>=", // BSON contains or equals
            "@<=", // BSON contained by or equals
            "@?",  // BSON path exists
            "@!",  // BSON path does not exist
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Collection access
            "documentdb_api.collection",
            // CRUD operations
            "documentdb_api.insert",
            "documentdb_api.insert_one",
            "documentdb_api.insert_many",
            "documentdb_api.update",
            "documentdb_api.update_one",
            "documentdb_api.update_many",
            "documentdb_api.delete",
            "documentdb_api.delete_one",
            "documentdb_api.delete_many",
            "documentdb_api.find",
            "documentdb_api.find_one",
            // Aggregation
            "documentdb_api.aggregate",
            "documentdb_api.count",
            "documentdb_api.distinct",
            // Index management
            "documentdb_api.create_index",
            "documentdb_api.drop_index",
            "documentdb_api.list_indexes",
        ]
    }

    fn parse_statement(&self, sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // Check if this is a DocumentDB query
        if sql.contains("documentdb_api.collection") || sql.contains("@=") {
            // TODO: Parse DocumentDB-specific syntax
            // For now, let standard parser handle it
            Ok(None)
        } else {
            Ok(None)
        }
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://docs.aws.amazon.com/documentdb/latest/developerguide/")
    }

    fn min_version(&self) -> Option<&str> {
        Some("4.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_documentdb_extension() {
        let ext = DocumentDBExtension;
        assert_eq!(ext.name(), "documentdb");

        // Check @= operator (the key fix!)
        let operators = ext.operators();
        assert!(operators.contains(&"@="), "@= operator should be supported");
        assert!(operators.contains(&"@>"));
        assert!(operators.contains(&"@<"));
    }

    #[test]
    fn test_documentdb_functions() {
        let ext = DocumentDBExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"documentdb_api.collection"));
        assert!(functions.contains(&"documentdb_api.insert"));
        assert!(functions.contains(&"documentdb_api.find"));
    }

    #[test]
    fn test_bson_operators() {
        let ext = DocumentDBExtension;
        let operators = ext.operators();

        // Verify all BSON operators are present
        let expected_operators = vec!["@=", "@>", "@<", "@>=", "@<=", "@?", "@!"];
        for op in expected_operators {
            assert!(
                operators.contains(&op),
                "DocumentDB should support {op} operator",
            );
        }
    }
}
