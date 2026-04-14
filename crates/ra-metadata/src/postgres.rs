//! `PostgreSQL` database connector.
//!
//! Queries `PostgreSQL` system catalogs (`pg_class`, `pg_attribute`,
//! `pg_constraint`, `pg_stats`, `pg_indexes`) and parses
//! `EXPLAIN (FORMAT JSON)` output.

use std::collections::HashMap;

use postgres::{Client, NoTls};

use crate::connector::{DatabaseConnector, MetadataResult};
use crate::error::MetadataError;
use crate::explain::{parse_postgres_explain, ExplainPlan};
use crate::schema::{
    ColumnInfo, ColumnStatistics, ConstraintInfo, ConstraintKind, DatabaseKind, IndexInfo,
    SchemaInfo, TableInfo, TableStats, TriggerEvent, TriggerInfo, TriggerScope, TriggerTiming,
};

/// `PostgreSQL` connector using the `postgres` crate.
pub struct PostgresConnector {
    client: Client,
    schema: String,
}

impl PostgresConnector {
    /// Connect to a `PostgreSQL` database.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` if the connection fails.
    pub fn connect(connection_string: &str) -> MetadataResult<Self> {
        let client =
            Client::connect(connection_string, NoTls).map_err(|e| MetadataError::Connection {
                message: format!("PostgreSQL connection failed: {e}"),
            })?;

        Ok(Self {
            client,
            schema: "public".to_owned(),
        })
    }

    /// Set the schema to query (defaults to "public").
    pub fn set_schema(&mut self, schema: &str) {
        schema.clone_into(&mut self.schema);
    }

    fn query_tables(&mut self) -> MetadataResult<Vec<String>> {
        let rows = self
            .client
            .query(
                "SELECT tablename FROM pg_tables \
                 WHERE schemaname = $1 \
                 ORDER BY tablename",
                &[&self.schema],
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to list tables: {e}"),
            })?;

        let mut tables = Vec::new();
        for row in &rows {
            let name: String = row.get(0);
            tables.push(name);
        }
        Ok(tables)
    }

    fn query_columns(&mut self, table: &str) -> MetadataResult<Vec<ColumnInfo>> {
        let rows = self
            .client
            .query(
                "SELECT column_name, data_type, is_nullable, \
                 ordinal_position, column_default \
                 FROM information_schema.columns \
                 WHERE table_schema = $1 AND table_name = $2 \
                 ORDER BY ordinal_position",
                &[&self.schema, &table],
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query columns for {table}: {e}"),
            })?;

        let mut columns = Vec::new();
        for row in &rows {
            let name: String = row.get(0);
            let data_type: String = row.get(1);
            let nullable_str: String = row.get(2);
            let ordinal: i32 = row.get(3);
            let default_value: Option<String> = row.get(4);

            columns.push(ColumnInfo {
                name,
                data_type,
                nullable: nullable_str == "YES",
                ordinal: ordinal as u32,
                default_value,
            });
        }
        Ok(columns)
    }

    fn query_constraints(&mut self, table: &str) -> MetadataResult<Vec<ConstraintInfo>> {
        let rows = self
            .client
            .query(
                "SELECT c.conname, c.contype::text, \
                 array_agg(a.attname ORDER BY x.n) AS columns, \
                 ft.relname AS fk_table, \
                 array_agg(fa.attname ORDER BY fx.n) \
                   FILTER (WHERE fa.attname IS NOT NULL) \
                   AS fk_columns \
                 FROM pg_constraint c \
                 JOIN pg_class t ON c.conrelid = t.oid \
                 JOIN pg_namespace ns ON t.relnamespace = ns.oid \
                 JOIN LATERAL unnest(c.conkey) \
                   WITH ORDINALITY AS x(attnum, n) ON true \
                 JOIN pg_attribute a ON a.attrelid = t.oid \
                   AND a.attnum = x.attnum \
                 LEFT JOIN pg_class ft ON c.confrelid = ft.oid \
                 LEFT JOIN LATERAL unnest(c.confkey) \
                   WITH ORDINALITY AS fx(attnum, n) ON true \
                 LEFT JOIN pg_attribute fa ON fa.attrelid = ft.oid \
                   AND fa.attnum = fx.attnum \
                 WHERE ns.nspname = $1 AND t.relname = $2 \
                 GROUP BY c.conname, c.contype, ft.relname",
                &[&self.schema, &table],
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query constraints for {table}: {e}"),
            })?;

        let mut constraints = Vec::new();
        for row in &rows {
            let name: String = row.get(0);
            let contype: String = row.get(1);
            let columns: Vec<String> = row.get(2);
            let referenced_table: Option<String> = row.get(3);
            let referenced_columns: Option<Vec<String>> = row.get(4);

            let kind = match contype.as_str() {
                "p" => ConstraintKind::PrimaryKey,
                "f" => ConstraintKind::ForeignKey,
                "u" => ConstraintKind::Unique,
                "c" => ConstraintKind::Check,
                _ => continue,
            };

            constraints.push(ConstraintInfo {
                name,
                kind,
                columns,
                referenced_table,
                referenced_columns: referenced_columns.unwrap_or_default(),
                check_expression: None,
            });
        }
        Ok(constraints)
    }

    fn query_indexes(&mut self, table: &str) -> MetadataResult<Vec<IndexInfo>> {
        let rows = self
            .client
            .query(
                "SELECT i.relname, am.amname, \
                 ix.indisunique, \
                 array_agg(a.attname ORDER BY x.n) AS columns \
                 FROM pg_index ix \
                 JOIN pg_class t ON ix.indrelid = t.oid \
                 JOIN pg_class i ON ix.indexrelid = i.oid \
                 JOIN pg_namespace ns ON t.relnamespace = ns.oid \
                 JOIN pg_am am ON i.relam = am.oid \
                 JOIN LATERAL unnest(ix.indkey) \
                   WITH ORDINALITY AS x(attnum, n) ON true \
                 JOIN pg_attribute a ON a.attrelid = t.oid \
                   AND a.attnum = x.attnum \
                 WHERE ns.nspname = $1 AND t.relname = $2 \
                 GROUP BY i.relname, am.amname, ix.indisunique",
                &[&self.schema, &table],
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query indexes for {table}: {e}"),
            })?;

        let mut indexes = Vec::new();
        for row in &rows {
            let name: String = row.get(0);
            let index_type: String = row.get(1);
            let unique: bool = row.get(2);
            let columns: Vec<String> = row.get(3);

            indexes.push(IndexInfo {
                name,
                columns,
                unique,
                index_type,
            });
        }
        Ok(indexes)
    }

    fn query_table_stats(&mut self, table: &str) -> MetadataResult<(f64, u64)> {
        let rows = self
            .client
            .query(
                "SELECT reltuples::float8, \
                 pg_total_relation_size(c.oid)::bigint \
                 FROM pg_class c \
                 JOIN pg_namespace ns ON c.relnamespace = ns.oid \
                 WHERE ns.nspname = $1 AND c.relname = $2",
                &[&self.schema, &table],
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query table stats for {table}: {e}"),
            })?;

        let row = rows.first().ok_or_else(|| MetadataError::Query {
            message: format!("table not found: {table}"),
        })?;

        let row_count: f64 = row.get(0);
        let total_bytes: i64 = row.get(1);

        Ok((row_count.max(0.0), total_bytes.max(0) as u64))
    }

    fn query_column_stats(
        &mut self,
        table: &str,
    ) -> MetadataResult<HashMap<String, ColumnStatistics>> {
        let rows = self
            .client
            .query(
                "SELECT attname, n_distinct, null_frac, \
                 avg_width, \
                 most_common_vals::text, most_common_freqs, \
                 histogram_bounds::text \
                 FROM pg_stats \
                 WHERE schemaname = $1 AND tablename = $2",
                &[&self.schema, &table],
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query pg_stats for {table}: {e}"),
            })?;

        let mut columns = HashMap::new();
        for row in &rows {
            let col_name: String = row.get(0);
            let n_distinct: f32 = row.get(1);
            let null_frac: f32 = row.get(2);
            let avg_width: i32 = row.get(3);
            let mcv_text: Option<String> = row.get(4);
            let mcv_freqs: Option<Vec<f32>> = row.get(5);
            let hist_text: Option<String> = row.get(6);

            let distinct_count = if n_distinct >= 0.0 {
                f64::from(n_distinct)
            } else {
                // Negative means fraction of rows
                0.0
            };

            let most_common_values = parse_pg_mcv(mcv_text.as_ref(), mcv_freqs.as_ref());
            let histogram_bounds = parse_pg_histogram(hist_text.as_ref());

            columns.insert(
                col_name.clone(),
                ColumnStatistics {
                    column_name: col_name,
                    distinct_count,
                    null_fraction: f64::from(null_frac),
                    avg_width: Some(f64::from(avg_width)),
                    most_common_values,
                    histogram_bounds,
                },
            );
        }

        Ok(columns)
    }

    fn query_triggers(&mut self, table: &str) -> MetadataResult<Vec<TriggerInfo>> {
        let rows = self
            .client
            .query(
                "SELECT t.tgname, \
                 CASE WHEN t.tgtype & 4 > 0 THEN 'INSERT' \
                      WHEN t.tgtype & 8 > 0 THEN 'DELETE' \
                      WHEN t.tgtype & 16 > 0 THEN 'UPDATE' \
                      WHEN t.tgtype & 32 > 0 THEN 'TRUNCATE' \
                      ELSE 'UNKNOWN' END AS event, \
                 CASE WHEN t.tgtype & 2 > 0 THEN 'BEFORE' \
                      WHEN t.tgtype & 64 > 0 THEN 'INSTEAD OF' \
                      ELSE 'AFTER' END AS timing, \
                 CASE WHEN t.tgtype & 1 > 0 THEN 'ROW' \
                      ELSE 'STATEMENT' END AS scope, \
                 p.proname AS function_name, \
                 t.tgenabled \
                 FROM pg_trigger t \
                 JOIN pg_class c ON t.tgrelid = c.oid \
                 JOIN pg_namespace ns ON c.relnamespace = ns.oid \
                 JOIN pg_proc p ON t.tgfoid = p.oid \
                 WHERE ns.nspname = $1 AND c.relname = $2 \
                 AND NOT t.tgisinternal",
                &[&self.schema, &table],
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query triggers for {table}: {e}"),
            })?;

        let mut triggers = Vec::new();
        for row in &rows {
            let name: String = row.get(0);
            let event_str: String = row.get(1);
            let timing_str: String = row.get(2);
            let scope_str: String = row.get(3);
            let function_name: String = row.get(4);
            let enabled_char: String = row.get(5);

            let event = match event_str.as_str() {
                "INSERT" => TriggerEvent::Insert,
                "DELETE" => TriggerEvent::Delete,
                "UPDATE" => TriggerEvent::Update,
                "TRUNCATE" => TriggerEvent::Truncate,
                _ => continue,
            };

            let timing = match timing_str.as_str() {
                "BEFORE" => TriggerTiming::Before,
                "INSTEAD OF" => TriggerTiming::InsteadOf,
                _ => TriggerTiming::After,
            };

            let scope = match scope_str.as_str() {
                "ROW" => TriggerScope::Row,
                _ => TriggerScope::Statement,
            };

            triggers.push(TriggerInfo {
                name,
                event,
                timing,
                scope,
                action_sql: format!("EXECUTE FUNCTION {function_name}()"),
                table_name: table.to_owned(),
                enabled: enabled_char != "D",
            });
        }
        Ok(triggers)
    }

    fn query_check_expressions(&mut self, table: &str) -> MetadataResult<HashMap<String, String>> {
        let rows = self
            .client
            .query(
                "SELECT c.conname, \
                 pg_get_constraintdef(c.oid) \
                 FROM pg_constraint c \
                 JOIN pg_class t ON c.conrelid = t.oid \
                 JOIN pg_namespace ns ON t.relnamespace = ns.oid \
                 WHERE ns.nspname = $1 AND t.relname = $2 \
                 AND c.contype = 'c'",
                &[&self.schema, &table],
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query check constraints for \
                     {table}: {e}"
                ),
            })?;

        let mut result = HashMap::new();
        for row in &rows {
            let name: String = row.get(0);
            let expr: String = row.get(1);
            result.insert(name, expr);
        }
        Ok(result)
    }

    /// Gather full schema information.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_schema_mut(&mut self) -> MetadataResult<SchemaInfo> {
        let table_names = self.query_tables()?;
        let mut tables = HashMap::new();

        for name in &table_names {
            let columns = self.query_columns(name)?;
            let mut constraints = self.query_constraints(name)?;
            let indexes = self.query_indexes(name)?;
            let triggers = self.query_triggers(name)?;
            let (row_count, _) = self.query_table_stats(name)?;
            let check_exprs = self.query_check_expressions(name)?;

            for constraint in &mut constraints {
                if constraint.kind == ConstraintKind::Check {
                    constraint.check_expression = check_exprs.get(&constraint.name).cloned();
                }
            }

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
            kind: DatabaseKind::PostgreSQL,
            schema_name: self.schema.clone(),
            tables,
        })
    }

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_statistics_mut(&mut self, table: &str) -> MetadataResult<TableStats> {
        let (row_count, total_bytes) = self.query_table_stats(table)?;
        let columns = self.query_column_stats(table)?;

        Ok(TableStats {
            table_name: table.to_owned(),
            row_count,
            total_bytes,
            columns,
        })
    }

    /// Execute EXPLAIN (FORMAT JSON) on a query.
    ///
    /// # Errors
    ///
    /// Returns errors if the EXPLAIN query fails or output cannot
    /// be parsed.
    pub fn explain_query_mut(&mut self, sql: &str) -> MetadataResult<ExplainPlan> {
        let explain_sql = format!("EXPLAIN (FORMAT JSON) {sql}");
        let rows = self
            .client
            .query(&explain_sql, &[])
            .map_err(|e| MetadataError::Query {
                message: format!("EXPLAIN failed for query: {e}"),
            })?;

        let json_value: serde_json::Value = rows
            .first()
            .ok_or_else(|| MetadataError::ExplainParse {
                message: "no EXPLAIN output".to_owned(),
            })?
            .get(0);
        let json_text = json_value.to_string();

        parse_postgres_explain(&json_text)
    }
}

impl DatabaseConnector for PostgresConnector {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::PostgreSQL
    }

    fn gather_schema(&self) -> MetadataResult<SchemaInfo> {
        Err(MetadataError::Unsupported {
            message: "use gather_schema_mut() instead".to_owned(),
        })
    }

    fn gather_statistics(&self, _table: &str) -> MetadataResult<TableStats> {
        Err(MetadataError::Unsupported {
            message: "use gather_statistics_mut() instead".to_owned(),
        })
    }

    fn explain_query(&self, _sql: &str) -> MetadataResult<ExplainPlan> {
        Err(MetadataError::Unsupported {
            message: "use explain_query_mut() instead".to_owned(),
        })
    }
}

/// Parse `PostgreSQL` `most_common_vals` and `most_common_freqs` arrays.
fn parse_pg_mcv(vals_text: Option<&String>, freqs: Option<&Vec<f32>>) -> Vec<(String, f64)> {
    let Some(vals_text) = vals_text else {
        return Vec::new();
    };
    let Some(freqs) = freqs else {
        return Vec::new();
    };

    let vals: Vec<&str> = vals_text
        .trim_matches(|c| c == '{' || c == '}')
        .split(',')
        .collect();

    let mut result = Vec::new();
    for (val, freq) in vals.iter().zip(freqs.iter()) {
        result.push((val.trim().to_owned(), f64::from(*freq)));
    }
    result
}

/// Parse `PostgreSQL` `histogram_bounds` text array.
fn parse_pg_histogram(text: Option<&String>) -> Vec<String> {
    let Some(text) = text else {
        return Vec::new();
    };

    text.trim_matches(|c| c == '{' || c == '}')
        .split(',')
        .map(|s| s.trim().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mcv_empty() {
        let result = parse_pg_mcv(None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_mcv_values() {
        let vals = Some("{apple,banana,cherry}".to_owned());
        let freqs = Some(vec![0.3, 0.2, 0.1]);
        let result = parse_pg_mcv(vals.as_ref(), freqs.as_ref());
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, "apple");
        assert!((result[0].1 - 0.3).abs() < 0.001);
    }

    #[test]
    fn parse_histogram_empty() {
        let result = parse_pg_histogram(None);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_histogram_values() {
        let text = Some("{1,10,20,30,40,50}".to_owned());
        let result = parse_pg_histogram(text.as_ref());
        assert_eq!(result.len(), 6);
        assert_eq!(result[0], "1");
        assert_eq!(result[5], "50");
    }

    #[test]
    fn connector_kind() {
        assert_eq!(DatabaseKind::PostgreSQL.to_string(), "PostgreSQL");
    }
}
