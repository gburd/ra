//! Microsoft SQL Server database connector.
//!
//! Queries SQL Server system views (`sys.tables`, `sys.columns`,
//! `sys.stats`, `sys.indexes`) and parses `SET SHOWPLAN_TEXT ON`
//! output.
//!
//! This connector uses the `tiberius` async TDS driver. All public
//! methods block on an internal tokio runtime so the synchronous
//! `DatabaseConnector` trait can be implemented.

use std::collections::HashMap;

use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, Config, Query, Row};
use tokio::net::TcpStream;
use tokio::runtime::Runtime;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::connector::{DatabaseConnector, MetadataResult};
use crate::error::MetadataError;
use crate::explain::{
    ExplainNode, ExplainPlan, NodeType,
};
use crate::schema::{
    ColumnInfo, ConstraintInfo, ConstraintKind,
    DatabaseKind, IndexInfo, SchemaInfo, TableInfo,
    TableStats,
};

/// SQL Server connector using the `tiberius` crate.
pub struct SqlServerConnector {
    client: Client<Compat<TcpStream>>,
    rt: Runtime,
    database: String,
}

impl SqlServerConnector {
    /// Connect to a SQL Server instance.
    ///
    /// `connection_string` should be in the format:
    /// `sqlserver://user:pass@host:port/database`
    ///
    /// # Errors
    ///
    /// Returns `MetadataError::Connection` on failure.
    pub fn connect(
        connection_string: &str,
    ) -> MetadataResult<Self> {
        let rt = Runtime::new().map_err(|e| {
            MetadataError::Connection {
                message: format!(
                    "failed to create async runtime: {e}"
                ),
            }
        })?;

        let (client, database) = rt.block_on(
            Self::async_connect(connection_string),
        )?;

        Ok(Self {
            client,
            rt,
            database,
        })
    }

    async fn async_connect(
        connection_string: &str,
    ) -> MetadataResult<(Client<Compat<TcpStream>>, String)>
    {
        let parts =
            parse_sqlserver_url(connection_string)?;

        let mut config = Config::new();
        config.host(&parts.host);
        config.port(parts.port);
        config.authentication(AuthMethod::sql_server(
            &parts.user,
            &parts.password,
        ));
        config.database(&parts.database);
        config.trust_cert();

        let tcp = TcpStream::connect(config.get_addr())
            .await
            .map_err(|e| MetadataError::Connection {
                message: format!(
                    "SQL Server TCP connect failed: {e}"
                ),
            })?;

        tcp.set_nodelay(true).ok();

        let client =
            Client::connect(config, tcp.compat_write())
                .await
                .map_err(|e| MetadataError::Connection {
                    message: format!(
                        "SQL Server TDS connect failed: {e}"
                    ),
                })?;

        Ok((client, parts.database))
    }

    fn query_tables(
        &mut self,
    ) -> MetadataResult<Vec<String>> {
        self.rt.block_on(async {
            let stream = Query::new(
                "SELECT t.name \
                 FROM sys.tables t \
                 WHERE t.is_ms_shipped = 0 \
                 ORDER BY t.name",
            )
            .query(&mut self.client)
            .await
            .map_err(|e| MetadataError::Query {
                message: format!(
                    "failed to list tables: {e}"
                ),
            })?;

            let rows: Vec<Row> = stream
                .into_row_stream()
                .try_collect()
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read tables: {e}"
                    ),
                })?;

            let mut tables = Vec::new();
            for row in &rows {
                if let Some(name) =
                    row.try_get::<&str, _>(0)
                        .ok()
                        .flatten()
                {
                    tables.push(name.to_owned());
                }
            }
            Ok(tables)
        })
    }

    fn query_columns(
        &mut self,
        table: &str,
    ) -> MetadataResult<Vec<ColumnInfo>> {
        self.rt.block_on(async {
            let sql = format!(
                "SELECT c.name, t.name AS type_name, \
                 c.is_nullable, c.column_id, \
                 dc.definition \
                 FROM sys.columns c \
                 JOIN sys.types t \
                   ON c.user_type_id = t.user_type_id \
                 LEFT JOIN sys.default_constraints dc \
                   ON c.default_object_id = dc.object_id \
                 WHERE c.object_id = OBJECT_ID('{table}') \
                 ORDER BY c.column_id"
            );

            let stream = Query::new(&sql)
                .query(&mut self.client)
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to query columns for \
                         {table}: {e}"
                    ),
                })?;

            let rows: Vec<Row> = stream
                .into_row_stream()
                .try_collect()
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read columns: {e}"
                    ),
                })?;

            let mut columns = Vec::new();
            for row in &rows {
                let name: &str = row
                    .try_get(0)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let data_type: &str = row
                    .try_get(1)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let nullable: bool = row
                    .try_get(2)
                    .ok()
                    .flatten()
                    .unwrap_or(true);
                let ordinal: i32 = row
                    .try_get(3)
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                let default_value: Option<&str> =
                    row.try_get(4).ok().flatten();

                columns.push(ColumnInfo {
                    name: name.to_owned(),
                    data_type: data_type.to_owned(),
                    nullable,
                    ordinal: ordinal as u32,
                    default_value: default_value
                        .map(str::to_owned),
                });
            }
            Ok(columns)
        })
    }

    fn query_constraints(
        &mut self,
        table: &str,
    ) -> MetadataResult<Vec<ConstraintInfo>> {
        self.rt.block_on(async {
            let sql = format!(
                "SELECT kc.name, kc.type_desc, \
                 STRING_AGG(col.name, ',') \
                   WITHIN GROUP \
                   (ORDER BY ic.key_ordinal) \
                   AS columns \
                 FROM sys.key_constraints kc \
                 JOIN sys.index_columns ic \
                   ON kc.parent_object_id = ic.object_id \
                   AND kc.unique_index_id = ic.index_id \
                 JOIN sys.columns col \
                   ON ic.object_id = col.object_id \
                   AND ic.column_id = col.column_id \
                 WHERE kc.parent_object_id = \
                   OBJECT_ID('{table}') \
                 GROUP BY kc.name, kc.type_desc"
            );

            let stream = Query::new(&sql)
                .query(&mut self.client)
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to query constraints \
                         for {table}: {e}"
                    ),
                })?;

            let rows: Vec<Row> = stream
                .into_row_stream()
                .try_collect()
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read constraints: {e}"
                    ),
                })?;

            let mut constraints = Vec::new();
            for row in &rows {
                let name: &str = row
                    .try_get(0)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let type_desc: &str = row
                    .try_get(1)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let cols_str: &str = row
                    .try_get(2)
                    .ok()
                    .flatten()
                    .unwrap_or("");

                let kind = match type_desc {
                    "PRIMARY_KEY_CONSTRAINT" => {
                        ConstraintKind::PrimaryKey
                    }
                    "UNIQUE_CONSTRAINT" => {
                        ConstraintKind::Unique
                    }
                    _ => continue,
                };

                let columns: Vec<String> = cols_str
                    .split(',')
                    .map(|s: &str| {
                        s.trim().to_owned()
                    })
                    .filter(|s: &String| !s.is_empty())
                    .collect();

                constraints.push(ConstraintInfo {
                    name: name.to_owned(),
                    kind,
                    columns,
                    referenced_table: None,
                    referenced_columns: vec![],
                    check_expression: None,
                });
            }

            // Also query foreign keys.
            let fk_sql = format!(
                "SELECT fk.name, \
                 STRING_AGG(\
                   COL_NAME(fkc.parent_object_id, \
                   fkc.parent_column_id), ',') \
                   WITHIN GROUP \
                   (ORDER BY fkc.constraint_column_id), \
                 OBJECT_NAME(\
                   fk.referenced_object_id), \
                 STRING_AGG(\
                   COL_NAME(fkc.referenced_object_id, \
                   fkc.referenced_column_id), ',') \
                   WITHIN GROUP \
                   (ORDER BY fkc.constraint_column_id) \
                 FROM sys.foreign_keys fk \
                 JOIN sys.foreign_key_columns fkc \
                   ON fk.object_id = \
                      fkc.constraint_object_id \
                 WHERE fk.parent_object_id = \
                   OBJECT_ID('{table}') \
                 GROUP BY fk.name, \
                   fk.referenced_object_id"
            );

            let fk_stream = Query::new(&fk_sql)
                .query(&mut self.client)
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to query FKs for \
                         {table}: {e}"
                    ),
                })?;

            let fk_rows: Vec<Row> = fk_stream
                .into_row_stream()
                .try_collect()
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read FKs: {e}"
                    ),
                })?;

            for row in &fk_rows {
                let name: &str = row
                    .try_get(0)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let cols_str: &str = row
                    .try_get(1)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let ref_tbl: &str = row
                    .try_get(2)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let ref_cols_str: &str = row
                    .try_get(3)
                    .ok()
                    .flatten()
                    .unwrap_or("");

                constraints.push(ConstraintInfo {
                    name: name.to_owned(),
                    kind: ConstraintKind::ForeignKey,
                    columns: cols_str
                        .split(',')
                        .map(|s: &str| {
                            s.trim().to_owned()
                        })
                        .filter(|s: &String| {
                            !s.is_empty()
                        })
                        .collect(),
                    referenced_table: Some(
                        ref_tbl.to_owned(),
                    ),
                    referenced_columns: ref_cols_str
                        .split(',')
                        .map(|s: &str| {
                            s.trim().to_owned()
                        })
                        .filter(|s: &String| {
                            !s.is_empty()
                        })
                        .collect(),
                    check_expression: None,
                });
            }

            Ok(constraints)
        })
    }

    fn query_indexes(
        &mut self,
        table: &str,
    ) -> MetadataResult<Vec<IndexInfo>> {
        self.rt.block_on(async {
            let sql = format!(
                "SELECT i.name, i.is_unique, \
                 i.type_desc, \
                 STRING_AGG(col.name, ',') \
                   WITHIN GROUP \
                   (ORDER BY ic.key_ordinal) \
                 FROM sys.indexes i \
                 JOIN sys.index_columns ic \
                   ON i.object_id = ic.object_id \
                   AND i.index_id = ic.index_id \
                 JOIN sys.columns col \
                   ON ic.object_id = col.object_id \
                   AND ic.column_id = col.column_id \
                 WHERE i.object_id = \
                   OBJECT_ID('{table}') \
                 AND i.name IS NOT NULL \
                 AND ic.is_included_column = 0 \
                 GROUP BY i.name, i.is_unique, \
                   i.type_desc"
            );

            let stream = Query::new(&sql)
                .query(&mut self.client)
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to query indexes for \
                         {table}: {e}"
                    ),
                })?;

            let rows: Vec<Row> = stream
                .into_row_stream()
                .try_collect()
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read indexes: {e}"
                    ),
                })?;

            let mut indexes = Vec::new();
            for row in &rows {
                let name: &str = row
                    .try_get(0)
                    .ok()
                    .flatten()
                    .unwrap_or("");
                let unique: bool = row
                    .try_get(1)
                    .ok()
                    .flatten()
                    .unwrap_or(false);
                let type_desc: &str = row
                    .try_get(2)
                    .ok()
                    .flatten()
                    .unwrap_or("NONCLUSTERED");
                let cols_str: &str = row
                    .try_get(3)
                    .ok()
                    .flatten()
                    .unwrap_or("");

                let index_type = match type_desc {
                    "CLUSTERED" => "clustered",
                    "NONCLUSTERED" => "nonclustered",
                    "CLUSTERED COLUMNSTORE" => {
                        "columnstore_clustered"
                    }
                    "NONCLUSTERED COLUMNSTORE" => {
                        "columnstore_nonclustered"
                    }
                    other => other,
                };

                indexes.push(IndexInfo {
                    name: name.to_owned(),
                    columns: cols_str
                        .split(',')
                        .map(|s: &str| {
                            s.trim().to_owned()
                        })
                        .filter(|s: &String| {
                            !s.is_empty()
                        })
                        .collect(),
                    unique,
                    index_type: index_type.to_owned(),
                });
            }

            Ok(indexes)
        })
    }

    fn query_table_stats(
        &mut self,
        table: &str,
    ) -> MetadataResult<(f64, u64)> {
        self.rt.block_on(async {
            let sql = format!(
                "SELECT \
                 SUM(p.rows) AS row_count, \
                 SUM(a.total_pages) * 8 * 1024 \
                   AS total_bytes \
                 FROM sys.tables t \
                 JOIN sys.partitions p \
                   ON t.object_id = p.object_id \
                 JOIN sys.allocation_units a \
                   ON p.partition_id = a.container_id \
                 WHERE t.object_id = \
                   OBJECT_ID('{table}') \
                 AND p.index_id IN (0, 1)"
            );

            let stream = Query::new(&sql)
                .query(&mut self.client)
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to query stats for \
                         {table}: {e}"
                    ),
                })?;

            let rows: Vec<Row> = stream
                .into_row_stream()
                .try_collect()
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read stats: {e}"
                    ),
                })?;

            if let Some(row) = rows.first() {
                let row_count: i64 = row
                    .try_get(0)
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                let bytes: i64 = row
                    .try_get(1)
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                Ok((
                    row_count.max(0) as f64,
                    bytes.max(0) as u64,
                ))
            } else {
                Err(MetadataError::Query {
                    message: format!(
                        "table not found: {table}"
                    ),
                })
            }
        })
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
            let constraints =
                self.query_constraints(name)?;
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
            kind: DatabaseKind::SqlServer,
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
            self.query_table_stats(table)?;

        Ok(TableStats {
            table_name: table.to_owned(),
            row_count,
            total_bytes,
            columns: HashMap::new(),
        })
    }

    /// Execute `SET SHOWPLAN_TEXT ON` and parse the
    /// result.
    ///
    /// # Errors
    ///
    /// Returns errors if the EXPLAIN query fails.
    pub fn explain_query_mut(
        &mut self,
        sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        self.rt.block_on(async {
            // Enable showplan
            Query::new("SET SHOWPLAN_TEXT ON")
                .execute(&mut self.client)
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "SET SHOWPLAN_TEXT ON failed: {e}"
                    ),
                })?;

            let stream = Query::new(sql)
                .query(&mut self.client)
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "SHOWPLAN query failed: {e}"
                    ),
                })?;

            let rows: Vec<Row> = stream
                .into_row_stream()
                .try_collect()
                .await
                .map_err(|e| MetadataError::Query {
                    message: format!(
                        "failed to read showplan: {e}"
                    ),
                })?;

            let mut lines = Vec::new();
            for row in &rows {
                if let Some(text) =
                    row.try_get::<&str, _>(0)
                        .ok()
                        .flatten()
                {
                    lines.push(text.to_owned());
                }
            }

            // Disable showplan
            Query::new("SET SHOWPLAN_TEXT OFF")
                .execute(&mut self.client)
                .await
                .ok();

            let text = lines.join("\n");
            parse_sqlserver_explain(&text)
        })
    }
}

impl DatabaseConnector for SqlServerConnector {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::SqlServer
    }

    fn gather_schema(
        &self,
    ) -> MetadataResult<SchemaInfo> {
        Err(MetadataError::Unsupported {
            message: "use gather_schema_mut() instead"
                .to_owned(),
        })
    }

    fn gather_statistics(
        &self,
        _table: &str,
    ) -> MetadataResult<TableStats> {
        Err(MetadataError::Unsupported {
            message:
                "use gather_statistics_mut() instead"
                    .to_owned(),
        })
    }

    fn explain_query(
        &self,
        _sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        Err(MetadataError::Unsupported {
            message:
                "use explain_query_mut() instead"
                    .to_owned(),
        })
    }
}

/// Parsed connection URL parts.
struct UrlParts {
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

/// Parse `sqlserver://user:pass@host:port/database`.
fn parse_sqlserver_url(
    url: &str,
) -> MetadataResult<UrlParts> {
    let stripped = url
        .strip_prefix("sqlserver://")
        .or_else(|| url.strip_prefix("mssql://"))
        .ok_or_else(|| MetadataError::Connection {
            message: format!(
                "invalid SQL Server URL scheme: {url}"
            ),
        })?;

    let (userinfo, rest) = stripped
        .split_once('@')
        .unwrap_or(("sa:", stripped));
    let (user, password) = userinfo
        .split_once(':')
        .unwrap_or((userinfo, ""));
    let (hostport, database) = rest
        .split_once('/')
        .unwrap_or((rest, "master"));
    let (host, port_str) = hostport
        .split_once(':')
        .unwrap_or((hostport, "1433"));

    let port: u16 = port_str.parse().unwrap_or(1433);

    Ok(UrlParts {
        host: host.to_owned(),
        port,
        user: user.to_owned(),
        password: password.to_owned(),
        database: database.to_owned(),
    })
}

/// Parse SQL Server SHOWPLAN_TEXT output into an
/// `ExplainPlan`.
fn parse_sqlserver_explain(
    text: &str,
) -> MetadataResult<ExplainPlan> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    let first = lines.first().copied().unwrap_or("");
    let node_type = classify_sqlserver_node(first);

    Ok(ExplainPlan {
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
    })
}

fn classify_sqlserver_node(line: &str) -> NodeType {
    let upper = line.to_uppercase();
    if upper.contains("TABLE SCAN") {
        NodeType::SeqScan
    } else if upper.contains("CLUSTERED INDEX SCAN") {
        NodeType::SeqScan
    } else if upper.contains("INDEX SEEK")
        || upper.contains("INDEX SCAN")
    {
        NodeType::IndexScan
    } else if upper.contains("HASH MATCH") {
        NodeType::HashJoin
    } else if upper.contains("MERGE JOIN") {
        NodeType::MergeJoin
    } else if upper.contains("NESTED LOOPS") {
        NodeType::NestedLoop
    } else if upper.contains("STREAM AGGREGATE")
        || upper.contains("HASH AGGREGATE")
    {
        NodeType::HashAggregate
    } else if upper.contains("SORT") {
        NodeType::Sort
    } else {
        NodeType::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_basic() {
        let parts = parse_sqlserver_url(
            "sqlserver://sa:pass@localhost:1433/mydb",
        )
        .expect("should parse");
        assert_eq!(parts.host, "localhost");
        assert_eq!(parts.port, 1433);
        assert_eq!(parts.user, "sa");
        assert_eq!(parts.password, "pass");
        assert_eq!(parts.database, "mydb");
    }

    #[test]
    fn parse_url_defaults() {
        let parts = parse_sqlserver_url(
            "sqlserver://sa:@localhost",
        )
        .expect("should parse");
        assert_eq!(parts.port, 1433);
        assert_eq!(parts.database, "master");
    }

    #[test]
    fn parse_url_mssql_scheme() {
        let parts = parse_sqlserver_url(
            "mssql://user:pass@host/db",
        )
        .expect("should parse");
        assert_eq!(parts.host, "host");
        assert_eq!(parts.database, "db");
    }

    #[test]
    fn parse_url_invalid() {
        assert!(
            parse_sqlserver_url("postgresql://localhost")
                .is_err()
        );
    }

    #[test]
    fn classify_nodes() {
        assert_eq!(
            classify_sqlserver_node("Table Scan"),
            NodeType::SeqScan
        );
        assert_eq!(
            classify_sqlserver_node("Index Seek"),
            NodeType::IndexScan
        );
        assert_eq!(
            classify_sqlserver_node("Hash Match"),
            NodeType::HashJoin
        );
        assert_eq!(
            classify_sqlserver_node("Nested Loops"),
            NodeType::NestedLoop
        );
        assert_eq!(
            classify_sqlserver_node("Sort"),
            NodeType::Sort
        );
        assert_eq!(
            classify_sqlserver_node("Something"),
            NodeType::Other
        );
    }

    #[test]
    fn parse_explain_basic() {
        let text = "  |--Table Scan [orders]";
        let plan = parse_sqlserver_explain(text)
            .expect("should parse");
        assert_eq!(
            plan.root.node_type,
            NodeType::SeqScan
        );
    }

    #[test]
    fn connector_kind() {
        assert_eq!(
            DatabaseKind::SqlServer.to_string(),
            "SQL Server"
        );
    }
}
