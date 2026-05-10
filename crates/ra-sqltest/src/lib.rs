//! SQL logic test harness for the Ra query optimizer.
//!
//! Implements the `sqllogictest::AsyncDB` trait to verify that Ra can parse
//! and optimize SQL queries without errors. This is a "parse + optimize"
//! harness — it does not execute queries or return actual result sets.

use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use sqllogictest::{AsyncDB, DBOutput, DefaultColumnType};
use thiserror::Error;

/// Errors from the Ra test harness.
#[derive(Error, Debug)]
pub enum RaTestError {
    /// SQL parsing failed.
    #[error("parse error: {0}")]
    Parse(String),
    /// Optimization failed.
    #[error("optimize error: {0}")]
    Optimize(String),
}

/// A test database backed by Ra's parser and optimizer.
///
/// Does not execute queries — only parses SQL to relational algebra
/// and runs the optimizer. Returns synthetic results for validation.
pub struct RaDb {
    optimizer: Optimizer,
}

impl Default for RaDb {
    fn default() -> Self {
        Self::new()
    }
}

impl RaDb {
    /// Create a new test database instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            optimizer: Optimizer::new(),
        }
    }

    /// Parse and optimize a SQL statement, returning whether it succeeded.
    fn parse_and_optimize(&self, sql: &str) -> Result<(), RaTestError> {
        let rel_expr =
            sql_to_relexpr::sql_to_relexpr(sql).map_err(|e| RaTestError::Parse(e.to_string()))?;

        self.optimizer
            .optimize(&rel_expr)
            .map_err(|e| RaTestError::Optimize(e.to_string()))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncDB for RaDb {
    type Error = RaTestError;
    type ColumnType = DefaultColumnType;

    async fn shutdown(&mut self) {}

    async fn run(
        &mut self,
        sql: &str,
    ) -> Result<DBOutput<Self::ColumnType>, Self::Error> {
        // Skip non-query statements (DDL, DML)
        let trimmed = sql.trim().to_uppercase();
        if trimmed.starts_with("CREATE")
            || trimmed.starts_with("DROP")
            || trimmed.starts_with("INSERT")
            || trimmed.starts_with("UPDATE")
            || trimmed.starts_with("DELETE")
            || trimmed.starts_with("ALTER")
            || trimmed.starts_with("TRUNCATE")
            || trimmed.starts_with("SET")
            || trimmed.starts_with("BEGIN")
            || trimmed.starts_with("COMMIT")
            || trimmed.starts_with("ROLLBACK")
        {
            return Ok(DBOutput::StatementComplete(0));
        }

        // Try to parse and optimize the query
        self.parse_and_optimize(sql)?;

        // Return a synthetic single-row result to satisfy the test framework.
        Ok(DBOutput::Rows {
            types: vec![DefaultColumnType::Text],
            rows: vec![vec!["ok".to_string()]],
        })
    }
}
