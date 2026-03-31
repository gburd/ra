//! SQL:2016 - JSON Support
//!
//! SQL:2016 introduced comprehensive JSON support, allowing relational databases to handle
//! JSON data natively.
//!
//! # Key Features
//!
//! ## JSON Data Type
//!
//! A native JSON data type for storing JSON documents:
//!
//! ```sql
//! CREATE TABLE users (
//!   id INT PRIMARY KEY,
//!   profile JSON
//! );
//! ```
//!
//! ## JSON_TABLE
//!
//! Converts JSON data into a relational table:
//!
//! ```sql
//! SELECT jt.*
//! FROM orders,
//!   JSON_TABLE(order_data, '$.items[*]'
//!     COLUMNS (
//!       item_id INT PATH '$.id',
//!       item_name VARCHAR(100) PATH '$.name',
//!       quantity INT PATH '$.qty',
//!       price DECIMAL(10,2) PATH '$.price'
//!     )
//!   ) AS jt;
//! ```
//!
//! ## JSON Path Expressions
//!
//! SQL:2016 defines a path language for navigating JSON:
//! - `$.store.book[0].title` - Navigate to nested elements
//! - `$.*` - All members of object
//! - `$[*]` - All elements of array
//! - `$..author` - Recursive descent
//!
//! ## JSON Functions
//!
//! ```sql
//! -- Extract scalar value
//! SELECT JSON_VALUE(data, '$.user.name') FROM documents;
//!
//! -- Extract object or array
//! SELECT JSON_QUERY(data, '$.user.addresses') FROM documents;
//!
//! -- Check for existence
//! WHERE JSON_EXISTS(data, '$.user.premium');
//! ```
//!
//! # References
//!
//! - ISO/IEC 9075-2:2016 - SQL/Foundation (JSON support)
//! - [SQL:2016 JSON Features](https://modern-sql.com/blog/2017-06/whats-new-in-sql-2016)

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL:2016 JSON extension.
pub struct SQL2016Extension;

impl GrammarExtension for SQL2016Extension {
    fn name(&self) -> &str {
        "sql:2016"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            "JSON",
            "JSON_TABLE",
            "JSON_EXISTS",
            "JSON_VALUE",
            "JSON_QUERY",
            "JSON_OBJECT",
            "JSON_ARRAY",
            "JSON_ARRAYAGG",
            "JSON_OBJECTAGG",
            "FORMAT",
            "WRAPPER",
            "KEEP",
            "OMIT",
            "QUOTES",
            "EMPTY",
            "ERROR",
            "NULL",
            "ON",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // No special operators in standard SQL:2016
            // (vendors may add their own like PostgreSQL's -> and ->>)
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // JSON query functions
            "JSON_VALUE",
            "JSON_QUERY",
            "JSON_EXISTS",
            "JSON_TABLE",
            // JSON construction
            "JSON_OBJECT",
            "JSON_ARRAY",
            "JSON_ARRAYAGG",
            "JSON_OBJECTAGG",
            // Utilities
            "IS JSON",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // TODO: Implement JSON_TABLE parsing
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://modern-sql.com/blog/2017-06/whats-new-in-sql-2016")
    }

    fn min_version(&self) -> Option<&str> {
        Some("SQL:2016")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_2016_extension() {
        let ext = SQL2016Extension;
        assert_eq!(ext.name(), "sql:2016");

        let keywords = ext.keywords();
        assert!(keywords.contains(&"JSON_TABLE"));
        assert!(keywords.contains(&"JSON_VALUE"));
        assert!(keywords.contains(&"JSON_QUERY"));
    }

    #[test]
    fn test_json_functions() {
        let ext = SQL2016Extension;
        let functions = ext.functions();

        assert!(functions.contains(&"JSON_VALUE"));
        assert!(functions.contains(&"JSON_QUERY"));
        assert!(functions.contains(&"JSON_EXISTS"));
        assert!(functions.contains(&"JSON_TABLE"));
    }
}
