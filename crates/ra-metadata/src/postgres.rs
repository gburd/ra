//! PostgreSQL metadata connector.
//!
//! Gathers schema, statistics, and EXPLAIN plans from PostgreSQL
//! by querying system catalogs (`pg_class`, `pg_attribute`,
//! `pg_constraint`, `pg_stats`, `pg_indexes`).
//!
//! This module defines the queries and result parsing but does not
//! directly depend on a PostgreSQL client library. Instead, it
//! provides [`PostgresQueries`] with SQL strings and result parsers
//! that can be used with any PostgreSQL driver.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::connector::{
    ColumnInfo, ConstraintInfo, ConstraintType, GatheredColumnStats,
    GatheredIndexStats, GatheredTableStats, IndexInfo, SchemaInfo,
    TableInfo, ViewInfo,
};
use crate::error::MetadataError;
use crate::explain::{parse_postgres_explain, ExplainPlan};

/// Catalog query definitions for PostgreSQL.
///
/// Each method returns the SQL query string needed to gather
/// a particular piece of metadata. The caller is responsible for
/// executing the query and passing the results to the corresponding
/// `parse_*` method.
pub struct PostgresQueries;

impl PostgresQueries {
    /// SQL to list all user tables in the current database.
    #[must_use]
    pub fn list_tables_sql() -> &'static str {
        "SELECT schemaname, tablename, n_live_tup \
         FROM pg_stat_user_tables \
         ORDER BY schemaname, tablename"
    }

    /// SQL to get columns for a specific table.
    #[must_use]
    pub fn columns_sql() -> &'static str {
        "SELECT a.attname AS column_name, \
                pg_catalog.format_type(a.atttypid, a.atttypmod) \
                    AS data_type, \
                NOT a.attnotnull AS nullable, \
                pg_get_expr(d.adbin, d.adrelid) AS default_value, \
                a.attnum AS ordinal_position \
         FROM pg_attribute a \
         LEFT JOIN pg_attrdef d ON a.attrelid = d.adrelid \
              AND a.attnum = d.adnum \
         WHERE a.attrelid = $1::regclass \
               AND a.attnum > 0 \
               AND NOT a.attisdropped \
         ORDER BY a.attnum"
    }

    /// SQL to get constraints for a specific table.
    #[must_use]
    pub fn constraints_sql() -> &'static str {
        "SELECT con.conname AS constraint_name, \
                con.contype AS constraint_type, \
                array_agg(att.attname ORDER BY u.ord) AS columns, \
                confrel.relname AS references_table, \
                array_agg(ref_att.attname ORDER BY u.ord) \
                    FILTER (WHERE ref_att.attname IS NOT NULL) \
                    AS references_columns \
         FROM pg_constraint con \
         JOIN LATERAL unnest(con.conkey) \
              WITH ORDINALITY AS u(attnum, ord) ON TRUE \
         JOIN pg_attribute att ON att.attrelid = con.conrelid \
              AND att.attnum = u.attnum \
         LEFT JOIN pg_class confrel ON confrel.oid = con.confrelid \
         LEFT JOIN LATERAL unnest(con.confkey) \
              WITH ORDINALITY AS ru(attnum, ord) ON TRUE \
         LEFT JOIN pg_attribute ref_att \
              ON ref_att.attrelid = con.confrelid \
              AND ref_att.attnum = ru.attnum \
              AND ru.ord = u.ord \
         WHERE con.conrelid = $1::regclass \
         GROUP BY con.conname, con.contype, confrel.relname \
         ORDER BY con.conname"
    }

    /// SQL to get indexes for a specific table.
    #[must_use]
    pub fn indexes_sql() -> &'static str {
        "SELECT i.relname AS index_name, \
                array_agg(a.attname ORDER BY k.ord) AS columns, \
                ix.indisunique AS is_unique, \
                am.amname AS index_type, \
                ix.indisprimary AS is_primary \
         FROM pg_index ix \
         JOIN pg_class i ON i.oid = ix.indexrelid \
         JOIN pg_am am ON am.oid = i.relam \
         JOIN LATERAL unnest(ix.indkey) \
              WITH ORDINALITY AS k(attnum, ord) ON TRUE \
         JOIN pg_attribute a ON a.attrelid = ix.indrelid \
              AND a.attnum = k.attnum \
         WHERE ix.indrelid = $1::regclass \
         GROUP BY i.relname, ix.indisunique, am.amname, \
                  ix.indisprimary \
         ORDER BY i.relname"
    }

    /// SQL to get column statistics from pg_stats.
    #[must_use]
    pub fn column_stats_sql() -> &'static str {
        "SELECT s.attname, \
                s.n_distinct, \
                s.null_frac, \
                s.avg_width, \
                s.correlation \
         FROM pg_stats s \
         WHERE s.schemaname = $1 AND s.tablename = $2 \
         ORDER BY s.attname"
    }

    /// SQL to get table size statistics.
    #[must_use]
    pub fn table_size_sql() -> &'static str {
        "SELECT pg_total_relation_size($1::regclass) AS total_size, \
                (SELECT reltuples::bigint \
                 FROM pg_class WHERE oid = $1::regclass) AS row_count"
    }

    /// SQL to get index size and usage statistics.
    #[must_use]
    pub fn index_stats_sql() -> &'static str {
        "SELECT indexrelname AS index_name, \
                pg_relation_size(indexrelid) AS size_bytes, \
                idx_scan AS scans, \
                idx_tup_read AS tuples_read, \
                idx_tup_fetch AS tuples_fetched \
         FROM pg_stat_user_indexes \
         WHERE relname = $1 AND schemaname = $2 \
         ORDER BY indexrelname"
    }

    /// SQL to list views.
    #[must_use]
    pub fn list_views_sql() -> &'static str {
        "SELECT schemaname, viewname, definition \
         FROM pg_views \
         WHERE schemaname NOT IN \
               ('pg_catalog', 'information_schema') \
         ORDER BY schemaname, viewname"
    }

    /// Build the EXPLAIN command for a query.
    #[must_use]
    pub fn explain_sql(sql: &str) -> String {
        format!("EXPLAIN (FORMAT JSON) {sql}")
    }
}

/// Intermediate row from the tables listing query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgTableRow {
    /// Schema name.
    pub schemaname: String,
    /// Table name.
    pub tablename: String,
    /// Estimated live tuple count.
    pub n_live_tup: Option<u64>,
}

/// Intermediate row from the columns query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgColumnRow {
    /// Column name.
    pub column_name: String,
    /// Data type string.
    pub data_type: String,
    /// Whether the column is nullable.
    pub nullable: bool,
    /// Default value expression.
    pub default_value: Option<String>,
    /// Ordinal position.
    pub ordinal_position: u32,
}

/// Intermediate row from the constraints query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgConstraintRow {
    /// Constraint name.
    pub constraint_name: String,
    /// Constraint type character (p, u, f, c).
    pub constraint_type: String,
    /// Columns in the constraint.
    pub columns: Vec<String>,
    /// Referenced table (for foreign keys).
    pub references_table: Option<String>,
    /// Referenced columns (for foreign keys).
    pub references_columns: Option<Vec<String>>,
}

/// Intermediate row from the indexes query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgIndexRow {
    /// Index name.
    pub index_name: String,
    /// Columns in the index.
    pub columns: Vec<String>,
    /// Whether the index is unique.
    pub is_unique: bool,
    /// Index access method (btree, hash, etc.).
    pub index_type: String,
    /// Whether this is the primary key index.
    pub is_primary: bool,
}

/// Intermediate row from pg_stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgStatsRow {
    /// Column name.
    pub attname: String,
    /// Number of distinct values. Negative means fraction of rows.
    pub n_distinct: f64,
    /// Fraction of nulls.
    pub null_frac: f64,
    /// Average width in bytes.
    pub avg_width: f64,
    /// Physical vs logical order correlation.
    pub correlation: Option<f64>,
}

/// Intermediate row for table size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgTableSizeRow {
    /// Total size in bytes.
    pub total_size: u64,
    /// Estimated row count.
    pub row_count: u64,
}

/// Intermediate row for index statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgIndexStatsRow {
    /// Index name.
    pub index_name: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Number of scans.
    pub scans: Option<u64>,
    /// Tuples read.
    pub tuples_read: Option<u64>,
    /// Tuples fetched.
    pub tuples_fetched: Option<u64>,
}

/// Intermediate row for views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PgViewRow {
    /// Schema name.
    pub schemaname: String,
    /// View name.
    pub viewname: String,
    /// View definition SQL.
    pub definition: Option<String>,
}

// ── Result parsers ──────────────────────────────────────────

/// Build a [`SchemaInfo`] from PostgreSQL catalog query results.
pub fn build_pg_schema(
    database: &str,
    table_rows: &[PgTableRow],
    columns_by_table: &HashMap<String, Vec<PgColumnRow>>,
    constraints_by_table: &HashMap<String, Vec<PgConstraintRow>>,
    indexes_by_table: &HashMap<String, Vec<PgIndexRow>>,
    view_rows: &[PgViewRow],
    view_columns: &HashMap<String, Vec<PgColumnRow>>,
) -> SchemaInfo {
    let mut tables = Vec::new();
    for row in table_rows {
        let key = format!("{}.{}", row.schemaname, row.tablename);

        let columns = columns_by_table
            .get(&key)
            .map(|cols| {
                cols.iter().map(pg_column_to_info).collect()
            })
            .unwrap_or_default();

        let constraints = constraints_by_table
            .get(&key)
            .map(|cons| {
                cons.iter().map(pg_constraint_to_info).collect()
            })
            .unwrap_or_default();

        let indexes = indexes_by_table
            .get(&key)
            .map(|idxs| {
                idxs.iter().map(pg_index_to_info).collect()
            })
            .unwrap_or_default();

        tables.push(TableInfo {
            schema: row.schemaname.clone(),
            name: row.tablename.clone(),
            columns,
            constraints,
            indexes,
            estimated_rows: row.n_live_tup,
        });
    }

    let mut views = Vec::new();
    for row in view_rows {
        let key =
            format!("{}.{}", row.schemaname, row.viewname);
        let columns = view_columns
            .get(&key)
            .map(|cols| {
                cols.iter().map(pg_column_to_info).collect()
            })
            .unwrap_or_default();

        views.push(ViewInfo {
            schema: row.schemaname.clone(),
            name: row.viewname.clone(),
            columns,
            definition: row.definition.clone(),
        });
    }

    SchemaInfo {
        database: database.to_string(),
        tables,
        views,
    }
}

/// Build [`GatheredTableStats`] from PostgreSQL catalog data.
pub fn build_pg_table_stats(
    table: &str,
    size_row: &PgTableSizeRow,
    stats_rows: &[PgStatsRow],
    index_stats_rows: &[PgIndexStatsRow],
) -> GatheredTableStats {
    let mut columns = HashMap::new();
    for row in stats_rows {
        let distinct = if row.n_distinct < 0.0 {
            // Negative means fraction of rows
            #[allow(clippy::cast_sign_loss)]
            let count = (-row.n_distinct
                * size_row.row_count as f64)
                as u64;
            count
        } else {
            #[allow(clippy::cast_sign_loss)]
            let count = row.n_distinct as u64;
            count
        };

        columns.insert(
            row.attname.clone(),
            GatheredColumnStats {
                distinct_count: distinct,
                null_fraction: row.null_frac,
                avg_width: row.avg_width,
                correlation: row.correlation,
                most_common_values: None,
            },
        );
    }

    let mut indexes = HashMap::new();
    for row in index_stats_rows {
        indexes.insert(
            row.index_name.clone(),
            GatheredIndexStats {
                size_bytes: row.size_bytes,
                scans: row.scans,
                tuples_read: row.tuples_read,
                tuples_fetched: row.tuples_fetched,
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

/// Parse PostgreSQL EXPLAIN JSON output into an [`ExplainPlan`].
pub fn parse_pg_explain(
    json_text: &str,
    query: &str,
) -> Result<ExplainPlan, MetadataError> {
    parse_postgres_explain(json_text, query)
}

fn pg_column_to_info(row: &PgColumnRow) -> ColumnInfo {
    ColumnInfo {
        name: row.column_name.clone(),
        data_type: row.data_type.clone(),
        nullable: row.nullable,
        default_value: row.default_value.clone(),
        ordinal_position: row.ordinal_position,
    }
}

fn pg_constraint_to_info(
    row: &PgConstraintRow,
) -> ConstraintInfo {
    let constraint_type = match row.constraint_type.as_str() {
        "p" => ConstraintType::PrimaryKey,
        "u" => ConstraintType::Unique,
        "f" => ConstraintType::ForeignKey,
        _ => ConstraintType::Check,
    };

    ConstraintInfo {
        name: row.constraint_name.clone(),
        constraint_type,
        columns: row.columns.clone(),
        references_table: row.references_table.clone(),
        references_columns: row.references_columns.clone(),
    }
}

fn pg_index_to_info(row: &PgIndexRow) -> IndexInfo {
    IndexInfo {
        name: row.index_name.clone(),
        columns: row.columns.clone(),
        unique: row.is_unique,
        index_type: row.index_type.clone(),
        primary: row.is_primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tables_sql_is_valid() {
        let sql = PostgresQueries::list_tables_sql();
        assert!(sql.contains("pg_stat_user_tables"));
        assert!(sql.contains("schemaname"));
    }

    #[test]
    fn columns_sql_references_pg_attribute() {
        let sql = PostgresQueries::columns_sql();
        assert!(sql.contains("pg_attribute"));
        assert!(sql.contains("attname"));
    }

    #[test]
    fn explain_sql_wraps_query() {
        let sql =
            PostgresQueries::explain_sql("SELECT 1");
        assert_eq!(
            sql,
            "EXPLAIN (FORMAT JSON) SELECT 1"
        );
    }

    #[test]
    fn build_schema_empty() {
        let schema = build_pg_schema(
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
        assert!(schema.views.is_empty());
    }

    #[test]
    fn build_schema_with_table() {
        let table_rows = vec![PgTableRow {
            schemaname: "public".to_string(),
            tablename: "users".to_string(),
            n_live_tup: Some(1000),
        }];

        let mut columns = HashMap::new();
        columns.insert(
            "public.users".to_string(),
            vec![PgColumnRow {
                column_name: "id".to_string(),
                data_type: "integer".to_string(),
                nullable: false,
                default_value: Some(
                    "nextval('users_id_seq')".to_string(),
                ),
                ordinal_position: 1,
            }],
        );

        let schema = build_pg_schema(
            "testdb",
            &table_rows,
            &columns,
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &HashMap::new(),
        );

        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "users");
        assert_eq!(schema.tables[0].columns.len(), 1);
        assert_eq!(
            schema.tables[0].estimated_rows,
            Some(1000)
        );
    }

    #[test]
    fn build_table_stats_with_negative_ndistinct() {
        let size = PgTableSizeRow {
            total_size: 1_048_576,
            row_count: 10_000,
        };

        let stats = vec![PgStatsRow {
            attname: "status".to_string(),
            n_distinct: -0.05,
            null_frac: 0.0,
            avg_width: 8.0,
            correlation: Some(0.1),
        }];

        let result =
            build_pg_table_stats("orders", &size, &stats, &[]);

        assert_eq!(result.row_count, 10_000);
        let col = result
            .columns
            .get("status")
            .expect("status column");
        assert_eq!(col.distinct_count, 500);
    }

    #[test]
    fn build_table_stats_with_indexes() {
        let size = PgTableSizeRow {
            total_size: 500_000,
            row_count: 5000,
        };

        let idx_stats = vec![PgIndexStatsRow {
            index_name: "users_pkey".to_string(),
            size_bytes: 100_000,
            scans: Some(42),
            tuples_read: Some(1000),
            tuples_fetched: Some(900),
        }];

        let result = build_pg_table_stats(
            "users",
            &size,
            &[],
            &idx_stats,
        );

        let idx = result
            .indexes
            .get("users_pkey")
            .expect("pkey index");
        assert_eq!(idx.size_bytes, 100_000);
        assert_eq!(idx.scans, Some(42));
    }

    #[test]
    fn constraint_type_mapping() {
        let row = PgConstraintRow {
            constraint_name: "users_pkey".to_string(),
            constraint_type: "p".to_string(),
            columns: vec!["id".to_string()],
            references_table: None,
            references_columns: None,
        };
        let info = pg_constraint_to_info(&row);
        assert_eq!(
            info.constraint_type,
            ConstraintType::PrimaryKey
        );

        let fk_row = PgConstraintRow {
            constraint_name: "orders_user_fk".to_string(),
            constraint_type: "f".to_string(),
            columns: vec!["user_id".to_string()],
            references_table: Some("users".to_string()),
            references_columns: Some(vec!["id".to_string()]),
        };
        let fk_info = pg_constraint_to_info(&fk_row);
        assert_eq!(
            fk_info.constraint_type,
            ConstraintType::ForeignKey
        );
        assert_eq!(
            fk_info.references_table.as_deref(),
            Some("users")
        );
    }

    #[test]
    fn pg_view_row_serialization() {
        let row = PgViewRow {
            schemaname: "public".to_string(),
            viewname: "active_users".to_string(),
            definition: Some(
                "SELECT * FROM users WHERE active".to_string(),
            ),
        };
        let json = serde_json::to_string(&row)
            .expect("should serialize");
        let roundtrip: PgViewRow = serde_json::from_str(&json)
            .expect("should deserialize");
        assert_eq!(row.viewname, roundtrip.viewname);
    }
}
