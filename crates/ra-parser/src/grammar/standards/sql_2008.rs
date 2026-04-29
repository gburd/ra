//! SQL:2008 - MERGE Statement and Enhanced Features
//!
//! SQL:2008 refined earlier standards and introduced the MERGE statement (upsert).
//!
//! # Key Features
//!
//! ## MERGE Statement
//!
//! ```sql
//! MERGE INTO target_table t
//! USING source_table s
//! ON t.id = s.id
//! WHEN MATCHED THEN
//!   UPDATE SET t.value = s.value, t.updated_at = CURRENT_TIMESTAMP
//! WHEN NOT MATCHED THEN
//!   INSERT (id, value, created_at)
//!   VALUES (s.id, s.value, CURRENT_TIMESTAMP);
//! ```
//!
//! ## Other Features
//!
//! - **TRUNCATE TABLE**: Fast table deletion
//! - **Enhanced datetime**: INTERVAL types, datetime arithmetic
//! - **ORDER BY in aggregates**: `string_agg(name ORDER BY name)`
//! - **FETCH FIRST/NEXT**: Standard pagination syntax

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL:2008 MERGE and enhancements extension.
pub struct SQL2008Extension;

impl GrammarExtension for SQL2008Extension {
    fn name(&self) -> &'static str {
        "sql:2008"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            "MERGE",
            "USING",
            "WHEN MATCHED",
            "WHEN NOT MATCHED",
            "TRUNCATE",
            "FETCH",
            "FIRST",
            "NEXT",
            "ONLY",
            "OFFSET",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![]
    }

    fn functions(&self) -> Vec<&str> {
        vec![]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // MERGE already handled by sqlparser-rs
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://en.wikipedia.org/wiki/SQL:2008")
    }

    fn min_version(&self) -> Option<&str> {
        Some("SQL:2008")
    }
}
