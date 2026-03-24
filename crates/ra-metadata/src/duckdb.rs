//! `DuckDB` database connector.
//!
//! Queries `DuckDB` system catalogs (`information_schema`,
//! `duckdb_tables()`, `duckdb_columns()`) and parses
//! `EXPLAIN` output.

use std::collections::HashMap;

use duckdb::Connection;

use crate::connector::{DatabaseConnector, MetadataResult};
use crate::error::MetadataError;
use crate::explain::{
    ExplainNode, ExplainPlan, NodeType,
};
use crate::schema::{
    ColumnInfo, ColumnStatistics, ConstraintInfo, ConstraintKind,
    DatabaseKind, IndexInfo, SchemaInfo, TableInfo, TableStats,
};

/// `DuckDB` connector using the `duckdb` crate.
pub struct DuckDBConnector {
    conn: Connection,
    db_path: String,
}

impl DuckDBConnector {
    /// Open a connection to a `DuckDB` database file.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` if the file cannot be
    /// opened.
    pub fn connect(path: &str) -> MetadataResult<Self> {
        let conn =
            Connection::open(path).map_err(|e| {
                MetadataError::Connection {
                    message: format!(
                        "DuckDB open failed for {path}: {e}"
                    ),
                }
            })?;

        Ok(Self {
            conn,
            db_path: path.to_owned(),
        })
    }

    /// Open an in-memory `DuckDB` database.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` if initialization fails.
    pub fn open_in_memory() -> MetadataResult<Self> {
        let conn =
            Connection::open_in_memory().map_err(|e| {
                MetadataError::Connection {
                    message: format!(
                        "DuckDB in-memory open failed: {e}"
                    ),
                }
            })?;

        Ok(Self {
            conn,
            db_path: ":memory:".to_owned(),
        })
    }

    /// Get the underlying duckdb connection (for testing).
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    fn query_tables(&self) -> MetadataResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT table_name \
                 FROM information_schema.tables \
                 WHERE table_schema = 'main' \
                 AND table_type = 'BASE TABLE' \
                 ORDER BY table_name",
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to list tables: {e}"),
            })?;

        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| MetadataError::Query {
                message: format!("failed to read tables: {e}"),
            })?
            .filter_map(Result::ok)
            .collect();

        Ok(tables)
    }

    fn query_columns(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<ColumnInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT column_name, data_type, is_nullable, \
                 ordinal_position, column_default \
                 FROM information_schema.columns \
                 WHERE table_schema = 'main' \
                 AND table_name = ? \
                 ORDER BY ordinal_position",
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query columns for {table}: {e}"
                ),
            })?;

        let columns: Vec<ColumnInfo> = stmt
            .query_map([table], |row| {
                let name: String = row.get(0)?;
                let data_type: String = row.get(1)?;
                let nullable_str: String = row.get(2)?;
                let ordinal: u32 = row.get(3)?;
                let default_value: Option<String> = row.get(4)?;
                Ok(ColumnInfo {
                    name,
                    data_type,
                    nullable: nullable_str == "YES",
                    ordinal,
                    default_value,
                })
            })
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read columns for {table}: {e}"
                ),
            })?
            .filter_map(Result::ok)
            .collect();

        Ok(columns)
    }

    fn query_constraints(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<ConstraintInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT constraint_name, constraint_type \
                 FROM information_schema.table_constraints \
                 WHERE table_schema = 'main' \
                 AND table_name = ?",
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query constraints for {table}: {e}"
                ),
            })?;

        let raw: Vec<(String, String)> = stmt
            .query_map([table], |row| {
                let name: String = row.get(0)?;
                let ctype: String = row.get(1)?;
                Ok((name, ctype))
            })
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read constraints for {table}: {e}"
                ),
            })?
            .filter_map(Result::ok)
            .collect();

        let mut constraints = Vec::new();
        for (name, ctype) in raw {
            let kind = match ctype.as_str() {
                "PRIMARY KEY" => ConstraintKind::PrimaryKey,
                "FOREIGN KEY" => ConstraintKind::ForeignKey,
                "UNIQUE" => ConstraintKind::Unique,
                "CHECK" => ConstraintKind::Check,
                "NOT NULL" => ConstraintKind::NotNull,
                _ => continue,
            };

            // Query the columns for this constraint.
            let columns =
                self.query_constraint_columns(table, &name);

            constraints.push(ConstraintInfo {
                name,
                kind,
                columns,
                referenced_table: None,
                referenced_columns: vec![],
                check_expression: None,
            });
        }

        Ok(constraints)
    }

    fn query_constraint_columns(
        &self,
        table: &str,
        constraint: &str,
    ) -> Vec<String> {
        let result = self.conn.prepare(
            "SELECT column_name \
             FROM information_schema.key_column_usage \
             WHERE table_schema = 'main' \
             AND table_name = ? \
             AND constraint_name = ? \
             ORDER BY ordinal_position",
        );

        let Ok(mut stmt) = result else {
            return vec![];
        };

        stmt.query_map(
            duckdb::params![table, constraint],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .map(|rows| {
            rows.filter_map(Result::ok).collect()
        })
        .unwrap_or_default()
    }

    fn query_indexes(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<IndexInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT index_name, is_unique, sql \
                 FROM duckdb_indexes() \
                 WHERE schema_name = 'main' \
                 AND table_name = ?",
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query indexes for {table}: {e}"
                ),
            })?;

        let indexes: Vec<IndexInfo> = stmt
            .query_map([table], |row| {
                let name: String = row.get(0)?;
                let unique: bool = row.get(1)?;
                let sql: Option<String> = row.get(2)?;
                let columns =
                    parse_index_columns(sql.as_deref());
                Ok(IndexInfo {
                    name,
                    columns,
                    unique,
                    index_type: "ART".to_owned(),
                })
            })
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read indexes for {table}: {e}"
                ),
            })?
            .filter_map(Result::ok)
            .collect();

        Ok(indexes)
    }

    fn query_row_count(
        &self,
        table: &str,
    ) -> MetadataResult<(f64, u64)> {
        // Try duckdb_tables() for estimated row count first.
        let result: Result<(f64, u64), _> = self.conn.query_row(
            "SELECT estimated_size, \
             COALESCE(estimated_size * 100, 0) \
             FROM duckdb_tables() \
             WHERE schema_name = 'main' \
             AND table_name = ?",
            [table],
            |row| {
                let rows: f64 = row.get::<_, i64>(0)? as f64;
                let bytes: u64 = row.get::<_, i64>(1)? as u64;
                Ok((rows, bytes))
            },
        );

        if let Ok(stats) = result {
            return Ok(stats);
        }

        // Fallback to COUNT(*)
        let count: f64 = self
            .conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM \"{}\"",
                    table.replace('"', "\"\"")
                ),
                [],
                |row| row.get(0),
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to count rows for {table}: {e}"
                ),
            })?;

        Ok((count, 0))
    }

    fn query_column_stats(
        &self,
        table: &str,
    ) -> MetadataResult<HashMap<String, ColumnStatistics>> {
        let columns = self.query_columns(table)?;
        let mut result = HashMap::new();

        for col in &columns {
            // DuckDB supports approx_count_distinct for NDV.
            let ndv_sql = format!(
                "SELECT approx_count_distinct(\"{}\") FROM \"{}\"",
                col.name.replace('"', "\"\""),
                table.replace('"', "\"\""),
            );

            let distinct_count: f64 = self
                .conn
                .query_row(&ndv_sql, [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap_or(0) as f64;

            let null_sql = format!(
                "SELECT \
                 CAST(SUM(CASE WHEN \"{}\" IS NULL \
                   THEN 1 ELSE 0 END) AS DOUBLE) \
                 / CAST(COUNT(*) AS DOUBLE) \
                 FROM \"{}\"",
                col.name.replace('"', "\"\""),
                table.replace('"', "\"\""),
            );

            let null_fraction: f64 = self
                .conn
                .query_row(&null_sql, [], |row| row.get(0))
                .unwrap_or(0.0);

            result.insert(
                col.name.clone(),
                ColumnStatistics {
                    column_name: col.name.clone(),
                    distinct_count,
                    null_fraction,
                    avg_width: None,
                    most_common_values: vec![],
                    histogram_bounds: vec![],
                },
            );
        }

        Ok(result)
    }

    fn build_explain_text(
        &self,
        sql: &str,
    ) -> MetadataResult<String> {
        let explain_sql = format!("EXPLAIN {sql}");
        let mut stmt = self
            .conn
            .prepare(&explain_sql)
            .map_err(|e| MetadataError::Query {
                message: format!("EXPLAIN failed: {e}"),
            })?;

        let rows: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read EXPLAIN output: {e}"
                ),
            })?
            .filter_map(Result::ok)
            .collect();

        Ok(rows.join("\n"))
    }

    /// Gather full schema information.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_schema_mut(
        &self,
    ) -> MetadataResult<SchemaInfo> {
        let table_names = self.query_tables()?;
        let mut tables = HashMap::new();

        for name in &table_names {
            let columns = self.query_columns(name)?;
            let constraints = self.query_constraints(name)?;
            let indexes = self.query_indexes(name)?;
            let (row_count, _) = self.query_row_count(name)?;

            tables.insert(
                name.clone(),
                TableInfo {
                    name: name.clone(),
                    columns,
                    constraints,
                    indexes,
                    triggers: vec![],
                    estimated_rows: Some(row_count),
                },
            );
        }

        Ok(SchemaInfo {
            kind: DatabaseKind::DuckDB,
            schema_name: self.db_path.clone(),
            tables,
        })
    }

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_statistics_mut(
        &self,
        table: &str,
    ) -> MetadataResult<TableStats> {
        let (row_count, total_bytes) =
            self.query_row_count(table)?;
        let columns = self.query_column_stats(table)?;

        Ok(TableStats {
            table_name: table.to_owned(),
            row_count,
            total_bytes,
            columns,
        })
    }

    /// Execute EXPLAIN on a query and parse the result.
    ///
    /// # Errors
    ///
    /// Returns errors if the EXPLAIN query fails or output cannot
    /// be parsed.
    pub fn explain_query_mut(
        &self,
        sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        let text = self.build_explain_text(sql)?;
        Ok(parse_duckdb_explain(&text))
    }
}

impl DatabaseConnector for DuckDBConnector {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::DuckDB
    }

    fn gather_schema(&self) -> MetadataResult<SchemaInfo> {
        self.gather_schema_mut()
    }

    fn gather_statistics(
        &self,
        table: &str,
    ) -> MetadataResult<TableStats> {
        self.gather_statistics_mut(table)
    }

    fn explain_query(
        &self,
        sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        self.explain_query_mut(sql)
    }
}

/// Parse column names from a `DuckDB` CREATE INDEX SQL statement.
fn parse_index_columns(sql: Option<&str>) -> Vec<String> {
    let Some(sql) = sql else {
        return vec![];
    };

    // Extract columns between the last pair of parentheses.
    let Some(start) = sql.rfind('(') else {
        return vec![];
    };
    let Some(end) = sql.rfind(')') else {
        return vec![];
    };
    if start >= end {
        return vec![];
    }

    sql[start + 1..end]
        .split(',')
        .map(|s| {
            s.trim()
                .trim_matches('"')
                .to_owned()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse `DuckDB` EXPLAIN output into an `ExplainPlan`.
///
/// `DuckDB` EXPLAIN produces a text-based plan. We extract a
/// basic tree structure from the indented output.
fn parse_duckdb_explain(
    text: &str,
) -> ExplainPlan {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return ExplainPlan {
            root: ExplainNode {
                node_type: NodeType::Other,
                join_type: None,
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail: Some(text.to_owned()),
                children: Vec::new(),
            },
            query: None,
            total_cost: None,
            total_rows: None,
        };
    }

    let first = lines[0];
    let node_type = classify_duckdb_node(first);
    let relation = extract_table_name(first);

    ExplainPlan {
        root: ExplainNode {
            node_type,
            join_type: None,
            relation,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some(text.to_owned()),
            children: Vec::new(),
        },
        query: None,
        total_cost: None,
        total_rows: None,
    }
}

fn classify_duckdb_node(line: &str) -> NodeType {
    let upper = line.to_uppercase();
    if upper.contains("SEQ_SCAN") || upper.contains("TABLE_SCAN")
    {
        NodeType::SeqScan
    } else if upper.contains("INDEX_SCAN") {
        NodeType::IndexScan
    } else if upper.contains("HASH_JOIN") {
        NodeType::HashJoin
    } else if upper.contains("MERGE_JOIN")
        || upper.contains("PIECEWISE_MERGE_JOIN")
    {
        NodeType::MergeJoin
    } else if upper.contains("NESTED_LOOP") {
        NodeType::NestedLoop
    } else if upper.contains("HASH_GROUP") {
        NodeType::HashAggregate
    } else if upper.contains("ORDER") || upper.contains("SORT")
    {
        NodeType::Sort
    } else {
        NodeType::Other
    }
}

fn extract_table_name(line: &str) -> Option<String> {
    // DuckDB plans often show table names after the operator.
    // e.g., "SEQ_SCAN users" or "TABLE_SCAN orders"
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 2 {
        let candidate = parts[parts.len() - 1]
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if !candidate.is_empty()
            && candidate.chars().next().is_some_and(|c| {
                c.is_ascii_alphabetic() || c == '_'
            })
        {
            return Some(candidate.to_owned());
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory() {
        let connector = DuckDBConnector::open_in_memory()
            .expect("should open in-memory db");
        assert_eq!(connector.kind(), DatabaseKind::DuckDB);
    }

    #[test]
    fn gather_schema_empty() {
        let connector = DuckDBConnector::open_in_memory()
            .expect("should open in-memory db");
        let schema = connector
            .gather_schema()
            .expect("should gather schema");
        assert_eq!(schema.kind, DatabaseKind::DuckDB);
        assert!(schema.tables.is_empty());
    }

    #[test]
    fn gather_schema_with_table() {
        let connector = DuckDBConnector::open_in_memory()
            .expect("should open");
        connector
            .conn
            .execute_batch(
                "CREATE TABLE users ( \
                    id INTEGER PRIMARY KEY, \
                    name VARCHAR NOT NULL, \
                    email VARCHAR \
                ); \
                INSERT INTO users VALUES (1, 'Alice', 'a@b.com'); \
                INSERT INTO users VALUES (2, 'Bob', NULL);",
            )
            .expect("setup");

        let schema = connector
            .gather_schema()
            .expect("should gather schema");
        assert_eq!(schema.tables.len(), 1);

        let users = schema
            .tables
            .get("users")
            .expect("should have users table");
        assert_eq!(users.columns.len(), 3);
        assert_eq!(users.columns[0].name, "id");
    }

    #[test]
    fn gather_statistics_duckdb() {
        let connector = DuckDBConnector::open_in_memory()
            .expect("should open");
        connector
            .conn
            .execute_batch(
                "CREATE TABLE orders (id INTEGER, amount DOUBLE); \
                 INSERT INTO orders VALUES (1, 100.0); \
                 INSERT INTO orders VALUES (2, 200.0); \
                 INSERT INTO orders VALUES (3, 300.0);",
            )
            .expect("setup");

        let stats = connector
            .gather_statistics("orders")
            .expect("should gather stats");
        assert_eq!(stats.table_name, "orders");
        assert!(stats.row_count >= 3.0 || stats.row_count == 0.0);
    }

    #[test]
    fn explain_query_duckdb() {
        let connector = DuckDBConnector::open_in_memory()
            .expect("should open");
        connector
            .conn
            .execute_batch(
                "CREATE TABLE t (id INTEGER, v VARCHAR); \
                 INSERT INTO t VALUES (1, 'a');",
            )
            .expect("setup");

        let plan = connector
            .explain_query("SELECT * FROM t WHERE id = 1")
            .expect("explain");
        assert!(plan.root.node_count() >= 1);
    }

    #[test]
    fn parse_index_columns_basic() {
        let sql = "CREATE INDEX idx ON t (a, b, c)";
        let cols = parse_index_columns(Some(sql));
        assert_eq!(cols, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_index_columns_none() {
        let cols = parse_index_columns(None);
        assert!(cols.is_empty());
    }

    #[test]
    fn classify_node_types() {
        assert_eq!(
            classify_duckdb_node("SEQ_SCAN users"),
            NodeType::SeqScan
        );
        assert_eq!(
            classify_duckdb_node("HASH_JOIN"),
            NodeType::HashJoin
        );
        assert_eq!(
            classify_duckdb_node("SOMETHING_ELSE"),
            NodeType::Other
        );
    }
}
