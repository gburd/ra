//! Polyglot SQL transpiler backend.
//!
//! This module provides integration with the polyglot-sql transpiler,
//! supporting 32+ SQL dialects with high-fidelity translation.

use polyglot_sql::{transpile, DialectType};

use crate::dialect::Dialect;
use crate::error::{TranslationError, TranslationWarning, WarningSeverity};
use crate::{Backend, TranslationResult};

/// Polyglot translation backend implementation.
pub struct PolyglotBackend;

impl Backend for PolyglotBackend {
    fn translate(
        &self,
        sql: &str,
        source: Dialect,
        target: Dialect,
    ) -> Result<TranslationResult, TranslationError> {
        let source_dialect = map_to_polyglot_dialect(source)?;
        let target_dialect = map_to_polyglot_dialect(target)?;

        match transpile(sql, source_dialect, target_dialect) {
            Ok(translated_sql) => {
                let mut warnings = Vec::new();

                // Add informational warning about using polyglot backend
                if source != target {
                    warnings.push(TranslationWarning {
                        severity: WarningSeverity::Info,
                        message: format!(
                            "Translated from {} to {} using Polyglot backend",
                            source, target
                        ),
                        hint: Some("This translation uses the Polyglot SQL transpiler".to_string()),
                    });
                }

                Ok(TranslationResult {
                    sql: translated_sql.join("\n"),
                    warnings,
                })
            }
            Err(e) => Err(TranslationError::TranspilationFailed(format!(
                "Polyglot transpilation failed: {}",
                e
            ))),
        }
    }
}

/// Map our Dialect enum to Polyglot's DialectType.
fn map_to_polyglot_dialect(dialect: Dialect) -> Result<DialectType, TranslationError> {
    let polyglot_dialect = match dialect {
        Dialect::PostgreSql => DialectType::PostgreSQL,
        Dialect::MySql => DialectType::MySQL,
        Dialect::Sqlite => DialectType::SQLite,
        Dialect::DuckDb => DialectType::DuckDB,
        Dialect::MsSql => DialectType::TSQL,
        Dialect::Oracle => DialectType::Oracle,

        #[cfg(feature = "polyglot-backend")]
        Dialect::BigQuery => DialectType::BigQuery,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Snowflake => DialectType::Snowflake,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Databricks => DialectType::Databricks,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Redshift => DialectType::Redshift,
        #[cfg(feature = "polyglot-backend")]
        Dialect::ClickHouse => DialectType::ClickHouse,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Trino => DialectType::Trino,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Presto => DialectType::Presto,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Athena => DialectType::Athena,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Hive => DialectType::Hive,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Spark => DialectType::Spark,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Teradata => DialectType::Teradata,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Exasol => DialectType::Exasol,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Fabric => DialectType::Fabric,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Dremio => DialectType::Dremio,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Drill => DialectType::Drill,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Druid => DialectType::Druid,
        #[cfg(feature = "polyglot-backend")]
        Dialect::CockroachDb => DialectType::CockroachDB,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Materialize => DialectType::Materialize,
        #[cfg(feature = "polyglot-backend")]
        Dialect::RisingWave => DialectType::RisingWave,
        #[cfg(feature = "polyglot-backend")]
        Dialect::SingleStore => DialectType::SingleStore,
        #[cfg(feature = "polyglot-backend")]
        Dialect::StarRocks => DialectType::StarRocks,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Doris => DialectType::Doris,
        #[cfg(feature = "polyglot-backend")]
        Dialect::TiDb => DialectType::TiDB,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Tableau => DialectType::Tableau,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Solr => DialectType::Solr,
        #[cfg(feature = "polyglot-backend")]
        Dialect::Dune => DialectType::Dune,
    };

    Ok(polyglot_dialect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_to_mysql() {
        let backend = PolyglotBackend;
        let sql = "SELECT * FROM users WHERE age > 18";

        let result = backend.translate(sql, Dialect::PostgreSql, Dialect::MySql).unwrap();
        assert!(!result.sql.is_empty());
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_mysql_to_postgres() {
        let backend = PolyglotBackend;
        let sql = "SELECT IFNULL(name, 'Unknown') FROM users";

        let result = backend.translate(sql, Dialect::MySql, Dialect::PostgreSql).unwrap();
        assert!(!result.sql.is_empty());
        // Should translate IFNULL to COALESCE
        assert!(result.sql.contains("COALESCE") || result.sql.contains("coalesce"));
    }

    #[test]
    fn test_same_dialect() {
        let backend = PolyglotBackend;
        let sql = "SELECT * FROM users";

        let result = backend.translate(sql, Dialect::PostgreSql, Dialect::PostgreSql).unwrap();
        assert_eq!(result.sql, sql);
    }
}