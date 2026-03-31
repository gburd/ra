//! SQL:2011 - Temporal Tables (System-Versioned)
//!
//! SQL:2011 introduced support for temporal databases, allowing databases to track
//! historical data automatically.
//!
//! # Key Features
//!
//! ## System-Versioned Tables
//!
//! ```sql
//! CREATE TABLE employees (
//!   id INT PRIMARY KEY,
//!   name VARCHAR(100),
//!   salary DECIMAL(10,2),
//!   sys_start TIMESTAMP(12) GENERATED ALWAYS AS ROW START,
//!   sys_end TIMESTAMP(12) GENERATED ALWAYS AS ROW END,
//!   PERIOD FOR SYSTEM_TIME (sys_start, sys_end)
//! ) WITH SYSTEM VERSIONING;
//! ```
//!
//! ## Temporal Queries
//!
//! ```sql
//! -- Current data
//! SELECT * FROM employees;
//!
//! -- Data as of specific time
//! SELECT * FROM employees
//! FOR SYSTEM_TIME AS OF TIMESTAMP '2024-01-01 00:00:00';
//!
//! -- Data between two times
//! SELECT * FROM employees
//! FOR SYSTEM_TIME BETWEEN
//!   TIMESTAMP '2024-01-01 00:00:00'
//!   AND TIMESTAMP '2024-12-31 23:59:59';
//!
//! -- All historical versions
//! SELECT * FROM employees
//! FOR SYSTEM_TIME FROM
//!   TIMESTAMP '2024-01-01 00:00:00'
//!   TO TIMESTAMP '2024-12-31 23:59:59';
//! ```
//!
//! ## Application-Time Period Tables
//!
//! Track valid time (business time) separately from system time:
//!
//! ```sql
//! CREATE TABLE insurance_policies (
//!   id INT PRIMARY KEY,
//!   customer_id INT,
//!   valid_from DATE,
//!   valid_to DATE,
//!   PERIOD FOR valid_time (valid_from, valid_to)
//! );
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL:2011 temporal tables extension.
pub struct SQL2011Extension;

impl GrammarExtension for SQL2011Extension {
    fn name(&self) -> &str {
        "sql:2011"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // System versioning
            "SYSTEM_TIME", "SYSTEM VERSIONING",
            "FOR SYSTEM_TIME",
            "AS OF", "BETWEEN", "FROM", "TO",
            "CONTAINED IN",
            // Periods
            "PERIOD", "ROW START", "ROW END",
            "WITHOUT OVERLAPS",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![]
    }

    fn functions(&self) -> Vec<&str> {
        vec![]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // Temporal tables not yet in sqlparser-rs
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://en.wikipedia.org/wiki/SQL:2011")
    }

    fn min_version(&self) -> Option<&str> {
        Some("SQL:2011")
    }
}
