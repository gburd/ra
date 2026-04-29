//! SQL:2003 - Window Functions and XML Support
//!
//! SQL:2003 introduced window functions (also called analytic functions) and XML support.
//!
//! # Key Features
//!
//! ## Window Functions
//!
//! ```sql
//! SELECT
//!   employee_id,
//!   salary,
//!   ROW_NUMBER() OVER (ORDER BY salary DESC) as rank,
//!   AVG(salary) OVER (PARTITION BY department_id) as dept_avg
//! FROM employees;
//! ```
//!
//! Window functions include:
//! - **Ranking**: `ROW_NUMBER`, RANK, `DENSE_RANK`, NTILE
//! - **Offset**: LAG, LEAD, `FIRST_VALUE`, `LAST_VALUE`
//! - **Aggregates**: All standard aggregates (SUM, AVG, etc.) with OVER clause
//!
//! ## XML Support
//!
//! - `XMLType` data type
//! - XML construction: XMLELEMENT, XMLATTRIBUTES, XMLAGG
//! - XML query: XMLEXISTS, XMLQUERY, XMLTABLE
//!
//! ## Other Features
//!
//! - SEQUENCE objects
//! - IDENTITY columns (auto-increment)
//! - MERGE statement (upsert)

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL:2003 window functions and XML extension.
pub struct SQL2003Extension;

impl GrammarExtension for SQL2003Extension {
    fn name(&self) -> &'static str {
        "sql:2003"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // Window functions
            "OVER",
            "PARTITION BY",
            "ROWS",
            "RANGE",
            "PRECEDING",
            "FOLLOWING",
            "UNBOUNDED",
            "CURRENT ROW",
            // XML
            "XMLTYPE",
            "XMLELEMENT",
            "XMLATTRIBUTES",
            "XMLAGG",
            "XMLEXISTS",
            "XMLQUERY",
            "XMLTABLE",
            // Sequences
            "SEQUENCE",
            "NEXTVAL",
            "CURRVAL",
            // Identity columns
            "IDENTITY",
            "GENERATED",
            "ALWAYS",
            "BY DEFAULT",
            // MERGE
            "MERGE",
            "MATCHED",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Window/ranking functions
            "ROW_NUMBER",
            "RANK",
            "DENSE_RANK",
            "NTILE",
            "LAG",
            "LEAD",
            "FIRST_VALUE",
            "LAST_VALUE",
            "PERCENT_RANK",
            "CUME_DIST",
            // XML functions
            "XMLELEMENT",
            "XMLATTRIBUTES",
            "XMLAGG",
            "XMLEXISTS",
            "XMLQUERY",
            "XMLTABLE",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // Window functions already handled by sqlparser-rs
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://en.wikipedia.org/wiki/SQL:2003")
    }

    fn min_version(&self) -> Option<&str> {
        Some("SQL:2003")
    }
}
