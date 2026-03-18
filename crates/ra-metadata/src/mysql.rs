//! MySQL metadata connector.
//!
//! Gathers schema, statistics, and EXPLAIN plans from MySQL/MariaDB
//! by querying `information_schema` tables.
//!
//! Like the PostgreSQL module, this provides query definitions and
//! result parsers without directly depending on a MySQL client
//! library.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::connector::{
    ColumnInfo, ConstraintInfo, ConstraintType, GatheredColumnStats,
    GatheredIndexStats, GatheredTableStats, IndexInfo, SchemaInfo,
    TableInfo, ViewInfo,
};
use crate::error::MetadataError;
use crate::explain::{parse_mysql_explain, ExplainPlan};

/// Catalog query definitions for MySQL.
pub struct MySqlQueries;

impl MySqlQueries {
    /// SQL to list all user tables.
    #[must_use]
    pub fn list_tables_sql() -> &'static str {
        "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_ROWS \
         FROM information_schema.TABLES \
         WHERE TABLE_TYPE = 'BASE TABLE' \
               AND TABLE_SCHEMA NOT IN \
               ('information_schema', 'performance_schema', \
                'mysql', 'sys') \
         ORDER BY TABLE_SCHEMA, TABLE_NAME"
    }

    /// SQL to get columns for a specific table.
    #[must_use]
    pub fn columns_sql() -> &'static str {
        "SELECT COLUMN_NAME, DATA_TYPE, \
                IS_NULLABLE, COLUMN_DEFAULT, \
                ORDINAL_POSITION \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
         ORDER BY ORDINAL_POSITION"
    }

    /// SQL to get constraints for a specific table.
    #[must_use]
    pub fn constraints_sql() -> &'static str {
        "SELECT tc.CONSTRAINT_NAME, \
                tc.CONSTRAINT_TYPE, \
                GROUP_CONCAT(kcu.COLUMN_NAME \
                    ORDER BY kcu.ORDINAL_POSITION) AS columns, \
                kcu.REFERENCED_TABLE_NAME, \
                GROUP_CONCAT(kcu.REFERENCED_COLUMN_NAME \
                    ORDER BY kcu.ORDINAL_POSITION) \
                    AS referenced_columns \
         FROM information_schema.TABLE_CONSTRAINTS tc \
         JOIN information_schema.KEY_COLUMN_USAGE kcu \
              ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
              AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA \
              AND tc.TABLE_NAME = kcu.TABLE_NAME \
         WHERE tc.TABLE_SCHEMA = ? AND tc.TABLE_NAME = ? \
         GROUP BY tc.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE, \
                  kcu.REFERENCED_TABLE_NAME \
         ORDER BY tc.CONSTRAINT_NAME"
    }

    /// SQL to get indexes for a specific table.
    #[must_use]
    pub fn indexes_sql() -> &'static str {
        "SELECT INDEX_NAME, \
                GROUP_CONCAT(COLUMN_NAME \
                    ORDER BY SEQ_IN_INDEX) AS columns, \
                NOT NON_UNIQUE AS is_unique, \
                INDEX_TYPE \
         FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
         GROUP BY INDEX_NAME, NON_UNIQUE, INDEX_TYPE \
         ORDER BY INDEX_NAME"
    }

    /// SQL to get column statistics from information_schema.
    #[must_use]
    pub fn column_stats_sql() -> &'static str {
        "SELECT COLUMN_NAME, CARDINALITY, NULLABLE \
         FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
               AND SEQ_IN_INDEX = 1 \
         ORDER BY COLUMN_NAME"
    }

    /// SQL to get table size.
    #[must_use]
    pub fn table_size_sql() -> &'static str {
        "SELECT TABLE_ROWS AS row_count, \
                DATA_LENGTH + INDEX_LENGTH AS total_size \
         FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?"
    }

    /// SQL to get index sizes.
    #[must_use]
    pub fn index_stats_sql() -> &'static str {
        "SELECT INDEX_NAME, \
                STAT_VALUE * @@innodb_page_size AS size_bytes \
         FROM mysql.innodb_index_stats \
         WHERE database_name = ? AND table_name = ? \
               AND stat_name = 'size' \
         ORDER BY INDEX_NAME"
    }

    /// SQL to list views.
    #[must_use]
    pub fn list_views_sql() -> &'static str {
        "SELECT TABLE_SCHEMA, TABLE_NAME, VIEW_DEFINITION \
         FROM information_schema.VIEWS \
         WHERE TABLE_SCHEMA NOT IN \
               ('information_schema', 'performance_schema', \
                'mysql', 'sys') \
         ORDER BY TABLE_SCHEMA, TABLE_NAME"
    }

    /// Build the EXPLAIN command for a query.
    #[must_use]
    pub fn explain_sql(sql: &str) -> String {
        format!("EXPLAIN FORMAT=JSON {sql}")
    }
}

/// Intermediate row from the tables listing query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlTableRow {
    /// Schema (database) name.
    pub table_schema: String,
    /// Table name.
    pub table_name: String,
    /// Estimated row count.
    pub table_rows: Option<u64>,
}

/// Intermediate row from the columns query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlColumnRow {
    /// Column name.
    pub column_name: String,
    /// Data type.
    pub data_type: String,
    /// "YES" or "NO".
    pub is_nullable: String,
    /// Default value.
    pub column_default: Option<String>,
    /// Ordinal position.
    pub ordinal_position: u32,
}

/// Intermediate row from the constraints query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlConstraintRow {
    /// Constraint name.
    pub constraint_name: String,
    /// Constraint type: PRIMARY KEY, UNIQUE, FOREIGN KEY.
    pub constraint_type: String,
    /// Comma-separated column names.
    pub columns: String,
    /// Referenced table (for foreign keys).
    pub referenced_table_name: Option<String>,
    /// Comma-separated referenced columns.
    pub referenced_columns: Option<String>,
}

/// Intermediate row from the indexes query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlIndexRow {
    /// Index name.
    pub index_name: String,
    /// Comma-separated column names.
    pub columns: String,
    /// Whether the index is unique (1=unique, 0=not).
    pub is_unique: bool,
    /// Index type (BTREE, HASH, FULLTEXT, SPATIAL).
    pub index_type: String,
}

/// Intermediate row for column cardinality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlCardinalityRow {
    /// Column name.
    pub column_name: String,
    /// Estimated cardinality (NDV).
    pub cardinality: Option<u64>,
    /// Whether nullable.
    pub nullable: String,
}

/// Intermediate row for table size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlTableSizeRow {
    /// Estimated row count.
    pub row_count: u64,
    /// Total size in bytes.
    pub total_size: u64,
}

/// Intermediate row for index size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlIndexSizeRow {
    /// Index name.
    pub index_name: String,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// Intermediate row for views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlViewRow {
    /// Schema name.
    pub table_schema: String,
    /// View name.
    pub table_name: String,
    /// View definition.
    pub view_definition: Option<String>,
}

// ── Result parsers ──────────────────────────────────────────

/// Build a [`SchemaInfo`] from MySQL catalog query results.
pub fn build_mysql_schema(
    database: &str,
    table_rows: &[MySqlTableRow],
    columns_by_table: &HashMap<String, Vec<MySqlColumnRow>>,
    constraints_by_table: &HashMap<
        String,
        Vec<MySqlConstraintRow>,
    >,
    indexes_by_table: &HashMap<String, Vec<MySqlIndexRow>>,
    view_rows: &[MySqlViewRow],
    view_columns: &HashMap<String, Vec<MySqlColumnRow>>,
) -> SchemaInfo {
    let mut tables = Vec::new();
    for row in table_rows {
        let key = format!(
            "{}.{}",
            row.table_schema, row.table_name
        );

        let columns = columns_by_table
            .get(&key)
            .map(|cols| {
                cols.iter()
                    .map(mysql_column_to_info)
                    .collect()
            })
            .unwrap_or_default();

        let constraints = constraints_by_table
            .get(&key)
            .map(|cons| {
                cons.iter()
                    .map(mysql_constraint_to_info)
                    .collect()
            })
            .unwrap_or_default();

        let indexes = indexes_by_table
            .get(&key)
            .map(|idxs| {
                idxs.iter().map(mysql_index_to_info).collect()
            })
            .unwrap_or_default();

        tables.push(TableInfo {
            schema: row.table_schema.clone(),
            name: row.table_name.clone(),
            columns,
            constraints,
            indexes,
            estimated_rows: row.table_rows,
        });
    }

    let mut views = Vec::new();
    for row in view_rows {
        let key = format!(
            "{}.{}",
            row.table_schema, row.table_name
        );
        let columns = view_columns
            .get(&key)
            .map(|cols| {
                cols.iter()
                    .map(mysql_column_to_info)
                    .collect()
            })
            .unwrap_or_default();

        views.push(ViewInfo {
            schema: row.table_schema.clone(),
            name: row.table_name.clone(),
            columns,
            definition: row.view_definition.clone(),
        });
    }

    SchemaInfo {
        database: database.to_string(),
        tables,
        views,
    }
}

/// Build [`GatheredTableStats`] from MySQL catalog data.
pub fn build_mysql_table_stats(
    table: &str,
    size_row: &MySqlTableSizeRow,
    cardinality_rows: &[MySqlCardinalityRow],
    index_size_rows: &[MySqlIndexSizeRow],
) -> GatheredTableStats {
    let mut columns = HashMap::new();
    for row in cardinality_rows {
        let null_fraction = if row.nullable == "YES" {
            0.01
        } else {
            0.0
        };

        columns.insert(
            row.column_name.clone(),
            GatheredColumnStats {
                distinct_count: row.cardinality.unwrap_or(0),
                null_fraction,
                avg_width: 0.0,
                correlation: None,
                most_common_values: None,
            },
        );
    }

    let mut indexes = HashMap::new();
    for row in index_size_rows {
        indexes.insert(
            row.index_name.clone(),
            GatheredIndexStats {
                size_bytes: row.size_bytes,
                scans: None,
                tuples_read: None,
                tuples_fetched: None,
            },
        );
    }

    GatheredTableStats {
        table: table.to_string(),
        row_count: size_row.row_count,
        total_size_bytes: size_row.total_size,
        columns,
        indexes,
    }
}

/// Parse MySQL EXPLAIN JSON output into an [`ExplainPlan`].
pub fn parse_my_explain(
    json_text: &str,
    query: &str,
) -> Result<ExplainPlan, MetadataError> {
    parse_mysql_explain(json_text, query)
}

fn mysql_column_to_info(row: &MySqlColumnRow) -> ColumnInfo {
    ColumnInfo {
        name: row.column_name.clone(),
        data_type: row.data_type.clone(),
        nullable: row.is_nullable == "YES",
        default_value: row.column_default.clone(),
        ordinal_position: row.ordinal_position,
    }
}

fn mysql_constraint_to_info(
    row: &MySqlConstraintRow,
) -> ConstraintInfo {
    let constraint_type = match row.constraint_type.as_str() {
        "PRIMARY KEY" => ConstraintType::PrimaryKey,
        "UNIQUE" => ConstraintType::Unique,
        "FOREIGN KEY" => ConstraintType::ForeignKey,
        _ => ConstraintType::Check,
    };

    let columns: Vec<String> = row
        .columns
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let references_columns = row
        .referenced_columns
        .as_ref()
        .map(|rc| {
            rc.split(',')
                .map(|s| s.trim().to_string())
                .collect()
        });

    ConstraintInfo {
        name: row.constraint_name.clone(),
        constraint_type,
        columns,
        references_table: row.referenced_table_name.clone(),
        references_columns,
    }
}

fn mysql_index_to_info(row: &MySqlIndexRow) -> IndexInfo {
    let columns: Vec<String> = row
        .columns
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    IndexInfo {
        name: row.index_name.clone(),
        columns,
        unique: row.is_unique,
        index_type: row.index_type.clone(),
        primary: row.index_name == "PRIMARY",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tables_sql_is_valid() {
        let sql = MySqlQueries::list_tables_sql();
        assert!(sql.contains("information_schema.TABLES"));
        assert!(sql.contains("TABLE_SCHEMA"));
    }

    #[test]
    fn explain_sql_wraps_query() {
        let sql = MySqlQueries::explain_sql(
            "SELECT * FROM users",
        );
        assert_eq!(
            sql,
            "EXPLAIN FORMAT=JSON SELECT * FROM users"
        );
    }

    #[test]
    fn build_schema_empty() {
        let schema = build_mysql_schema(
            "testdb",
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &HashMap::new(),
        );
        assert_eq!(schema.database, "testdb");
        assert!(schema.tables.is_empty());
    }

    #[test]
    fn build_schema_with_table() {
        let table_rows = vec![MySqlTableRow {
            table_schema: "mydb".to_string(),
            table_name: "orders".to_string(),
            table_rows: Some(5000),
        }];

        let mut columns = HashMap::new();
        columns.insert(
            "mydb.orders".to_string(),
            vec![MySqlColumnRow {
                column_name: "id".to_string(),
                data_type: "int".to_string(),
                is_nullable: "NO".to_string(),
                column_default: None,
                ordinal_position: 1,
            }],
        );

        let schema = build_mysql_schema(
            "mydb",
            &table_rows,
            &columns,
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &HashMap::new(),
        );

        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "orders");
        assert_eq!(schema.tables[0].columns.len(), 1);
        assert!(!schema.tables[0].columns[0].nullable);
    }

    #[test]
    fn build_table_stats_basic() {
        let size = MySqlTableSizeRow {
            row_count: 3000,
            total_size: 200_000,
        };
        let cardinality = vec![MySqlCardinalityRow {
            column_name: "email".to_string(),
            cardinality: Some(2800),
            nullable: "NO".to_string(),
        }];

        let result = build_mysql_table_stats(
            "users",
            &size,
            &cardinality,
            &[],
        );

        assert_eq!(result.row_count, 3000);
        let col = result
            .columns
            .get("email")
            .expect("email column");
        assert_eq!(col.distinct_count, 2800);
        assert_eq!(col.null_fraction, 0.0);
    }

    #[test]
    fn constraint_type_mapping() {
        let pk = MySqlConstraintRow {
            constraint_name: "PRIMARY".to_string(),
            constraint_type: "PRIMARY KEY".to_string(),
            columns: "id".to_string(),
            referenced_table_name: None,
            referenced_columns: None,
        };
        let info = mysql_constraint_to_info(&pk);
        assert_eq!(
            info.constraint_type,
            ConstraintType::PrimaryKey
        );
    }

    #[test]
    fn index_primary_detection() {
        let idx = MySqlIndexRow {
            index_name: "PRIMARY".to_string(),
            columns: "id".to_string(),
            is_unique: true,
            index_type: "BTREE".to_string(),
        };
        let info = mysql_index_to_info(&idx);
        assert!(info.primary);
        assert!(info.unique);
    }

    #[test]
    fn columns_split_correctly() {
        let row = MySqlConstraintRow {
            constraint_name: "idx_composite".to_string(),
            constraint_type: "UNIQUE".to_string(),
            columns: "first_name,last_name".to_string(),
            referenced_table_name: None,
            referenced_columns: None,
        };
        let info = mysql_constraint_to_info(&row);
        assert_eq!(
            info.columns,
            vec!["first_name", "last_name"]
        );
    }
}
