//! `SQLite` database connector.
//!
//! Uses PRAGMA commands (`table_info`, `index_list`, `index_info`)
//! and `sqlite_stat1` for statistics. Parses `EXPLAIN QUERY PLAN`
//! output.

use std::collections::HashMap;
use std::fmt::Write;

use rusqlite::Connection;

use crate::connector::{DatabaseConnector, MetadataResult};
use crate::error::MetadataError;
use crate::explain::ExplainPlan;
use crate::schema::{
    ColumnInfo, ColumnStatistics, ConstraintInfo, ConstraintKind,
    DatabaseKind, IndexInfo, SchemaInfo, TableInfo, TableStats,
    TriggerEvent, TriggerInfo, TriggerScope, TriggerTiming,
};

/// `SQLite` connector using the `rusqlite` crate.
pub struct SqliteConnector {
    conn: Connection,
    db_path: String,
}

impl SqliteConnector {
    /// Open a connection to a `SQLite` database file.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` if the file cannot be
    /// opened.
    pub fn connect(path: &str) -> MetadataResult<Self> {
        let conn = Connection::open(path).map_err(|e| {
            MetadataError::Connection {
                message: format!(
                    "SQLite open failed for {path}: {e}"
                ),
            }
        })?;

        Ok(Self {
            conn,
            db_path: path.to_owned(),
        })
    }

    /// Open an in-memory `SQLite` database.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` if initialization fails.
    pub fn open_in_memory() -> MetadataResult<Self> {
        let conn = Connection::open_in_memory().map_err(|e| {
            MetadataError::Connection {
                message: format!(
                    "SQLite in-memory open failed: {e}"
                ),
            }
        })?;

        Ok(Self {
            conn,
            db_path: ":memory:".to_owned(),
        })
    }

    /// Get the underlying rusqlite connection (for testing).
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    fn query_tables(&self) -> MetadataResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
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
            .prepare(&format!(
                "PRAGMA table_info('{}')",
                table.replace('\'', "''")
            ))
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "PRAGMA table_info failed for {table}: {e}"
                ),
            })?;

        let columns: Vec<ColumnInfo> = stmt
            .query_map([], |row| {
                let cid: u32 = row.get(0)?;
                let name: String = row.get(1)?;
                let data_type: String = row.get(2)?;
                let notnull: bool = row.get(3)?;
                let default_value: Option<String> = row.get(4)?;
                Ok(ColumnInfo {
                    name,
                    data_type,
                    nullable: !notnull,
                    ordinal: cid + 1,
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

    fn query_indexes(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<IndexInfo>> {
        let mut idx_stmt = self
            .conn
            .prepare(&format!(
                "PRAGMA index_list('{}')",
                table.replace('\'', "''")
            ))
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "PRAGMA index_list failed for {table}: {e}"
                ),
            })?;

        let index_list: Vec<(String, bool)> = idx_stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let unique: bool = row.get(2)?;
                Ok((name, unique))
            })
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read index list for {table}: {e}"
                ),
            })?
            .filter_map(Result::ok)
            .collect();

        let mut indexes = Vec::new();
        for (idx_name, unique) in &index_list {
            let mut col_stmt = self
                .conn
                .prepare(&format!(
                    "PRAGMA index_info('{}')",
                    idx_name.replace('\'', "''")
                ))
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "PRAGMA index_info failed for \
                         {idx_name}: {e}"
                    ),
                })?;

            let columns: Vec<String> = col_stmt
                .query_map([], |row| {
                    let name: String = row.get(2)?;
                    Ok(name)
                })
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read index columns for \
                         {idx_name}: {e}"
                    ),
                })?
                .filter_map(Result::ok)
                .collect();

            indexes.push(IndexInfo {
                name: idx_name.clone(),
                columns,
                unique: *unique,
                index_type: "btree".to_owned(),
            });
        }

        Ok(indexes)
    }

    fn query_pk_constraint(
        &self,
        table: &str,
    ) -> Vec<ConstraintInfo> {
        let Ok(mut stmt) = self.conn.prepare(&format!(
            "PRAGMA table_info('{}')",
            table.replace('\'', "''")
        )) else {
            return vec![];
        };

        let Ok(rows) = stmt.query_map([], |row| {
            let pk: u32 = row.get(5)?;
            let name: String = row.get(1)?;
            Ok((pk, name))
        }) else {
            return vec![];
        };

        let pk_cols: Vec<String> = rows
            .filter_map(Result::ok)
            .filter(|(pk, _)| *pk > 0)
            .map(|(_, name)| name)
            .collect();

        if pk_cols.is_empty() {
            return vec![];
        }

        vec![ConstraintInfo {
            name: format!("{table}_pk"),
            kind: ConstraintKind::PrimaryKey,
            columns: pk_cols,
            referenced_table: None,
            referenced_columns: vec![],
            check_expression: None,
        }]
    }

    fn query_fk_constraints(
        &self,
        table: &str,
    ) -> Vec<ConstraintInfo> {
        let Ok(mut stmt) = self.conn.prepare(&format!(
            "PRAGMA foreign_key_list('{}')",
            table.replace('\'', "''")
        )) else {
            return vec![];
        };

        let Ok(fk_rows) = stmt.query_map([], |row| {
            let ref_table: String = row.get(2)?;
            let from: String = row.get(3)?;
            let to: String = row.get(4)?;
            Ok((ref_table, from, to))
        }) else {
            return vec![];
        };

        let fks: Vec<(String, String, String)> = fk_rows
            .filter_map(Result::ok)
            .collect();

        if fks.is_empty() {
            return vec![];
        }

        let mut fk_map: HashMap<String, (Vec<String>, Vec<String>)> =
            HashMap::new();
        for (ref_table, from, to) in fks {
            let entry = fk_map
                .entry(ref_table)
                .or_insert_with(|| (Vec::new(), Vec::new()));
            entry.0.push(from);
            entry.1.push(to);
        }

        let mut constraints = Vec::new();
        for (ref_table, (from_cols, to_cols)) in fk_map {
            constraints.push(ConstraintInfo {
                name: format!("fk_{table}_{ref_table}"),
                kind: ConstraintKind::ForeignKey,
                columns: from_cols,
                referenced_table: Some(ref_table),
                referenced_columns: to_cols,
                check_expression: None,
            });
        }

        constraints
    }

    fn query_row_count(
        &self,
        table: &str,
    ) -> MetadataResult<f64> {
        let stat1_result: Result<f64, _> = self.conn.query_row(
            "SELECT stat FROM sqlite_stat1 \
             WHERE tbl = ?1 AND idx IS NULL \
             LIMIT 1",
            [table],
            |row| {
                let stat: String = row.get(0)?;
                let count = stat
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                Ok(count)
            },
        );

        if let Ok(count) = stat1_result {
            return Ok(count);
        }

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

        Ok(count)
    }

    fn query_triggers(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<TriggerInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, sql FROM sqlite_master \
                 WHERE type = 'trigger' \
                 AND tbl_name = ?1",
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query triggers for {table}: {e}"
                ),
            })?;

        let triggers: Vec<(String, String)> = stmt
            .query_map([table], |row| {
                let name: String = row.get(0)?;
                let sql: String = row.get(1)?;
                Ok((name, sql))
            })
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read triggers for {table}: {e}"
                ),
            })?
            .filter_map(Result::ok)
            .collect();

        let mut result = Vec::new();
        for (name, sql) in triggers {
            let upper = sql.to_uppercase();
            let event = if upper.contains("INSERT") {
                TriggerEvent::Insert
            } else if upper.contains("DELETE") {
                TriggerEvent::Delete
            } else if upper.contains("UPDATE") {
                TriggerEvent::Update
            } else {
                continue;
            };

            let timing = if upper.contains("BEFORE") {
                TriggerTiming::Before
            } else if upper.contains("INSTEAD OF") {
                TriggerTiming::InsteadOf
            } else {
                TriggerTiming::After
            };

            // SQLite triggers are always FOR EACH ROW
            let scope = TriggerScope::Row;

            result.push(TriggerInfo {
                name,
                event,
                timing,
                scope,
                action_sql: sql,
                table_name: table.to_owned(),
                enabled: true,
            });
        }

        Ok(result)
    }

    fn build_explain_text(
        &self,
        sql: &str,
    ) -> MetadataResult<String> {
        let explain_sql =
            format!("EXPLAIN QUERY PLAN {sql}");
        let mut stmt = self
            .conn
            .prepare(&explain_sql)
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "EXPLAIN QUERY PLAN failed: {e}"
                ),
            })?;

        let rows: Vec<(i64, i64, i64, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read EXPLAIN output: {e}"
                ),
            })?
            .filter_map(Result::ok)
            .collect();

        // Build pipe-delimited format for the parser
        let mut text = String::new();
        for (id, parent, notused, detail) in &rows {
            let _ = writeln!(
                text, "{id}|{parent}|{notused}|{detail}"
            );
        }

        Ok(text)
    }
}

impl DatabaseConnector for SqliteConnector {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::SQLite
    }

    fn gather_schema(&self) -> MetadataResult<SchemaInfo> {
        let table_names = self.query_tables()?;
        let mut tables = HashMap::new();

        for name in &table_names {
            let columns = self.query_columns(name)?;
            let indexes = self.query_indexes(name)?;
            let mut constraints =
                self.query_pk_constraint(name);
            constraints
                .extend(self.query_fk_constraints(name));
            let triggers = self.query_triggers(name)?;
            let row_count = self.query_row_count(name)?;

            tables.insert(
                name.clone(),
                TableInfo {
                    name: name.clone(),
                    columns,
                    constraints,
                    indexes,
                    triggers,
                    estimated_rows: Some(row_count),
                },
            );
        }

        Ok(SchemaInfo {
            kind: DatabaseKind::SQLite,
            schema_name: self.db_path.clone(),
            tables,
        })
    }

    fn gather_statistics(
        &self,
        table: &str,
    ) -> MetadataResult<TableStats> {
        let row_count = self.query_row_count(table)?;

        let columns_info = self.query_columns(table)?;
        let mut columns = HashMap::new();
        for col in &columns_info {
            columns.insert(
                col.name.clone(),
                ColumnStatistics {
                    column_name: col.name.clone(),
                    distinct_count: 0.0,
                    null_fraction: 0.0,
                    avg_width: None,
                    most_common_values: vec![],
                    histogram_bounds: vec![],
                },
            );
        }

        let total_bytes = self
            .conn
            .query_row(
                "SELECT page_count * page_size \
                 FROM pragma_page_count, pragma_page_size",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;

        Ok(TableStats {
            table_name: table.to_owned(),
            row_count,
            total_bytes,
            columns,
        })
    }

    fn explain_query(
        &self,
        sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        let text = self.build_explain_text(sql)?;
        crate::explain::parse_sqlite_explain(&text)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn setup_test_db() -> SqliteConnector {
        let connector = SqliteConnector::open_in_memory()
            .expect("should open in-memory db");

        connector
            .conn
            .execute_batch(
                "CREATE TABLE users (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    email TEXT UNIQUE,
                    age INTEGER
                );
                CREATE TABLE orders (
                    id INTEGER PRIMARY KEY,
                    user_id INTEGER NOT NULL,
                    amount REAL,
                    FOREIGN KEY (user_id) REFERENCES users(id)
                );
                CREATE INDEX idx_orders_user_id \
                    ON orders(user_id);
                INSERT INTO users VALUES (1, 'Alice', 'a@b.com', 30);
                INSERT INTO users VALUES (2, 'Bob', 'b@b.com', 25);
                INSERT INTO users VALUES (3, 'Carol', 'c@c.com', 35);
                INSERT INTO orders VALUES (1, 1, 100.0);
                INSERT INTO orders VALUES (2, 1, 200.0);
                INSERT INTO orders VALUES (3, 2, 150.0);
                ANALYZE;",
            )
            .expect("should set up test data");

        connector
    }

    #[test]
    fn gather_schema_sqlite() {
        let connector = setup_test_db();
        let schema = connector
            .gather_schema()
            .expect("should gather schema");

        assert_eq!(schema.kind, DatabaseKind::SQLite);
        assert_eq!(schema.tables.len(), 2);

        let users = schema
            .tables
            .get("users")
            .expect("should have users table");
        assert_eq!(users.columns.len(), 4);
        assert_eq!(users.columns[0].name, "id");
        // SQLite INTEGER PRIMARY KEY reports notnull=0 because
        // it's an alias for rowid; verify name column instead.
        let name_col = users
            .columns
            .iter()
            .find(|c| c.name == "name")
            .expect("should have name column");
        assert!(!name_col.nullable);
    }

    #[test]
    fn gather_schema_indexes() {
        let connector = setup_test_db();
        let schema = connector
            .gather_schema()
            .expect("should gather schema");

        let orders = schema
            .tables
            .get("orders")
            .expect("should have orders table");

        let idx = orders
            .indexes
            .iter()
            .find(|i| i.name == "idx_orders_user_id");
        assert!(idx.is_some());
        let idx = idx.expect("index should exist");
        assert_eq!(idx.columns, vec!["user_id"]);
        assert!(!idx.unique);
    }

    #[test]
    fn gather_schema_constraints() {
        let connector = setup_test_db();
        let schema = connector
            .gather_schema()
            .expect("should gather schema");

        let users = schema
            .tables
            .get("users")
            .expect("should have users table");
        let pk = users
            .constraints
            .iter()
            .find(|c| c.kind == ConstraintKind::PrimaryKey);
        assert!(pk.is_some());

        let orders = schema
            .tables
            .get("orders")
            .expect("should have orders table");
        let fk = orders
            .constraints
            .iter()
            .find(|c| c.kind == ConstraintKind::ForeignKey);
        assert!(fk.is_some());
        let fk = fk.expect("fk should exist");
        assert_eq!(
            fk.referenced_table.as_deref(),
            Some("users")
        );
    }

    #[test]
    fn gather_statistics_sqlite() {
        let connector = setup_test_db();
        let stats = connector
            .gather_statistics("users")
            .expect("should gather stats");

        assert_eq!(stats.table_name, "users");
        assert!(stats.row_count >= 3.0);
        assert!(stats.total_bytes > 0);
    }

    #[test]
    fn explain_query_sqlite() {
        let connector = setup_test_db();
        let plan = connector
            .explain_query("SELECT * FROM users WHERE id = 1")
            .expect("should explain query");

        // The plan should reference users somewhere in the tree
        fn has_table(node: &crate::explain::ExplainNode, name: &str) -> bool {
            if node.relation.as_deref() == Some(name) {
                return true;
            }
            if let Some(ref detail) = node.raw_detail {
                if detail.contains(name) {
                    return true;
                }
            }
            node.children.iter().any(|c| has_table(c, name))
        }
        assert!(
            has_table(&plan.root, "users"),
            "plan should reference users table"
        );
    }

    #[test]
    fn explain_join_sqlite() {
        let connector = setup_test_db();
        let plan = connector
            .explain_query(
                "SELECT u.name, o.amount \
                 FROM users u \
                 JOIN orders o ON u.id = o.user_id",
            )
            .expect("should explain join query");

        // Should have at least the root node
        assert!(plan.root.node_count() >= 1);
    }

    #[test]
    fn connector_kind_sqlite() {
        let connector = setup_test_db();
        assert_eq!(connector.kind(), DatabaseKind::SQLite);
    }

    #[test]
    fn stats_to_core() {
        let connector = setup_test_db();
        let stats = connector
            .gather_statistics("users")
            .expect("should gather stats");
        let core = stats.to_core_statistics();
        assert!(core.row_count >= 3.0);
    }
}
