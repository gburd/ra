//! SQL-92 - Foundation Standard
//!
//! SQL-92 (also known as SQL2) is the foundation of modern SQL. It defines the core
//! relational operations that are supported by virtually all SQL databases.
//!
//! # Key Features
//!
//! - **Basic DML**: SELECT, INSERT, UPDATE, DELETE
//! - **Joins**: INNER JOIN, LEFT/RIGHT/FULL OUTER JOIN, CROSS JOIN
//! - **Subqueries**: Scalar, row, and table subqueries
//! - **Set operations**: UNION, INTERSECT, EXCEPT
//! - **Aggregations**: GROUP BY, HAVING
//! - **DDL**: CREATE/ALTER/DROP TABLE, VIEW, INDEX
//! - **Constraints**: PRIMARY KEY, FOREIGN KEY, UNIQUE, CHECK
//! - **Transactions**: BEGIN, COMMIT, ROLLBACK
//! - **Cursors**: DECLARE, OPEN, FETCH, CLOSE
//!
//! SQL-92 defines three conformance levels:
//! - **Entry SQL**: Minimal features
//! - **Intermediate SQL**: Mid-level features
//! - **Full SQL**: Complete standard
//!
//! Most modern databases implement Entry SQL plus additional features.

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL-92 foundational extension.
pub struct SQL92Extension;

impl GrammarExtension for SQL92Extension {
    fn name(&self) -> &'static str {
        "sql-92"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // Core DML
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "FROM",
            "WHERE",
            "GROUP BY",
            "HAVING",
            "ORDER BY",
            // Joins
            "JOIN",
            "INNER",
            "LEFT",
            "RIGHT",
            "FULL",
            "OUTER",
            "CROSS",
            "ON",
            "USING",
            // Set operations
            "UNION",
            "INTERSECT",
            "EXCEPT",
            "ALL",
            "DISTINCT",
            // Subqueries
            "EXISTS",
            "IN",
            "ANY",
            "SOME",
            "ALL",
            // Aggregates
            "COUNT",
            "SUM",
            "AVG",
            "MIN",
            "MAX",
            // DDL
            "CREATE",
            "ALTER",
            "DROP",
            "TABLE",
            "VIEW",
            "INDEX",
            // Constraints
            "PRIMARY",
            "KEY",
            "FOREIGN",
            "REFERENCES",
            "UNIQUE",
            "CHECK",
            "NOT",
            "NULL",
            // Transactions
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "TRANSACTION",
            // Data types
            "CHAR",
            "VARCHAR",
            "INT",
            "INTEGER",
            "SMALLINT",
            "NUMERIC",
            "DECIMAL",
            "FLOAT",
            "REAL",
            "DOUBLE",
            "PRECISION",
            "DATE",
            "TIME",
            "TIMESTAMP",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            "=", "<>", "!=", "<", ">", "<=", ">=", "AND", "OR", "NOT", "LIKE", "BETWEEN", "+", "-",
            "*", "/", "||", // String concatenation
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Aggregates
            "COUNT",
            "SUM",
            "AVG",
            "MIN",
            "MAX",
            // String functions
            "SUBSTRING",
            "UPPER",
            "LOWER",
            "TRIM",
            // Numeric
            "ABS",
            "MOD",
            // Date/time
            "CURRENT_DATE",
            "CURRENT_TIME",
            "CURRENT_TIMESTAMP",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // SQL-92 is already handled by sqlparser-rs core
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://en.wikipedia.org/wiki/SQL-92")
    }

    fn min_version(&self) -> Option<&str> {
        Some("SQL-92")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_92_extension() {
        let ext = SQL92Extension;
        assert_eq!(ext.name(), "sql-92");

        let keywords = ext.keywords();
        assert!(keywords.contains(&"SELECT"));
        assert!(keywords.contains(&"JOIN"));
        assert!(keywords.contains(&"GROUP BY"));
    }
}
