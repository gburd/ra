//! SQL:1999 - Common Table Expressions and Advanced Features
//!
//! SQL:1999 (also known as SQL3) introduced major extensions including CTEs, CASE expressions,
//! and triggers.
//!
//! # Key Features
//!
//! - **CTEs (WITH clause)**: Recursive and non-recursive common table expressions
//! - **CASE expressions**: Conditional logic in queries
//! - **Window functions**: ROW_NUMBER, RANK, DENSE_RANK (part of OLAP amendment)
//! - **Triggers**: ON INSERT/UPDATE/DELETE
//! - **Stored procedures**: CREATE PROCEDURE/FUNCTION
//!
//! # Examples
//!
//! ```sql
//! -- Non-recursive CTE
//! WITH sales_summary AS (
//!   SELECT region, SUM(amount) as total
//!   FROM sales
//!   GROUP BY region
//! )
//! SELECT * FROM sales_summary WHERE total > 10000;
//!
//! -- Recursive CTE
//! WITH RECURSIVE tree AS (
//!   SELECT id, parent_id, name FROM nodes WHERE parent_id IS NULL
//!   UNION ALL
//!   SELECT n.id, n.parent_id, n.name
//!   FROM nodes n
//!   JOIN tree t ON n.parent_id = t.id
//! )
//! SELECT * FROM tree;
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL:1999 CTE and CASE extension.
pub struct SQL1999Extension;

impl GrammarExtension for SQL1999Extension {
    fn name(&self) -> &str {
        "sql:1999"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            "WITH", "RECURSIVE",
            "CASE", "WHEN", "THEN", "ELSE", "END",
            "TRIGGER", "BEFORE", "AFTER", "INSTEAD OF",
            "FOR EACH ROW", "FOR EACH STATEMENT",
            "OLD", "NEW",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![]  // No new operators
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // String functions
            "POSITION", "OVERLAY", "CHAR_LENGTH", "OCTET_LENGTH",
            // Type conversion
            "CAST", "COALESCE", "NULLIF",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // CTEs and CASE are already handled by sqlparser-rs
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://en.wikipedia.org/wiki/SQL:1999")
    }

    fn min_version(&self) -> Option<&str> {
        Some("SQL:1999")
    }
}
