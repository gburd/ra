//! Frontmatter metadata validation for `.rra` rule files.
//!
//! Checks required fields, known database names, category paths,
//! and version format.

use thiserror::Error;

use crate::RuleMetadata;

/// An error produced when metadata validation fails.
#[derive(Debug, Error)]
pub enum ValidationError {
    /// A required field is missing or empty.
    #[error("required field `{field}` is empty")]
    RequiredFieldEmpty {
        /// Name of the field that failed validation.
        field: &'static str,
    },

    /// The category path does not match any known prefix.
    #[error(
        "unknown category `{category}`; \
         expected one of: {expected}"
    )]
    UnknownCategory {
        /// The invalid category value.
        category: String,
        /// Comma-separated list of valid prefixes.
        expected: String,
    },

    /// A database name is not in the known set.
    #[error(
        "unknown database `{database}`; \
         known databases: {known}"
    )]
    UnknownDatabase {
        /// The invalid database name.
        database: String,
        /// Comma-separated list of known databases.
        known: String,
    },

    /// The version string does not look like semver.
    #[error(
        "invalid version `{version}`; \
         expected MAJOR.MINOR.PATCH format"
    )]
    InvalidVersion {
        /// The invalid version string.
        version: String,
    },
}

// ── Known values ─────────────────────────────────────────────

/// Database names recognized by the rule system.
///
/// Matches the directories under `rules/database-specific/`.
pub const KNOWN_DATABASES: &[&str] = &[
    "agensgraph",
    "apache-kylin",
    "aurora",
    "bigquery",
    "blazingsql",
    "blinkdb",
    "brytlyt",
    "c-store",
    "calcite",
    "citus",
    "clickhouse",
    "cockroachdb",
    "cosmosdb",
    "couchbase",
    "databricks",
    "datadog",
    "dataflow",
    "datafusion",
    "db2",
    "debezium",
    "derby",
    "differential-dataflow",
    "drill",
    "druid",
    "duckdb",
    "dynamodb",
    "flink",
    "greenplum",
    "heavydb",
    "hive",
    "hyper",
    "ibm-netezza",
    "impala",
    "influxdb",
    "intel-pac",
    "janusgraph",
    "ksqldb",
    "logicblox",
    "mariadb",
    "materialize",
    "memgraph",
    "memsql", // Legacy name for SingleStore
    "monetdb",
    "mongodb",
    "mssql",
    "mysql",
    "neo4j",
    "neptune",
    "noisepage",
    "noria",
    "omnisci", // Legacy name for HeavyDB
    "oracle",
    "pg-strom",
    "pinot",
    "postgresql",
    "presto",
    "questdb",
    "redshift",
    "research",
    "risingwave",
    "sap-hana",
    "singlestore",
    "snowflake",
    "spanner",
    "spark",
    "spark-streaming",
    "sqlite",
    "sqream",
    "telegraphcq",
    "theoretical",
    "tidb",
    "tigergraph",
    "timely-dataflow",
    "timescaledb",
    "trino",
    "umbra",
    "vectorwise",
    "velox",
    "verdictdb",
    "vertica",
    "vitess",
    "voltdb",
    "xilinx-alveo",
    "yellowbrick",
    "yugabyte", // Legacy name, use yugabytedb
    "yugabytedb",
];

/// Top-level category prefixes from `rules/index.toml`.
pub const KNOWN_CATEGORY_PREFIXES: &[&str] = &[
    "logical",
    "physical",
    "database-specific",
    "execution-models",
    "cost-models",
    "experimental",
    "distributed",
    "federated",
    "hardware",
    "multi-model",
];

// ── Public API ───────────────────────────────────────────────

/// Validate all metadata fields, returning the first error.
///
/// # Errors
///
/// Returns [`ValidationError`] if any field is invalid.
pub fn validate_metadata(meta: &RuleMetadata) -> Result<(), ValidationError> {
    validate_required_fields(meta)?;
    validate_category(&meta.category)?;
    validate_databases(&meta.databases)?;
    validate_version(&meta.version)?;
    Ok(())
}

/// Validate and collect *all* errors instead of failing fast.
#[must_use]
pub fn validate_metadata_all(meta: &RuleMetadata) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    if let Err(e) = validate_required_fields(meta) {
        errors.push(e);
    }
    if let Err(e) = validate_category(&meta.category) {
        errors.push(e);
    }
    for db_err in validate_databases_all(&meta.databases) {
        errors.push(db_err);
    }
    if let Err(e) = validate_version(&meta.version) {
        errors.push(e);
    }
    errors
}

// ── Internals ────────────────────────────────────────────────

fn validate_required_fields(meta: &RuleMetadata) -> Result<(), ValidationError> {
    if meta.id.trim().is_empty() {
        return Err(ValidationError::RequiredFieldEmpty { field: "id" });
    }
    if meta.name.trim().is_empty() {
        return Err(ValidationError::RequiredFieldEmpty { field: "name" });
    }
    if meta.category.trim().is_empty() {
        return Err(ValidationError::RequiredFieldEmpty { field: "category" });
    }
    Ok(())
}

fn validate_category(category: &str) -> Result<(), ValidationError> {
    let prefix = category.split('/').next().unwrap_or(category);
    if KNOWN_CATEGORY_PREFIXES.contains(&prefix) {
        return Ok(());
    }
    Err(ValidationError::UnknownCategory {
        category: category.to_owned(),
        expected: KNOWN_CATEGORY_PREFIXES.join(", "),
    })
}

fn validate_databases(databases: &[String]) -> Result<(), ValidationError> {
    for db in databases {
        let lower = db.to_lowercase();
        if !KNOWN_DATABASES.contains(&lower.as_str()) {
            return Err(ValidationError::UnknownDatabase {
                database: db.clone(),
                known: KNOWN_DATABASES.join(", "),
            });
        }
    }
    Ok(())
}

fn validate_databases_all(databases: &[String]) -> Vec<ValidationError> {
    databases
        .iter()
        .filter(|db| {
            let lower = db.to_lowercase();
            !KNOWN_DATABASES.contains(&lower.as_str())
        })
        .map(|db| ValidationError::UnknownDatabase {
            database: db.clone(),
            known: KNOWN_DATABASES.join(", "),
        })
        .collect()
}

fn validate_version(version: &str) -> Result<(), ValidationError> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(ValidationError::InvalidVersion {
            version: version.to_owned(),
        });
    }
    for part in &parts {
        if part.parse::<u32>().is_err() {
            return Err(ValidationError::InvalidVersion {
                version: version.to_owned(),
            });
        }
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn valid_meta() -> RuleMetadata {
        RuleMetadata {
            id: "test-rule".to_owned(),
            name: "Test Rule".to_owned(),
            category: "logical/predicate-pushdown".to_owned(),
            databases: vec!["postgresql".to_owned()],
            standard: None,
            execution_models: vec![],
            version: "1.0.0".to_owned(),
            authors: vec![],
            tags: vec![],
            preconditions: vec![],
        }
    }

    #[test]
    fn valid_metadata_passes() {
        assert!(validate_metadata(&valid_meta()).is_ok());
    }

    #[test]
    fn empty_id_rejected() {
        let mut m = valid_meta();
        m.id = String::new();
        let err = validate_metadata(&m).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::RequiredFieldEmpty { field: "id" }
        ));
    }

    #[test]
    fn whitespace_only_name_rejected() {
        let mut m = valid_meta();
        m.name = "  ".to_owned();
        let err = validate_metadata(&m).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::RequiredFieldEmpty { field: "name" }
        ));
    }

    #[test]
    fn empty_category_rejected() {
        let mut m = valid_meta();
        m.category = String::new();
        let err = validate_metadata(&m).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::RequiredFieldEmpty { field: "category" }
        ));
    }

    #[test]
    fn unknown_category_rejected() {
        let mut m = valid_meta();
        m.category = "bogus/thing".to_owned();
        let err = validate_metadata(&m).unwrap_err();
        assert!(matches!(err, ValidationError::UnknownCategory { .. }));
    }

    #[test]
    fn known_categories_accepted() {
        for prefix in KNOWN_CATEGORY_PREFIXES {
            let mut m = valid_meta();
            m.category = format!("{prefix}/sub");
            assert!(
                validate_metadata(&m).is_ok(),
                "category prefix `{prefix}` should be valid"
            );
        }
    }

    #[test]
    fn unknown_database_rejected() {
        let mut m = valid_meta();
        m.databases = vec!["nosuchdb".to_owned()];
        let err = validate_metadata(&m).unwrap_err();
        assert!(matches!(err, ValidationError::UnknownDatabase { .. }));
    }

    #[test]
    fn known_databases_accepted() {
        for db in KNOWN_DATABASES {
            let mut m = valid_meta();
            m.databases = vec![(*db).to_owned()];
            assert!(
                validate_metadata(&m).is_ok(),
                "database `{db}` should be valid"
            );
        }
    }

    #[test]
    fn database_case_insensitive() {
        let mut m = valid_meta();
        m.databases = vec!["PostgreSQL".to_owned()];
        assert!(validate_metadata(&m).is_ok());
    }

    #[test]
    fn version_valid_formats() {
        for v in &["0.1.0", "1.0.0", "12.34.56"] {
            let mut m = valid_meta();
            m.version = (*v).to_owned();
            assert!(
                validate_metadata(&m).is_ok(),
                "version `{v}` should be valid"
            );
        }
    }

    #[test]
    fn version_invalid_formats() {
        for v in &["1.0", "1", "v1.0.0", "1.0.0-beta", "a.b.c"] {
            let mut m = valid_meta();
            m.version = (*v).to_owned();
            let err = validate_metadata(&m).unwrap_err();
            assert!(
                matches!(err, ValidationError::InvalidVersion { .. }),
                "version `{v}` should be invalid"
            );
        }
    }

    #[test]
    fn validate_all_collects_multiple_errors() {
        let m = RuleMetadata {
            id: String::new(),
            name: "N".to_owned(),
            category: "bogus".to_owned(),
            databases: vec!["nosuchdb".to_owned()],
            standard: None,
            execution_models: vec![],
            version: "bad".to_owned(),
            authors: vec![],
            tags: vec![],
            preconditions: vec![],
        };
        let errs = validate_metadata_all(&m);
        assert!(errs.len() >= 4, "expected >= 4 errors, got: {errs:?}");
    }

    #[test]
    fn empty_databases_passes() {
        let mut m = valid_meta();
        m.databases = vec![];
        assert!(validate_metadata(&m).is_ok());
    }
}
