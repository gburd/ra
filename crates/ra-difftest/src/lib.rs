//! Differential testing framework for Ra vs native PostgreSQL.
//!
//! Connects to two PostgreSQL instances (one with Ra extension loaded,
//! one without) and verifies that query results match exactly.

use anyhow::{Context, Result};
use tokio_postgres::{Client, NoTls, Row};

/// Result of comparing a query's output between Ra and native PG.
#[derive(Debug)]
pub enum DiffResult {
    /// Results match perfectly.
    Match { rows: usize },
    /// Results differ.
    Mismatch {
        expected_rows: usize,
        actual_rows: usize,
        first_diff_row: Option<usize>,
        details: String,
    },
    /// Ra produced an error.
    RaError(String),
    /// Native PG produced an error.
    NativeError(String),
}

/// Differential test runner comparing Ra vs native PostgreSQL.
pub struct DiffTestRunner {
    ra_client: Client,
    native_client: Client,
}

impl DiffTestRunner {
    /// Connect to both PostgreSQL instances.
    ///
    /// `ra_url` - Connection string for PG with Ra extension loaded
    /// `native_url` - Connection string for PG without Ra extension
    pub async fn connect(ra_url: &str, native_url: &str) -> Result<Self> {
        let (ra_client, ra_conn) = tokio_postgres::connect(ra_url, NoTls)
            .await
            .context("connecting to Ra PostgreSQL")?;

        tokio::spawn(async move {
            if let Err(e) = ra_conn.await {
                tracing::error!("Ra connection error: {}", e);
            }
        });

        let (native_client, native_conn) = tokio_postgres::connect(native_url, NoTls)
            .await
            .context("connecting to native PostgreSQL")?;

        tokio::spawn(async move {
            if let Err(e) = native_conn.await {
                tracing::error!("Native connection error: {}", e);
            }
        });

        // Enable Ra on the Ra connection
        ra_client
            .execute("SET ra_planner.enabled = true", &[])
            .await
            .context("enabling Ra planner")?;

        Ok(Self {
            ra_client,
            native_client,
        })
    }

    /// Run a SELECT query through both planners and compare results.
    ///
    /// `ordered` - if true, compare row-by-row (query has ORDER BY);
    ///             if false, sort both result sets before comparison.
    pub async fn compare_query(&self, sql: &str, ordered: bool) -> DiffResult {
        let ra_result = self.ra_client.query(sql, &[]).await;
        let native_result = self.native_client.query(sql, &[]).await;

        let ra_rows = match ra_result {
            Ok(rows) => rows,
            Err(e) => return DiffResult::RaError(e.to_string()),
        };

        let native_rows = match native_result {
            Ok(rows) => rows,
            Err(e) => return DiffResult::NativeError(e.to_string()),
        };

        compare_row_sets(&ra_rows, &native_rows, ordered)
    }

    /// Run a DML statement and verify the resulting table state matches.
    ///
    /// 1. Runs `setup_sql` on both connections (e.g., CREATE TABLE, INSERT seed data)
    /// 2. Runs `dml_sql` on both connections
    /// 3. Runs `verify_sql` on both and compares results
    pub async fn compare_dml(
        &self,
        setup_sql: &[&str],
        dml_sql: &str,
        verify_sql: &str,
    ) -> DiffResult {
        // Setup
        for sql in setup_sql {
            if let Err(e) = self.ra_client.execute(*sql, &[]).await {
                return DiffResult::RaError(format!("setup failed: {e}"));
            }
            if let Err(e) = self.native_client.execute(*sql, &[]).await {
                return DiffResult::NativeError(format!("setup failed: {e}"));
            }
        }

        // Execute DML
        if let Err(e) = self.ra_client.execute(dml_sql, &[]).await {
            return DiffResult::RaError(format!("DML failed: {e}"));
        }
        if let Err(e) = self.native_client.execute(dml_sql, &[]).await {
            return DiffResult::NativeError(format!("DML failed: {e}"));
        }

        // Verify
        self.compare_query(verify_sql, false).await
    }

    /// Run a DML statement with RETURNING and compare the returned rows.
    ///
    /// Uses `query()` instead of `execute()` so RETURNING rows are captured.
    ///
    /// 1. Runs `setup_sql` on both connections
    /// 2. Runs `dml_sql` via `query()` on both and compares RETURNING results
    pub async fn compare_dml_returning(
        &self,
        setup_sql: &[&str],
        dml_sql: &str,
        ordered: bool,
    ) -> DiffResult {
        // Setup
        for sql in setup_sql {
            if let Err(e) = self.ra_client.execute(*sql, &[]).await {
                return DiffResult::RaError(format!("setup failed: {e}"));
            }
            if let Err(e) = self.native_client.execute(*sql, &[]).await {
                return DiffResult::NativeError(format!("setup failed: {e}"));
            }
        }

        // Execute DML with RETURNING via query()
        let ra_result = self.ra_client.query(dml_sql, &[]).await;
        let native_result = self.native_client.query(dml_sql, &[]).await;

        let ra_rows = match ra_result {
            Ok(rows) => rows,
            Err(e) => return DiffResult::RaError(format!("DML failed: {e}")),
        };

        let native_rows = match native_result {
            Ok(rows) => rows,
            Err(e) => return DiffResult::NativeError(format!("DML failed: {e}")),
        };

        compare_row_sets(&ra_rows, &native_rows, ordered)
    }
}

/// Compare two result sets for equality.
fn compare_row_sets(ra_rows: &[Row], native_rows: &[Row], ordered: bool) -> DiffResult {
    let mut ra_strings = rows_to_strings(ra_rows);
    let mut native_strings = rows_to_strings(native_rows);

    if !ordered {
        ra_strings.sort();
        native_strings.sort();
    }

    if ra_strings.len() != native_strings.len() {
        return DiffResult::Mismatch {
            expected_rows: native_strings.len(),
            actual_rows: ra_strings.len(),
            first_diff_row: None,
            details: format!(
                "row count mismatch: Ra={}, Native={}",
                ra_strings.len(),
                native_strings.len()
            ),
        };
    }

    for (i, (ra_row, native_row)) in ra_strings.iter().zip(native_strings.iter()).enumerate() {
        if ra_row != native_row {
            return DiffResult::Mismatch {
                expected_rows: native_strings.len(),
                actual_rows: ra_strings.len(),
                first_diff_row: Some(i),
                details: format!(
                    "row {i} differs:\n  Ra:     {ra_row}\n  Native: {native_row}"
                ),
            };
        }
    }

    DiffResult::Match {
        rows: ra_strings.len(),
    }
}

/// Convert rows to string representation for comparison.
///
/// Uses PostgreSQL's text output format (NULL represented as "NULL").
fn rows_to_strings(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let ncols = row.len();
            let mut parts = Vec::with_capacity(ncols);
            for i in 0..ncols {
                let val: Option<String> = row.try_get(i).ok().flatten();
                parts.push(val.unwrap_or_else(|| "NULL".to_string()));
            }
            parts.join("|")
        })
        .collect()
}

/// Pre-built test queries for TPC-H validation.
pub mod tpch {
    /// TPC-H Query 1 (Pricing Summary Report)
    pub const Q1: &str = r#"
        SELECT l_returnflag, l_linestatus,
               sum(l_quantity) as sum_qty,
               sum(l_extendedprice) as sum_base_price
        FROM lineitem
        WHERE l_shipdate <= date '1998-12-01' - interval '90 day'
        GROUP BY l_returnflag, l_linestatus
        ORDER BY l_returnflag, l_linestatus
    "#;

    /// TPC-H Query 6 (Forecasting Revenue Change)
    pub const Q6: &str = r#"
        SELECT sum(l_extendedprice * l_discount) as revenue
        FROM lineitem
        WHERE l_shipdate >= date '1994-01-01'
          AND l_shipdate < date '1994-01-01' + interval '1 year'
          AND l_discount between 0.06 - 0.01 AND 0.06 + 0.01
          AND l_quantity < 24
    "#;
}
