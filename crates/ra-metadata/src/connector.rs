//! Database connector trait for gathering metadata from live databases.
//!
//! The [`DatabaseConnector`] trait provides a uniform interface for
//! gathering schema metadata, table statistics, and EXPLAIN plans
//! from `PostgreSQL`, `MySQL`, and `SQLite` databases.

use crate::error::MetadataError;
use crate::explain::ExplainPlan;
use crate::schema::{DatabaseKind, SchemaInfo, TableStats};

/// Result type for metadata operations.
pub type MetadataResult<T> = Result<T, MetadataError>;

/// Trait for connecting to a database and retrieving metadata.
///
/// Each backend (`PostgreSQL`, `MySQL`, `SQLite`) provides an
/// implementation that queries system catalogs or PRAGMA commands.
pub trait DatabaseConnector {
    /// Return the kind of database this connector targets.
    fn kind(&self) -> DatabaseKind;

    /// Gather full schema information (tables, columns, constraints,
    /// indexes).
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if schema queries fail.
    fn gather_schema(&self) -> MetadataResult<SchemaInfo>;

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if statistics queries fail.
    fn gather_statistics(&self, table: &str) -> MetadataResult<TableStats>;

    /// Execute EXPLAIN on a SQL query and parse the result.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if the EXPLAIN query or parsing fails.
    fn explain_query(&self, sql: &str) -> MetadataResult<ExplainPlan>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, TableInfo};
    use std::collections::HashMap;

    /// A mock connector for testing.
    struct MockConnector {
        kind: DatabaseKind,
        schema: SchemaInfo,
    }

    impl MockConnector {
        fn new_sqlite() -> Self {
            let mut tables = HashMap::new();
            tables.insert(
                "users".to_owned(),
                TableInfo {
                    name: "users".to_owned(),
                    columns: vec![
                        ColumnInfo {
                            name: "id".to_owned(),
                            data_type: "INTEGER".to_owned(),
                            nullable: false,
                            ordinal: 1,
                            default_value: None,
                        },
                        ColumnInfo {
                            name: "name".to_owned(),
                            data_type: "TEXT".to_owned(),
                            nullable: true,
                            ordinal: 2,
                            default_value: None,
                        },
                    ],
                    constraints: vec![],
                    indexes: vec![],
                    triggers: vec![],
                    estimated_rows: Some(100.0),
                },
            );

            Self {
                kind: DatabaseKind::SQLite,
                schema: SchemaInfo {
                    kind: DatabaseKind::SQLite,
                    schema_name: "main".to_owned(),
                    tables,
                },
            }
        }
    }

    impl DatabaseConnector for MockConnector {
        fn kind(&self) -> DatabaseKind {
            self.kind
        }

        fn gather_schema(&self) -> MetadataResult<SchemaInfo> {
            Ok(self.schema.clone())
        }

        fn gather_statistics(&self, table: &str) -> MetadataResult<TableStats> {
            let table_info = self.schema.tables.get(table).ok_or_else(|| {
                MetadataError::Query {
                    message: format!("table {table} not found"),
                }
            })?;

            Ok(TableStats {
                table_name: table.to_owned(),
                row_count: table_info.estimated_rows.unwrap_or(0.0),
                total_bytes: 0,
                columns: HashMap::new(),
            })
        }

        fn explain_query(&self, _sql: &str) -> MetadataResult<ExplainPlan> {
            use crate::explain::{ExplainNode, NodeType};
            Ok(ExplainPlan {
                root: ExplainNode {
                    node_type: NodeType::SeqScan,
                    join_type: None,
                    relation: None,
                    index_name: None,
                    startup_cost: None,
                    total_cost: None,
                    estimated_rows: None,
                    estimated_width: None,
                    filter: None,
                    scan_direction: None,
                    raw_detail: None,
                    children: Vec::new(),
                },
                query: None,
                total_cost: None,
                total_rows: None,
            })
        }
    }

    #[test]
    fn mock_connector_kind() {
        let conn = MockConnector::new_sqlite();
        assert_eq!(conn.kind(), DatabaseKind::SQLite);
    }

    #[test]
    fn mock_connector_gather_schema() {
        let conn = MockConnector::new_sqlite();
        let schema = conn.gather_schema().expect("should succeed");
        assert_eq!(schema.table_count(), 1);
        assert!(schema.get_table("users").is_some());
    }

    #[test]
    fn mock_connector_gather_statistics() {
        let conn = MockConnector::new_sqlite();
        let stats = conn.gather_statistics("users").expect("should succeed");
        assert!((stats.row_count - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mock_connector_gather_statistics_missing_table() {
        let conn = MockConnector::new_sqlite();
        let result = conn.gather_statistics("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn mock_connector_explain() {
        let conn = MockConnector::new_sqlite();
        let plan = conn.explain_query("SELECT * FROM users").expect("should succeed");
        assert_eq!(
            plan.root.node_type,
            crate::explain::NodeType::SeqScan
        );
    }

    #[test]
    fn connector_is_object_safe() {
        // Verify the trait can be used as a trait object.
        let conn = MockConnector::new_sqlite();
        let boxed: Box<dyn DatabaseConnector> = Box::new(conn);
        assert_eq!(boxed.kind(), DatabaseKind::SQLite);
    }
}
