//! `MonetDB` database connector.
//!
//! Queries `MonetDB` system catalog (`sys.tables`, `sys.columns`,
//! `sys.statistics`, `sys.keys`) via ODBC and parses `EXPLAIN`
//! output.
//!
//! This connector uses the `odbc-api` crate for ODBC connectivity.

use std::collections::HashMap;

use odbc_api::{Connection, ConnectionOptions, Cursor, Environment, IntoParameter};

use crate::connector::{DatabaseConnector, MetadataResult};
use crate::error::MetadataError;
use crate::explain::{ExplainNode, ExplainPlan, NodeType};
use crate::schema::{
    ColumnInfo, ConstraintInfo, ConstraintKind, DatabaseKind, SchemaInfo, TableInfo, TableStats,
};

/// `MonetDB` connector using the `odbc-api` crate.
pub struct MonetDBConnector {
    env: Environment,
    connection_string: String,
    schema: String,
}

impl MonetDBConnector {
    /// Connect to a `MonetDB` database via ODBC.
    ///
    /// `dsn` should be an ODBC connection string such as:
    /// `monetdb://user:pass@host:50000/database` or a raw
    /// ODBC DSN string.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` on failure.
    pub fn connect(dsn: &str) -> MetadataResult<Self> {
        let env = Environment::new().map_err(|e| MetadataError::Connection {
            message: format!("ODBC environment init failed: {e}"),
        })?;

        let connection_string = if dsn.starts_with("monetdb://") {
            let parts = parse_monetdb_url(dsn)?;
            format!(
                "DRIVER={{MonetDB}};HOST={};PORT={};\
                 DATABASE={};UID={};PWD={}",
                parts.host, parts.port, parts.database, parts.user, parts.password,
            )
        } else {
            dsn.to_owned()
        };

        // Validate by opening a connection.
        env.connect_with_connection_string(&connection_string, ConnectionOptions::default())
            .map_err(|e| MetadataError::Connection {
                message: format!("MonetDB ODBC connect failed: {e}"),
            })?;

        Ok(Self {
            env,
            connection_string,
            schema: "sys".to_owned(),
        })
    }

    /// Set the schema to query (defaults to "sys").
    pub fn set_schema(&mut self, schema: &str) {
        schema.clone_into(&mut self.schema);
    }

    fn open_conn(&self) -> MetadataResult<Connection<'_>> {
        self.env
            .connect_with_connection_string(&self.connection_string, ConnectionOptions::default())
            .map_err(|e| MetadataError::Connection {
                message: format!("MonetDB connection failed: {e}"),
            })
    }

    fn query_tables(&self) -> MetadataResult<Vec<String>> {
        let conn = self.open_conn()?;

        let sql = "SELECT name FROM sys.tables \
             WHERE schema_id = (SELECT id FROM sys.schemas \
               WHERE name = ?) \
             AND type = 0 \
             ORDER BY name";

        let mut cursor = conn
            .execute(sql, &self.schema.as_str().into_parameter())
            .map_err(|e| MetadataError::Query {
                message: format!("failed to list tables: {e}"),
            })?
            .ok_or_else(|| MetadataError::Query {
                message: "no result set from table query".to_owned(),
            })?;

        let mut tables = Vec::new();
        let mut buf = Vec::new();
        while let Some(mut row) = cursor.next_row().map_err(|e| MetadataError::Query {
            message: format!("failed to read tables: {e}"),
        })? {
            buf.clear();
            if row.get_text(1, &mut buf).ok() == Some(true) {
                tables.push(String::from_utf8_lossy(&buf).to_string());
            }
            buf.clear();
        }
        Ok(tables)
    }

    fn query_columns(&self, table: &str) -> MetadataResult<Vec<ColumnInfo>> {
        let conn = self.open_conn()?;

        let sql = "SELECT c.name, c.type, c.\"null\", c.number, \
             c.\"default\" \
             FROM sys.columns c \
             JOIN sys.tables t ON c.table_id = t.id \
             JOIN sys.schemas s ON t.schema_id = s.id \
             WHERE s.name = ? AND t.name = ? \
             ORDER BY c.number";

        let mut cursor = conn
            .execute(
                sql,
                (
                    &self.schema.as_str().into_parameter(),
                    &table.into_parameter(),
                ),
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query columns for {table}: {e}"),
            })?
            .ok_or_else(|| MetadataError::Query {
                message: format!("no result set for columns of {table}"),
            })?;

        let mut columns = Vec::new();
        let mut buf = Vec::new();
        while let Some(mut row) = cursor.next_row().map_err(|e| MetadataError::Query {
            message: format!("failed to read columns: {e}"),
        })? {
            buf.clear();
            buf.clear();
            let name = if row.get_text(1, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            buf.clear();
            buf.clear();
            let data_type = if row.get_text(2, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            buf.clear();
            buf.clear();
            let nullable_str = if row.get_text(3, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            buf.clear();
            buf.clear();
            let number_str = if row.get_text(4, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            buf.clear();
            buf.clear();
            let default_value = if row.get_text(5, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            };

            let ordinal: u32 = number_str.parse().unwrap_or(0);

            columns.push(ColumnInfo {
                name,
                data_type,
                nullable: nullable_str == "true" || nullable_str == "1",
                ordinal,
                default_value,
            });
        }
        Ok(columns)
    }

    fn query_constraints(&self, table: &str) -> MetadataResult<Vec<ConstraintInfo>> {
        let conn = self.open_conn()?;

        let sql = "SELECT k.name, k.type, \
             GROUP_CONCAT(kc.name) \
             FROM sys.keys k \
             JOIN sys.objects kc ON k.id = kc.id \
             JOIN sys.tables t ON k.table_id = t.id \
             JOIN sys.schemas s ON t.schema_id = s.id \
             WHERE s.name = ? AND t.name = ? \
             GROUP BY k.name, k.type";

        let result = conn
            .execute(
                sql,
                (
                    &self.schema.as_str().into_parameter(),
                    &table.into_parameter(),
                ),
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query keys for {table}: {e}"),
            })?;

        let Some(mut cursor) = result else {
            return Ok(vec![]);
        };

        let mut constraints = Vec::new();
        let mut buf = Vec::new();
        while let Some(mut row) = cursor.next_row().map_err(|e| MetadataError::Query {
            message: format!("failed to read keys: {e}"),
        })? {
            buf.clear();
            buf.clear();
            let name = if row.get_text(1, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            buf.clear();
            buf.clear();
            let key_type = if row.get_text(2, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            buf.clear();
            buf.clear();
            let cols_str = if row.get_text(3, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            // MonetDB key types: 0=pkey, 1=unique, 2=fkey
            let kind = match key_type.as_str() {
                "0" => ConstraintKind::PrimaryKey,
                "1" => ConstraintKind::Unique,
                "2" => ConstraintKind::ForeignKey,
                _ => continue,
            };

            let columns: Vec<String> = cols_str
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();

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

    fn query_row_count(&self, table: &str) -> MetadataResult<(f64, u64)> {
        let conn = self.open_conn()?;

        // MonetDB stores row count in sys.storage.
        let sql = "SELECT SUM(count), SUM(columnsize) \
             FROM sys.storage() \
             WHERE schema = ? AND table = ? \
             GROUP BY schema, \"table\"";

        let result = conn
            .execute(
                sql,
                (
                    &self.schema.as_str().into_parameter(),
                    &table.into_parameter(),
                ),
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query stats for {table}: {e}"),
            })?;

        let Some(mut cursor) = result else {
            return Ok((0.0, 0));
        };

        let mut buf = Vec::new();
        if let Some(mut row) = cursor.next_row().map_err(|e| MetadataError::Query {
            message: format!("failed to read stats: {e}"),
        })? {
            buf.clear();
            buf.clear();
            let count_str = if row.get_text(1, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            buf.clear();
            buf.clear();
            let size_str = if row.get_text(2, &mut buf).ok() == Some(true) {
                Some(String::from_utf8_lossy(&buf).to_string())
            } else {
                None
            }
            .unwrap_or_default();

            let rows: f64 = count_str.parse().unwrap_or(0.0);
            let bytes: u64 = size_str.parse().unwrap_or(0);

            Ok((rows, bytes))
        } else {
            Ok((0.0, 0))
        }
    }

    /// Gather full schema information.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_schema_mut(&self) -> MetadataResult<SchemaInfo> {
        let table_names = self.query_tables()?;
        let mut tables = HashMap::new();

        for name in &table_names {
            let columns = self.query_columns(name)?;
            let constraints = self.query_constraints(name)?;
            let (row_count, _) = self.query_row_count(name)?;

            tables.insert(
                name.clone(),
                TableInfo {
                    name: name.clone(),
                    columns,
                    constraints,
                    indexes: vec![],
                    triggers: vec![],
                    estimated_rows: Some(row_count),
                },
            );
        }

        Ok(SchemaInfo {
            kind: DatabaseKind::MonetDB,
            schema_name: self.schema.clone(),
            tables,
        })
    }

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns errors if catalog queries fail.
    pub fn gather_statistics_mut(&self, table: &str) -> MetadataResult<TableStats> {
        let (row_count, total_bytes) = self.query_row_count(table)?;

        Ok(TableStats {
            table_name: table.to_owned(),
            row_count,
            total_bytes,
            columns: HashMap::new(),
        })
    }

    /// Execute EXPLAIN on a query and parse the result.
    ///
    /// # Errors
    ///
    /// Returns errors if the EXPLAIN query fails.
    pub fn explain_query_mut(&self, sql: &str) -> MetadataResult<ExplainPlan> {
        let conn = self.open_conn()?;

        let explain_sql = format!("EXPLAIN {sql}");
        let result = conn
            .execute(&explain_sql, ())
            .map_err(|e| MetadataError::Query {
                message: format!("EXPLAIN failed: {e}"),
            })?;

        let Some(mut cursor) = result else {
            return Err(MetadataError::ExplainParse {
                message: "no EXPLAIN output".to_owned(),
            });
        };

        let mut lines = Vec::new();
        let mut buf = Vec::new();
        while let Some(mut row) = cursor.next_row().map_err(|e| MetadataError::Query {
            message: format!("failed to read EXPLAIN: {e}"),
        })? {
            buf.clear();
            buf.clear();
            if row.get_text(1, &mut buf).ok() == Some(true) {
                lines.push(String::from_utf8_lossy(&buf).to_string());
            }
        }

        let text = lines.join("\n");
        Ok(parse_monetdb_explain(&text))
    }
}

impl DatabaseConnector for MonetDBConnector {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::MonetDB
    }

    fn gather_schema(&self) -> MetadataResult<SchemaInfo> {
        self.gather_schema_mut()
    }

    fn gather_statistics(&self, table: &str) -> MetadataResult<TableStats> {
        self.gather_statistics_mut(table)
    }

    fn explain_query(&self, sql: &str) -> MetadataResult<ExplainPlan> {
        self.explain_query_mut(sql)
    }
}

/// Parsed `MonetDB` connection URL parts.
struct MonetDBUrlParts {
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

/// Parse `monetdb://user:pass@host:port/database`.
fn parse_monetdb_url(url: &str) -> MetadataResult<MonetDBUrlParts> {
    let stripped = url
        .strip_prefix("monetdb://")
        .ok_or_else(|| MetadataError::Connection {
            message: format!("invalid MonetDB URL scheme: {url}"),
        })?;

    let (userinfo, rest) = stripped
        .split_once('@')
        .unwrap_or(("monetdb:monetdb", stripped));
    let (user, password) = userinfo.split_once(':').unwrap_or((userinfo, "monetdb"));
    let (hostport, database) = rest.split_once('/').unwrap_or((rest, "demo"));
    let (host, port_str) = hostport.split_once(':').unwrap_or((hostport, "50000"));

    let port: u16 = port_str.parse().unwrap_or(50000);

    Ok(MonetDBUrlParts {
        host: host.to_owned(),
        port,
        user: user.to_owned(),
        password: password.to_owned(),
        database: database.to_owned(),
    })
}

/// Parse `MonetDB` MAL-based EXPLAIN output into an `ExplainPlan`.
fn parse_monetdb_explain(text: &str) -> ExplainPlan {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    let first = lines.first().copied().unwrap_or("");
    let node_type = classify_monetdb_node(first);

    ExplainPlan {
        root: ExplainNode {
            node_type,
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
    }
}

fn classify_monetdb_node(line: &str) -> NodeType {
    let lower = line.to_lowercase();
    if lower.contains("table") && lower.contains("scan") {
        NodeType::SeqScan
    } else if lower.contains("index") {
        NodeType::IndexScan
    } else if lower.contains("join") {
        NodeType::HashJoin
    } else if lower.contains("group") {
        NodeType::HashAggregate
    } else if lower.contains("sort") || lower.contains("order") {
        NodeType::Sort
    } else {
        NodeType::Other
    }
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_basic() {
        let parts = parse_monetdb_url("monetdb://monetdb:monetdb@localhost:50000/demo")
            .expect("should parse");
        assert_eq!(parts.host, "localhost");
        assert_eq!(parts.port, 50000);
        assert_eq!(parts.user, "monetdb");
        assert_eq!(parts.password, "monetdb");
        assert_eq!(parts.database, "demo");
    }

    #[test]
    fn parse_url_defaults() {
        let parts = parse_monetdb_url("monetdb://user:pass@host").expect("should parse");
        assert_eq!(parts.port, 50000);
        assert_eq!(parts.database, "demo");
    }

    #[test]
    fn parse_url_invalid() {
        assert!(parse_monetdb_url("postgresql://localhost").is_err());
    }

    #[test]
    fn classify_nodes() {
        assert_eq!(classify_monetdb_node("table.scan"), NodeType::SeqScan);
        assert_eq!(classify_monetdb_node("index.lookup"), NodeType::IndexScan);
        assert_eq!(classify_monetdb_node("join operation"), NodeType::HashJoin);
        assert_eq!(classify_monetdb_node("group by"), NodeType::HashAggregate);
        assert_eq!(classify_monetdb_node("sort"), NodeType::Sort);
        assert_eq!(classify_monetdb_node("something"), NodeType::Other);
    }

    #[test]
    fn parse_explain_basic() {
        let text = "table.scan on orders\nfilter";
        let plan = parse_monetdb_explain(text);
        assert_eq!(plan.root.node_type, NodeType::SeqScan);
    }

    #[test]
    fn connector_kind() {
        assert_eq!(DatabaseKind::MonetDB.to_string(), "MonetDB");
    }
}
