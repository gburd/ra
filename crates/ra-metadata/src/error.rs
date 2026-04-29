//! Error types for the metadata crate.

use serde::{Deserialize, Serialize};

/// Errors that can occur during metadata operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum MetadataError {
    /// Failed to connect to the database.
    #[error("connection failed: {message}")]
    Connection {
        /// Human-readable error description.
        message: String,
    },

    /// A query against the system catalog failed.
    #[error("query failed: {message}")]
    Query {
        /// Human-readable error description.
        message: String,
    },

    /// Schema introspection failed for a specific object.
    #[error("schema introspection failed for {object}: {message}")]
    SchemaIntrospection {
        /// The schema object that caused the error.
        object: String,
        /// Human-readable error description.
        message: String,
    },

    /// Statistics gathering failed for a table.
    #[error("statistics gathering failed for {table}: {message}")]
    StatisticsGathering {
        /// The table for which statistics gathering failed.
        table: String,
        /// Human-readable error description.
        message: String,
    },

    /// Failed to parse EXPLAIN output.
    #[error("explain parse failed: {message}")]
    ExplainParse {
        /// Human-readable error description.
        message: String,
    },

    /// The requested feature is not supported by this backend.
    #[error("unsupported: {message}")]
    Unsupported {
        /// Human-readable error description.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_error_display() {
        let err = MetadataError::Connection {
            message: "timeout after 30s".into(),
        };
        assert_eq!(err.to_string(), "connection failed: timeout after 30s");
    }

    #[test]
    fn query_error_display() {
        let err = MetadataError::Query {
            message: "syntax error at position 42".into(),
        };
        assert_eq!(err.to_string(), "query failed: syntax error at position 42");
    }

    #[test]
    fn schema_introspection_error_display() {
        let err = MetadataError::SchemaIntrospection {
            object: "users".into(),
            message: "permission denied".into(),
        };
        assert_eq!(
            err.to_string(),
            "schema introspection failed for users: permission denied"
        );
    }

    #[test]
    fn statistics_gathering_error_display() {
        let err = MetadataError::StatisticsGathering {
            table: "orders".into(),
            message: "ANALYZE not run".into(),
        };
        assert_eq!(
            err.to_string(),
            "statistics gathering failed for orders: ANALYZE not run"
        );
    }

    #[test]
    fn explain_parse_error_display() {
        let err = MetadataError::ExplainParse {
            message: "unexpected token at line 3".into(),
        };
        assert_eq!(
            err.to_string(),
            "explain parse failed: unexpected token at line 3"
        );
    }

    #[test]
    fn unsupported_error_display() {
        let err = MetadataError::Unsupported {
            message: "materialized views".into(),
        };
        assert_eq!(err.to_string(), "unsupported: materialized views");
    }

    #[test]
    fn error_is_clone() {
        let err = MetadataError::Connection {
            message: "test".into(),
        };
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test code")]
    fn error_serialize_roundtrip() {
        let err = MetadataError::Query {
            message: "test error".into(),
        };
        let json = serde_json::to_string(&err).expect("serialization should succeed");
        let deserialized: MetadataError =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(err, deserialized);
    }
}
