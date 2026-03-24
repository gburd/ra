//! Oracle Database connector.
//!
//! Queries Oracle data dictionary views (`ALL_TABLES`,
//! `ALL_TAB_COLUMNS`, `ALL_INDEXES`, `ALL_CONSTRAINTS`,
//! `ALL_TAB_STATISTICS`, `ALL_TAB_HISTOGRAMS`) and parses
//! `EXPLAIN PLAN` output.

use std::collections::HashMap;

use oracle::Connection;

use crate::connector::{DatabaseConnector, MetadataResult};
use crate::error::MetadataError;
use crate::explain::{
    ExplainNode, ExplainPlan, NodeType,
};
use crate::schema::{
    ColumnInfo, ColumnStatistics, ConstraintInfo, ConstraintKind,
    DatabaseKind, IndexInfo, SchemaInfo, TableInfo, TableStats,
};

/// Oracle connector using the `oracle` crate.
pub struct OracleConnector {
    conn: Connection,
    schema: String,
}

impl OracleConnector {
    /// Connect to an Oracle database.
    ///
    /// `connection_string` should be in the format:
    /// `oracle://user:pass@host:port/service_name`
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` on failure.
    pub fn connect(
        connection_string: &str,
    ) -> MetadataResult<Self> {
        let parts =
            parse_oracle_url(connection_string)?;

        let connect_str = format!(
            "//{}:{}/{}",
            parts.host, parts.port, parts.service
        );

        let conn = Connection::connect(
            &parts.user,
            &parts.password,
            &connect_str,
        )
        .map_err(|e| MetadataError::Connection {
            message: format!(
                "Oracle connection failed: {e}"
            ),
        })?;

        let schema = parts.user.to_uppercase();

        Ok(Self { conn, schema })
    }

    /// Set the schema/owner to query.
    pub fn set_schema(&mut self, schema: &str) {
        self.schema = schema.to_uppercase();
    }

    fn query_tables(
        &self,
    ) -> MetadataResult<Vec<String>> {
        let sql =
            "SELECT table_name \
             FROM all_tables \
             WHERE owner = :1 \
             ORDER BY table_name";

        let rows = self
            .conn
            .query_as::<(String,)>(sql, &[&self.schema])
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to list tables: {e}"
                ),
            })?;

        let mut tables = Vec::new();
        for row in rows {
            let (name,) = row.map_err(|e| {
                MetadataError::Query {
                    message: format!(
                        "failed to read table: {e}"
                    ),
                }
            })?;
            tables.push(name);
        }
        Ok(tables)
    }

    fn query_columns(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<ColumnInfo>> {
        let sql =
            "SELECT column_name, data_type, nullable, \
             column_id, data_default \
             FROM all_tab_columns \
             WHERE owner = :1 AND table_name = :2 \
             ORDER BY column_id";

        let rows = self
            .conn
            .query_as::<(
                String,
                String,
                String,
                u32,
                Option<String>,
            )>(sql, &[&self.schema, &table.to_uppercase()])
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query columns for {table}: {e}"
                ),
            })?;

        let mut columns = Vec::new();
        for row in rows {
            let (name, data_type, nullable_str, ordinal, default_value) =
                row.map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read column: {e}"
                    ),
                })?;

            columns.push(ColumnInfo {
                name,
                data_type,
                nullable: nullable_str == "Y",
                ordinal,
                default_value,
            });
        }
        Ok(columns)
    }

    fn query_constraints(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<ConstraintInfo>> {
        let sql =
            "SELECT c.constraint_name, c.constraint_type, \
             LISTAGG(cc.column_name, ',') \
               WITHIN GROUP (ORDER BY cc.position), \
             c.r_constraint_name \
             FROM all_constraints c \
             JOIN all_cons_columns cc \
               ON c.owner = cc.owner \
               AND c.constraint_name = cc.constraint_name \
             WHERE c.owner = :1 AND c.table_name = :2 \
             GROUP BY c.constraint_name, c.constraint_type, \
               c.r_constraint_name";

        let rows = self
            .conn
            .query_as::<(
                String,
                String,
                String,
                Option<String>,
            )>(sql, &[&self.schema, &table.to_uppercase()])
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query constraints for \
                     {table}: {e}"
                ),
            })?;

        let mut constraints = Vec::new();
        for row in rows {
            let (name, ctype, cols_str, ref_constraint) =
                row.map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read constraint: {e}"
                    ),
                })?;

            let kind = match ctype.as_str() {
                "P" => ConstraintKind::PrimaryKey,
                "R" => ConstraintKind::ForeignKey,
                "U" => ConstraintKind::Unique,
                "C" => ConstraintKind::Check,
                _ => continue,
            };

            let columns: Vec<String> = cols_str
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();

            // For FKs, resolve referenced table.
            let referenced_table = if kind
                == ConstraintKind::ForeignKey
            {
                ref_constraint
                    .as_ref()
                    .and_then(|rc| {
                        self.resolve_ref_table(rc).ok()
                    })
            } else {
                None
            };

            constraints.push(ConstraintInfo {
                name,
                kind,
                columns,
                referenced_table,
                referenced_columns: vec![],
                check_expression: None,
            });
        }
        Ok(constraints)
    }

    fn resolve_ref_table(
        &self,
        constraint_name: &str,
    ) -> MetadataResult<String> {
        let sql =
            "SELECT table_name FROM all_constraints \
             WHERE owner = :1 AND constraint_name = :2";

        let row = self
            .conn
            .query_row_as::<(String,)>(
                sql,
                &[&self.schema, &constraint_name],
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to resolve FK ref: {e}"
                ),
            })?;

        Ok(row.0)
    }

    fn query_indexes(
        &self,
        table: &str,
    ) -> MetadataResult<Vec<IndexInfo>> {
        let sql =
            "SELECT i.index_name, i.uniqueness, \
             i.index_type, \
             LISTAGG(ic.column_name, ',') \
               WITHIN GROUP (ORDER BY ic.column_position) \
             FROM all_indexes i \
             JOIN all_ind_columns ic \
               ON i.owner = ic.index_owner \
               AND i.index_name = ic.index_name \
             WHERE i.owner = :1 AND i.table_name = :2 \
             GROUP BY i.index_name, i.uniqueness, \
               i.index_type";

        let rows = self
            .conn
            .query_as::<(String, String, String, String)>(
                sql,
                &[&self.schema, &table.to_uppercase()],
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query indexes for {table}: {e}"
                ),
            })?;

        let mut indexes = Vec::new();
        for row in rows {
            let (name, uniqueness, index_type, cols_str) =
                row.map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read index: {e}"
                    ),
                })?;

            indexes.push(IndexInfo {
                name,
                columns: cols_str
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect(),
                unique: uniqueness == "UNIQUE",
                index_type: index_type.to_lowercase(),
            });
        }
        Ok(indexes)
    }

    fn query_table_stats(
        &self,
        table: &str,
    ) -> MetadataResult<(f64, u64)> {
        let sql =
            "SELECT NVL(num_rows, 0), \
             NVL(blocks, 0) * \
               (SELECT value FROM v$parameter \
                WHERE name = 'db_block_size') \
             FROM all_tables \
             WHERE owner = :1 AND table_name = :2";

        let result = self.conn.query_row_as::<(f64, f64)>(
            sql,
            &[&self.schema, &table.to_uppercase()],
        );

        match result {
            Ok((rows, bytes)) => {
                Ok((rows.max(0.0), bytes.max(0.0) as u64))
            }
            Err(_) => {
                // Fallback: just get num_rows without size.
                let sql2 =
                    "SELECT NVL(num_rows, 0) \
                     FROM all_tables \
                     WHERE owner = :1 \
                     AND table_name = :2";

                let (rows,) = self
                    .conn
                    .query_row_as::<(f64,)>(
                        sql2,
                        &[
                            &self.schema,
                            &table.to_uppercase(),
                        ],
                    )
                    .map_err(|e| {
                        MetadataError::Query {
                            message: format!(
                                "failed to query stats for \
                                 {table}: {e}"
                            ),
                        }
                    })?;

                Ok((rows.max(0.0), 0))
            }
        }
    }

    fn query_column_stats(
        &self,
        table: &str,
    ) -> MetadataResult<HashMap<String, ColumnStatistics>> {
        let sql =
            "SELECT column_name, num_distinct, \
             NVL(num_nulls, 0), avg_col_len \
             FROM all_tab_col_statistics \
             WHERE owner = :1 AND table_name = :2";

        let rows = self
            .conn
            .query_as::<(String, Option<f64>, f64, Option<f64>)>(
                sql,
                &[&self.schema, &table.to_uppercase()],
            )
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to query column stats for \
                     {table}: {e}"
                ),
            })?;

        // Get total rows for null fraction calculation.
        let (total_rows, _) = self.query_table_stats(table)?;

        let mut result = HashMap::new();
        for row in rows {
            let (col_name, ndv, num_nulls, avg_len) =
                row.map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read column stats: {e}"
                    ),
                })?;

            let null_fraction = if total_rows > 0.0 {
                num_nulls / total_rows
            } else {
                0.0
            };

            result.insert(
                col_name.clone(),
                ColumnStatistics {
                    column_name: col_name,
                    distinct_count: ndv.unwrap_or(0.0),
                    null_fraction,
                    avg_width: avg_len,
                    most_common_values: vec![],
                    histogram_bounds: vec![],
                },
            );
        }

        // Query histogram bounds if available.
        let hist_sql =
            "SELECT column_name, endpoint_value \
             FROM all_tab_histograms \
             WHERE owner = :1 AND table_name = :2 \
             AND endpoint_value IS NOT NULL \
             ORDER BY column_name, endpoint_number";

        if let Ok(hist_rows) = self.conn.query_as::<(
            String,
            String,
        )>(
            hist_sql,
            &[&self.schema, &table.to_uppercase()],
        ) {
            for row in hist_rows.flatten() {
                let (col_name, endpoint) = row;
                if let Some(stats) = result.get_mut(&col_name) {
                    stats
                        .histogram_bounds
                        .push(endpoint);
                }
            }
        }

        Ok(result)
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
            let (row_count, _) =
                self.query_table_stats(name)?;

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
            kind: DatabaseKind::Oracle,
            schema_name: self.schema.clone(),
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
            self.query_table_stats(table)?;
        let columns = self.query_column_stats(table)?;

        Ok(TableStats {
            table_name: table.to_owned(),
            row_count,
            total_bytes,
            columns,
        })
    }

    /// Execute EXPLAIN PLAN and parse the result.
    ///
    /// # Errors
    ///
    /// Returns errors if the EXPLAIN query fails.
    pub fn explain_query_mut(
        &self,
        sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        // Delete previous plan.
        self.conn
            .execute(
                "DELETE FROM plan_table \
                 WHERE statement_id = 'RA_EXPLAIN'",
                &[],
            )
            .ok();

        // Generate the plan.
        let explain_sql = format!(
            "EXPLAIN PLAN SET STATEMENT_ID = 'RA_EXPLAIN' \
             FOR {sql}"
        );
        self.conn
            .execute(&explain_sql, &[])
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "EXPLAIN PLAN failed: {e}"
                ),
            })?;

        // Read the plan from PLAN_TABLE.
        let plan_sql =
            "SELECT LPAD(' ', 2 * level) || operation \
               || CASE WHEN options IS NOT NULL \
                  THEN ' (' || options || ')' END \
               || CASE WHEN object_name IS NOT NULL \
                  THEN ' ON ' || object_name END \
               AS plan_line, \
             cost, cardinality, bytes, object_name \
             FROM plan_table \
             WHERE statement_id = 'RA_EXPLAIN' \
             START WITH parent_id IS NULL \
             CONNECT BY PRIOR id = parent_id \
             ORDER SIBLINGS BY id";

        let rows = self
            .conn
            .query_as::<(
                String,
                Option<f64>,
                Option<f64>,
                Option<f64>,
                Option<String>,
            )>(plan_sql, &[])
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to read plan table: {e}"
                ),
            })?;

        let mut lines = Vec::new();
        let mut total_cost = None;
        let mut total_rows = None;

        for row in rows {
            let (line, cost, card, _bytes, _obj) =
                row.map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read plan row: {e}"
                    ),
                })?;

            if total_cost.is_none() {
                total_cost = cost;
                total_rows = card;
            }
            lines.push(line);
        }

        let text = lines.join("\n");
        let first = lines.first().map(String::as_str).unwrap_or("");
        let node_type = classify_oracle_node(first);

        Ok(ExplainPlan {
            root: ExplainNode {
                node_type,
                join_type: None,
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost,
                estimated_rows: total_rows,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail: Some(text),
                children: Vec::new(),
            },
            query: Some(sql.to_owned()),
            total_cost,
            total_rows,
        })
    }
}

impl DatabaseConnector for OracleConnector {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::Oracle
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

/// Parsed Oracle connection URL parts.
struct OracleUrlParts {
    host: String,
    port: u16,
    user: String,
    password: String,
    service: String,
}

/// Parse `oracle://user:pass@host:port/service_name`.
fn parse_oracle_url(
    url: &str,
) -> MetadataResult<OracleUrlParts> {
    let stripped = url
        .strip_prefix("oracle://")
        .ok_or_else(|| MetadataError::Connection {
            message: format!(
                "invalid Oracle URL scheme: {url}"
            ),
        })?;

    let (userinfo, rest) = stripped
        .split_once('@')
        .unwrap_or(("system:", stripped));
    let (user, password) = userinfo
        .split_once(':')
        .unwrap_or((userinfo, ""));
    let (hostport, service) = rest
        .split_once('/')
        .unwrap_or((rest, "ORCL"));
    let (host, port_str) = hostport
        .split_once(':')
        .unwrap_or((hostport, "1521"));

    let port: u16 = port_str.parse().unwrap_or(1521);

    Ok(OracleUrlParts {
        host: host.to_owned(),
        port,
        user: user.to_owned(),
        password: password.to_owned(),
        service: service.to_owned(),
    })
}

fn classify_oracle_node(line: &str) -> NodeType {
    let upper = line.trim().to_uppercase();
    if upper.contains("TABLE ACCESS FULL") {
        NodeType::SeqScan
    } else if upper.contains("INDEX RANGE SCAN")
        || upper.contains("INDEX UNIQUE SCAN")
        || upper.contains("INDEX FULL SCAN")
    {
        NodeType::IndexScan
    } else if upper.contains("HASH JOIN") {
        NodeType::HashJoin
    } else if upper.contains("MERGE JOIN") {
        NodeType::MergeJoin
    } else if upper.contains("NESTED LOOPS") {
        NodeType::NestedLoop
    } else if upper.contains("SORT")
        || upper.contains("ORDER BY")
    {
        NodeType::Sort
    } else if upper.contains("GROUP BY")
        || upper.contains("HASH GROUP")
    {
        NodeType::HashAggregate
    } else if upper.contains("FILTER") {
        NodeType::Other
    } else {
        NodeType::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_basic() {
        let parts = parse_oracle_url(
            "oracle://scott:tiger@dbhost:1521/ORCL",
        )
        .expect("should parse");
        assert_eq!(parts.host, "dbhost");
        assert_eq!(parts.port, 1521);
        assert_eq!(parts.user, "scott");
        assert_eq!(parts.password, "tiger");
        assert_eq!(parts.service, "ORCL");
    }

    #[test]
    fn parse_url_defaults() {
        let parts = parse_oracle_url(
            "oracle://system:@localhost",
        )
        .expect("should parse");
        assert_eq!(parts.port, 1521);
        assert_eq!(parts.service, "ORCL");
    }

    #[test]
    fn parse_url_invalid() {
        assert!(
            parse_oracle_url("postgresql://localhost")
                .is_err()
        );
    }

    #[test]
    fn classify_nodes() {
        assert_eq!(
            classify_oracle_node("TABLE ACCESS FULL"),
            NodeType::SeqScan
        );
        assert_eq!(
            classify_oracle_node("INDEX RANGE SCAN"),
            NodeType::IndexScan
        );
        assert_eq!(
            classify_oracle_node("HASH JOIN"),
            NodeType::HashJoin
        );
        assert_eq!(
            classify_oracle_node("MERGE JOIN"),
            NodeType::MergeJoin
        );
        assert_eq!(
            classify_oracle_node("NESTED LOOPS"),
            NodeType::NestedLoop
        );
        assert_eq!(
            classify_oracle_node("SORT ORDER BY"),
            NodeType::Sort
        );
        assert_eq!(
            classify_oracle_node("HASH GROUP BY"),
            NodeType::HashAggregate
        );
        assert_eq!(
            classify_oracle_node("SELECT STATEMENT"),
            NodeType::Other
        );
    }

    #[test]
    fn connector_kind() {
        assert_eq!(
            DatabaseKind::Oracle.to_string(),
            "Oracle"
        );
    }
}
