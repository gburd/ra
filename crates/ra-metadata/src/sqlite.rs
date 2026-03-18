//! SQLite metadata connector.
//!
//! Gathers schema, statistics, and EXPLAIN plans from SQLite
//! databases using PRAGMA commands and system tables.
//!
//! Like the other backend modules, this provides query/command
//! definitions and result parsers without depending on a SQLite
//! client library directly.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::connector::{
    ColumnInfo, ConstraintInfo, ConstraintType, GatheredColumnStats,
    GatheredIndexStats, GatheredTableStats, IndexInfo, SchemaInfo,
    TableInfo, ViewInfo,
};
use crate::error::MetadataError;
use crate::explain::{parse_sqlite_explain, ExplainPlan};

/// Catalog query/PRAGMA definitions for SQLite.
pub struct SqliteQueries;

impl SqliteQueries {
    /// SQL to list all user tables.
    #[must_use]
    pub fn list_tables_sql() -> &'static str {
        "SELECT name FROM sqlite_master \
         WHERE type = 'table' \
               AND name NOT LIKE 'sqlite_%' \
         ORDER BY name"
    }

    /// PRAGMA to get column info for a table.
    #[must_use]
    pub fn table_info_pragma(table: &str) -> String {
        format!("PRAGMA table_info('{table}')")
    }

    /// PRAGMA to list indexes for a table.
    #[must_use]
    pub fn index_list_pragma(table: &str) -> String {
        format!("PRAGMA index_list('{table}')")
    }

    /// PRAGMA to get columns for a specific index.
    #[must_use]
    pub fn index_info_pragma(index: &str) -> String {
        format!("PRAGMA index_info('{index}')")
    }

    /// SQL to get statistics from sqlite_stat1 if available.
    #[must_use]
    pub fn stat1_sql() -> &'static str {
        "SELECT tbl, idx, stat FROM sqlite_stat1 \
         WHERE tbl = ? \
         ORDER BY idx"
    }

    /// SQL to list foreign keys for a table.
    #[must_use]
    pub fn foreign_keys_pragma(table: &str) -> String {
        format!("PRAGMA foreign_key_list('{table}')")
    }

    /// SQL to list views.
    #[must_use]
    pub fn list_views_sql() -> &'static str {
        "SELECT name, sql FROM sqlite_master \
         WHERE type = 'view' \
         ORDER BY name"
    }

    /// SQL to count rows in a table (exact count).
    #[must_use]
    pub fn count_rows_sql(table: &str) -> String {
        format!("SELECT COUNT(*) AS cnt FROM \"{table}\"")
    }

    /// SQL to get database page count and size.
    #[must_use]
    pub fn page_count_sql() -> &'static str {
        "SELECT page_count * page_size AS total_size, \
                page_count \
         FROM pragma_page_count(), pragma_page_size()"
    }

    /// Build the EXPLAIN QUERY PLAN command.
    #[must_use]
    pub fn explain_sql(sql: &str) -> String {
        format!("EXPLAIN QUERY PLAN {sql}")
    }
}

/// Intermediate row from sqlite_master tables listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteTableRow {
    /// Table name.
    pub name: String,
}

/// Intermediate row from PRAGMA table_info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteColumnRow {
    /// Column ID (0-based).
    pub cid: u32,
    /// Column name.
    pub name: String,
    /// Column type.
    pub col_type: String,
    /// Whether NOT NULL is set (1 = not null).
    pub notnull: bool,
    /// Default value.
    pub dflt_value: Option<String>,
    /// Whether this column is part of the primary key.
    pub pk: bool,
}

/// Intermediate row from PRAGMA index_list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteIndexListRow {
    /// Index sequence number.
    pub seq: u32,
    /// Index name.
    pub name: String,
    /// Whether the index is unique.
    pub unique: bool,
    /// Origin: "c" (CREATE INDEX), "u" (UNIQUE), "pk" (PRIMARY KEY).
    pub origin: String,
}

/// Intermediate row from PRAGMA index_info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteIndexInfoRow {
    /// Column rank within the index.
    pub seqno: u32,
    /// Column index in the table (-1 for rowid).
    pub cid: i32,
    /// Column name.
    pub name: Option<String>,
}

/// Intermediate row from PRAGMA foreign_key_list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteForeignKeyRow {
    /// Foreign key ID.
    pub id: u32,
    /// Sequence number.
    pub seq: u32,
    /// Referenced table.
    pub table: String,
    /// Local column.
    pub from: String,
    /// Referenced column.
    pub to: String,
}

/// Intermediate row from sqlite_stat1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteStat1Row {
    /// Table name.
    pub tbl: String,
    /// Index name.
    pub idx: Option<String>,
    /// Statistics string: "N n1 n2 ..." where N is row count.
    pub stat: String,
}

/// Intermediate row for views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteViewRow {
    /// View name.
    pub name: String,
    /// View CREATE statement.
    pub sql: Option<String>,
}

// ── Result parsers ──────────────────────────────────────────

/// Build a [`SchemaInfo`] from SQLite catalog query results.
pub fn build_sqlite_schema(
    database: &str,
    table_rows: &[SqliteTableRow],
    columns_by_table: &HashMap<String, Vec<SqliteColumnRow>>,
    indexes_by_table: &HashMap<String, Vec<IndexInfo>>,
    fk_by_table: &HashMap<String, Vec<SqliteForeignKeyRow>>,
    view_rows: &[SqliteViewRow],
) -> SchemaInfo {
    let mut tables = Vec::new();
    for row in table_rows {
        let columns = columns_by_table
            .get(&row.name)
            .map(|cols| {
                cols.iter()
                    .map(sqlite_column_to_info)
                    .collect()
            })
            .unwrap_or_default();

        let indexes = indexes_by_table
            .get(&row.name)
            .cloned()
            .unwrap_or_default();

        let mut constraints = Vec::new();

        // Primary key constraint
        let pk_cols: Vec<String> = columns_by_table
            .get(&row.name)
            .map(|cols| {
                cols.iter()
                    .filter(|c| c.pk)
                    .map(|c| c.name.clone())
                    .collect()
            })
            .unwrap_or_default();

        if !pk_cols.is_empty() {
            constraints.push(ConstraintInfo {
                name: format!("{}_pkey", row.name),
                constraint_type: ConstraintType::PrimaryKey,
                columns: pk_cols,
                references_table: None,
                references_columns: None,
            });
        }

        // Foreign key constraints
        if let Some(fks) = fk_by_table.get(&row.name) {
            let mut fk_groups: HashMap<u32, Vec<&SqliteForeignKeyRow>> =
                HashMap::new();
            for fk in fks {
                fk_groups
                    .entry(fk.id)
                    .or_default()
                    .push(fk);
            }

            for (id, group) in &fk_groups {
                let ref_table = group
                    .first()
                    .map(|f| f.table.clone())
                    .unwrap_or_default();
                let from_cols: Vec<String> =
                    group.iter().map(|f| f.from.clone()).collect();
                let to_cols: Vec<String> =
                    group.iter().map(|f| f.to.clone()).collect();

                constraints.push(ConstraintInfo {
                    name: format!("{}_fk_{id}", row.name),
                    constraint_type: ConstraintType::ForeignKey,
                    columns: from_cols,
                    references_table: Some(ref_table),
                    references_columns: Some(to_cols),
                });
            }
        }

        tables.push(TableInfo {
            schema: "main".to_string(),
            name: row.name.clone(),
            columns,
            constraints,
            indexes,
            estimated_rows: None,
        });
    }

    let views = view_rows
        .iter()
        .map(|v| ViewInfo {
            schema: "main".to_string(),
            name: v.name.clone(),
            columns: Vec::new(),
            definition: v.sql.clone(),
        })
        .collect();

    SchemaInfo {
        database: database.to_string(),
        tables,
        views,
    }
}

/// Build index info from PRAGMA results.
pub fn build_sqlite_index(
    list_row: &SqliteIndexListRow,
    info_rows: &[SqliteIndexInfoRow],
) -> IndexInfo {
    let columns: Vec<String> = info_rows
        .iter()
        .filter_map(|r| r.name.clone())
        .collect();

    IndexInfo {
        name: list_row.name.clone(),
        columns,
        unique: list_row.unique,
        index_type: "btree".to_string(),
        primary: list_row.origin == "pk",
    }
}

/// Parse sqlite_stat1 rows into [`GatheredTableStats`].
pub fn build_sqlite_table_stats(
    table: &str,
    row_count: u64,
    stat1_rows: &[SqliteStat1Row],
) -> GatheredTableStats {
    let mut columns = HashMap::new();
    let mut indexes = HashMap::new();

    for row in stat1_rows {
        let parts: Vec<&str> = row.stat.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let table_rows = parts[0].parse::<u64>().unwrap_or(row_count);

        if let Some(idx_name) = &row.idx {
            // Parse per-column NDV estimates from stat1
            // Format: "N n1 n2 ..." where N is total rows,
            // n1 is avg rows per first column value, etc.
            for (i, part) in parts.iter().skip(1).enumerate() {
                if let Ok(avg_per_key) = part.parse::<u64>() {
                    if avg_per_key > 0 {
                        let ndv = table_rows / avg_per_key;
                        let col_key = format!(
                            "{idx_name}_col_{i}"
                        );
                        columns.insert(
                            col_key,
                            GatheredColumnStats {
                                distinct_count: ndv,
                                null_fraction: 0.0,
                                avg_width: 0.0,
                                correlation: None,
                                most_common_values: None,
                            },
                        );
                    }
                }
            }

            indexes.insert(
                idx_name.clone(),
                GatheredIndexStats {
                    size_bytes: 0,
                    scans: None,
                    tuples_read: None,
                    tuples_fetched: None,
                },
            );
        }
    }

    GatheredTableStats {
        table: table.to_string(),
        row_count,
        total_size_bytes: 0,
        columns,
        indexes,
    }
}

/// Parse SQLite EXPLAIN QUERY PLAN output.
pub fn parse_sq_explain(
    text: &str,
    query: &str,
) -> Result<ExplainPlan, MetadataError> {
    parse_sqlite_explain(text, query)
}

fn sqlite_column_to_info(row: &SqliteColumnRow) -> ColumnInfo {
    ColumnInfo {
        name: row.name.clone(),
        data_type: row.col_type.clone(),
        nullable: !row.notnull,
        default_value: row.dflt_value.clone(),
        ordinal_position: row.cid + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tables_sql_filters_system() {
        let sql = SqliteQueries::list_tables_sql();
        assert!(sql.contains("sqlite_master"));
        assert!(sql.contains("NOT LIKE 'sqlite_%'"));
    }

    #[test]
    fn table_info_pragma_format() {
        let pragma =
            SqliteQueries::table_info_pragma("users");
        assert_eq!(pragma, "PRAGMA table_info('users')");
    }

    #[test]
    fn explain_sql_format() {
        let sql =
            SqliteQueries::explain_sql("SELECT 1");
        assert_eq!(sql, "EXPLAIN QUERY PLAN SELECT 1");
    }

    #[test]
    fn build_schema_empty() {
        let schema = build_sqlite_schema(
            "test.db",
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &[],
        );
        assert_eq!(schema.database, "test.db");
        assert!(schema.tables.is_empty());
    }

    #[test]
    fn build_schema_with_table() {
        let table_rows = vec![SqliteTableRow {
            name: "users".to_string(),
        }];

        let mut columns = HashMap::new();
        columns.insert(
            "users".to_string(),
            vec![
                SqliteColumnRow {
                    cid: 0,
                    name: "id".to_string(),
                    col_type: "INTEGER".to_string(),
                    notnull: true,
                    dflt_value: None,
                    pk: true,
                },
                SqliteColumnRow {
                    cid: 1,
                    name: "name".to_string(),
                    col_type: "TEXT".to_string(),
                    notnull: false,
                    dflt_value: None,
                    pk: false,
                },
            ],
        );

        let schema = build_sqlite_schema(
            "test.db",
            &table_rows,
            &columns,
            &HashMap::new(),
            &HashMap::new(),
            &[],
        );

        assert_eq!(schema.tables.len(), 1);
        let table = &schema.tables[0];
        assert_eq!(table.name, "users");
        assert_eq!(table.schema, "main");
        assert_eq!(table.columns.len(), 2);
        assert!(!table.columns[0].nullable);
        assert!(table.columns[1].nullable);
        assert_eq!(table.constraints.len(), 1);
        assert_eq!(
            table.constraints[0].constraint_type,
            ConstraintType::PrimaryKey
        );
    }

    #[test]
    fn build_schema_with_foreign_keys() {
        let table_rows = vec![SqliteTableRow {
            name: "orders".to_string(),
        }];

        let mut fks = HashMap::new();
        fks.insert(
            "orders".to_string(),
            vec![SqliteForeignKeyRow {
                id: 0,
                seq: 0,
                table: "users".to_string(),
                from: "user_id".to_string(),
                to: "id".to_string(),
            }],
        );

        let schema = build_sqlite_schema(
            "test.db",
            &table_rows,
            &HashMap::new(),
            &HashMap::new(),
            &fks,
            &[],
        );

        let constraints = &schema.tables[0].constraints;
        assert_eq!(constraints.len(), 1);
        assert_eq!(
            constraints[0].constraint_type,
            ConstraintType::ForeignKey
        );
        assert_eq!(
            constraints[0].references_table.as_deref(),
            Some("users")
        );
    }

    #[test]
    fn build_index_from_pragma() {
        let list_row = SqliteIndexListRow {
            seq: 0,
            name: "idx_email".to_string(),
            unique: true,
            origin: "c".to_string(),
        };
        let info_rows = vec![SqliteIndexInfoRow {
            seqno: 0,
            cid: 2,
            name: Some("email".to_string()),
        }];

        let index = build_sqlite_index(&list_row, &info_rows);
        assert_eq!(index.name, "idx_email");
        assert!(index.unique);
        assert!(!index.primary);
        assert_eq!(index.columns, vec!["email"]);
    }

    #[test]
    fn build_table_stats_from_stat1() {
        let stat1 = vec![SqliteStat1Row {
            tbl: "orders".to_string(),
            idx: Some("idx_customer".to_string()),
            stat: "10000 100".to_string(),
        }];

        let result = build_sqlite_table_stats(
            "orders",
            10_000,
            &stat1,
        );

        assert_eq!(result.row_count, 10_000);
        assert!(result.indexes.contains_key("idx_customer"));
        assert_eq!(result.columns.len(), 1);
    }

    #[test]
    fn stat1_empty_stats() {
        let result = build_sqlite_table_stats("empty", 0, &[]);
        assert_eq!(result.row_count, 0);
        assert!(result.columns.is_empty());
        assert!(result.indexes.is_empty());
    }

    #[test]
    fn view_rows_parsed() {
        let view_rows = vec![SqliteViewRow {
            name: "active_users".to_string(),
            sql: Some(
                "CREATE VIEW active_users AS \
                 SELECT * FROM users WHERE active = 1"
                    .to_string(),
            ),
        }];

        let schema = build_sqlite_schema(
            "test.db",
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &view_rows,
        );

        assert_eq!(schema.views.len(), 1);
        assert_eq!(schema.views[0].name, "active_users");
        assert!(schema.views[0].definition.is_some());
    }
}
