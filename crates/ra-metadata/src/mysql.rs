//! `MySQL` database connector.
//!
//! Queries `MySQL` `information_schema` tables and parses
//! `EXPLAIN FORMAT=JSON` output.

use std::collections::HashMap;

use mysql::prelude::Queryable;
use mysql::{Conn, Opts};

use crate::connector::{DatabaseConnector, MetadataResult};
use crate::error::MetadataError;
use crate::explain::{ExplainPlan, parse_mysql_explain};
use crate::schema::{
    ColumnInfo, ColumnStatistics, ConstraintInfo, ConstraintKind,
    DatabaseKind, IndexInfo, SchemaInfo, TableInfo, TableStats,
    TriggerEvent, TriggerInfo, TriggerScope, TriggerTiming,
};

/// Row type for constraint queries.
type ConstraintRow =
    (String, String, String, Option<String>, Option<String>);

/// `MySQL` connector using the `mysql` crate.
pub struct MySqlConnector {
    conn: Conn,
    database: String,
}

impl MySqlConnector {
    /// Connect to a `MySQL` database.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` if the connection fails.
    pub fn connect(
        connection_string: &str,
    ) -> MetadataResult<Self> {
        let opts = Opts::from_url(connection_string)
            .map_err(|e| MetadataError::Connection {
                message: format!(
                    "invalid MySQL connection string: {e}"
                ),
            })?;

        let database = opts
            .get_db_name()
            .unwrap_or("information_schema")
            .to_owned();

        let conn = Conn::new(opts).map_err(|e| {
            MetadataError::Connection {
                message: format!(
                    "MySQL connection failed: {e}"
                ),
            }
        })?;

        Ok(Self { conn, database })
    }

    fn query_tables(
        &mut self,
    ) -> MetadataResult<Vec<String>> {
        let tables: Vec<String> = self
            .conn
            .exec(
                "SELECT TABLE_NAME \
                 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? \
                 AND TABLE_TYPE = 'BASE TABLE' \
                 ORDER BY TABLE_NAME",
                (&self.database,),
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to list tables: {e}"),
            })?;

        Ok(tables)
    }

    fn query_columns(
        &mut self,
        table: &str,
    ) -> MetadataResult<Vec<ColumnInfo>> {
        let rows: Vec<(String, String, String, u32, Option<String>)> =
            self.conn
                .exec(
                    "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, \
                     ORDINAL_POSITION, COLUMN_DEFAULT \
                     FROM information_schema.COLUMNS \
                     WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                     ORDER BY ORDINAL_POSITION",
                    (&self.database, table),
                )
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to query columns for {table}: {e}"
                    ),
                })?;

        let mut columns = Vec::new();
        for (name, data_type, nullable_str, ordinal, default_value) in
            rows
        {
            columns.push(ColumnInfo {
                name,
                data_type,
                nullable: nullable_str == "YES",
                ordinal,
                default_value,
            });
        }
        Ok(columns)
    }

    fn query_constraints(
        &mut self,
        table: &str,
    ) -> MetadataResult<Vec<ConstraintInfo>> {
        let rows: Vec<ConstraintRow> = self
            .conn
            .exec(
                "SELECT tc.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE, \
                 GROUP_CONCAT(kcu.COLUMN_NAME \
                   ORDER BY kcu.ORDINAL_POSITION), \
                 kcu.REFERENCED_TABLE_NAME, \
                 GROUP_CONCAT(kcu.REFERENCED_COLUMN_NAME \
                   ORDER BY kcu.ORDINAL_POSITION) \
                 FROM information_schema.TABLE_CONSTRAINTS tc \
                 JOIN information_schema.KEY_COLUMN_USAGE kcu \
                   ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
                   AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA \
                   AND tc.TABLE_NAME = kcu.TABLE_NAME \
                 WHERE tc.TABLE_SCHEMA = ? AND tc.TABLE_NAME = ? \
                 GROUP BY tc.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE, \
                   kcu.REFERENCED_TABLE_NAME",
                (&self.database, table),
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query constraints for {table}: {e}"
                ),
            })?;

        let mut constraints = Vec::new();
        for (name, ctype, cols_str, ref_table, ref_cols_str) in rows
        {
            let kind = match ctype.as_str() {
                "PRIMARY KEY" => ConstraintKind::PrimaryKey,
                "FOREIGN KEY" => ConstraintKind::ForeignKey,
                "UNIQUE" => ConstraintKind::Unique,
                "CHECK" => ConstraintKind::Check,
                _ => continue,
            };

            let columns: Vec<String> = cols_str
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect();
            let referenced_columns: Vec<String> = ref_cols_str
                .map(|s| {
                    s.split(',')
                        .map(|s| s.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default();

            constraints.push(ConstraintInfo {
                name,
                kind,
                columns,
                referenced_table: ref_table,
                referenced_columns,
                check_expression: None,
            });
        }

        Ok(constraints)
    }

    fn query_indexes(
        &mut self,
        table: &str,
    ) -> MetadataResult<Vec<IndexInfo>> {
        let rows: Vec<(String, u32, String, String)> = self
            .conn
            .exec(
                "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME, \
                 INDEX_TYPE \
                 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                 ORDER BY INDEX_NAME, SEQ_IN_INDEX",
                (&self.database, table),
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query indexes for {table}: {e}"
                ),
            })?;

        let mut index_map: HashMap<String, IndexInfo> =
            HashMap::new();

        for (idx_name, non_unique, col_name, idx_type) in rows {
            let entry =
                index_map.entry(idx_name.clone()).or_insert_with(
                    || IndexInfo {
                        name: idx_name,
                        columns: Vec::new(),
                        unique: non_unique == 0,
                        index_type: idx_type,
                    },
                );
            entry.columns.push(col_name);
        }

        let mut indexes: Vec<IndexInfo> =
            index_map.into_values().collect();
        indexes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(indexes)
    }

    fn query_table_row_count(
        &mut self,
        table: &str,
    ) -> MetadataResult<(f64, u64)> {
        let rows: Vec<(f64, u64)> = self
            .conn
            .exec(
                "SELECT TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH \
                 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (&self.database, table),
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query table stats for {table}: {e}"
                ),
            })?;

        let (row_count, total_bytes) =
            rows.first().ok_or_else(|| MetadataError::Query {
                message: format!("table not found: {table}"),
            })?;

        Ok((*row_count, *total_bytes))
    }

    fn query_triggers(
        &mut self,
        table: &str,
    ) -> MetadataResult<Vec<TriggerInfo>> {
        let rows: Vec<(String, String, String, String)> = self
            .conn
            .exec(
                "SELECT TRIGGER_NAME, EVENT_MANIPULATION, \
                 ACTION_TIMING, ACTION_STATEMENT \
                 FROM information_schema.TRIGGERS \
                 WHERE TRIGGER_SCHEMA = ? \
                 AND EVENT_OBJECT_TABLE = ?",
                (&self.database, table),
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query triggers for {table}: {e}"
                ),
            })?;

        let mut triggers = Vec::new();
        for (name, event_str, timing_str, action_sql) in rows {
            let event = match event_str.as_str() {
                "INSERT" => TriggerEvent::Insert,
                "DELETE" => TriggerEvent::Delete,
                "UPDATE" => TriggerEvent::Update,
                _ => continue,
            };

            let timing = match timing_str.as_str() {
                "BEFORE" => TriggerTiming::Before,
                _ => TriggerTiming::After,
            };

            triggers.push(TriggerInfo {
                name,
                event,
                timing,
                scope: TriggerScope::Row,
                action_sql,
                table_name: table.to_owned(),
                enabled: true,
            });
        }
        Ok(triggers)
    }

    /// Gather full schema information.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_schema_mut(
        &mut self,
    ) -> MetadataResult<SchemaInfo> {
        let table_names = self.query_tables()?;
        let mut tables = HashMap::new();

        for name in &table_names {
            let columns = self.query_columns(name)?;
            let constraints = self.query_constraints(name)?;
            let indexes = self.query_indexes(name)?;
            let triggers = self.query_triggers(name)?;
            let (row_count, _) =
                self.query_table_row_count(name)?;

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
            kind: DatabaseKind::MySQL,
            schema_name: self.database.clone(),
            tables,
        })
    }

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_statistics_mut(
        &mut self,
        table: &str,
    ) -> MetadataResult<TableStats> {
        let (row_count, total_bytes) =
            self.query_table_row_count(table)?;

        let rows: Vec<(String, Option<f64>)> = self
            .conn
            .exec(
                "SELECT COLUMN_NAME, CARDINALITY \
                 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                 AND SEQ_IN_INDEX = 1",
                (&self.database, table),
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query column stats for {table}: {e}"
                ),
            })?;

        let mut columns = HashMap::new();
        for (col_name, cardinality) in rows {
            columns.insert(
                col_name.clone(),
                ColumnStatistics {
                    column_name: col_name,
                    distinct_count: cardinality.unwrap_or(0.0),
                    null_fraction: 0.0,
                    avg_width: None,
                    most_common_values: vec![],
                    histogram_bounds: vec![],
                },
            );
        }

        Ok(TableStats {
            table_name: table.to_owned(),
            row_count,
            total_bytes,
            columns,
        })
    }

    /// Execute EXPLAIN FORMAT=JSON on a query.
    ///
    /// # Errors
    ///
    /// Returns errors if the EXPLAIN query fails or output cannot
    /// be parsed.
    pub fn explain_query_mut(
        &mut self,
        sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        let explain_sql =
            format!("EXPLAIN FORMAT=JSON {sql}");
        let rows: Vec<String> = self
            .conn
            .query(&explain_sql)
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "EXPLAIN failed for query: {e}"
                ),
            })?;

        let json_text = rows.first().ok_or_else(|| {
            MetadataError::ExplainParse {
                message: "no EXPLAIN output".to_owned(),
            }
        })?;

        parse_mysql_explain(json_text)
    }
}

impl DatabaseConnector for MySqlConnector {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::MySQL
    }

    fn gather_schema(&self) -> MetadataResult<SchemaInfo> {
        Err(MetadataError::Unsupported {
            message: "use gather_schema_mut() instead".to_owned(),
        })
    }

    fn gather_statistics(
        &self,
        _table: &str,
    ) -> MetadataResult<TableStats> {
        Err(MetadataError::Unsupported {
            message: "use gather_statistics_mut() instead"
                .to_owned(),
        })
    }

    fn explain_query(
        &self,
        _sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        Err(MetadataError::Unsupported {
            message: "use explain_query_mut() instead".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_kind() {
        assert_eq!(DatabaseKind::MySQL.to_string(), "MySQL");
    }
}
