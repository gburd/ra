//! Top-level synthesis pipeline.
//!
//! The [`Synthesizer`] orchestrates intent parsing, query generation,
//! validation, and SQL rendering into a single call.

use ra_core::RelExpr;
use serde::{Deserialize, Serialize};

use crate::error::SynthesisError;
use crate::generator::QueryGenerator;
use crate::intent::{IntentParser, QueryIntent};
use crate::render::SqlRenderer;
use crate::schema::SchemaInfo;
use crate::validator::QueryValidator;

/// Result of a successful synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisResult {
    /// The natural language input that was processed.
    pub input: String,
    /// The parsed query intent.
    pub intent: QueryIntent,
    /// The generated relational expression tree.
    pub rel_expr: RelExpr,
    /// The rendered SQL string.
    pub sql: String,
    /// Any validation warnings (non-fatal).
    pub warnings: Vec<String>,
}

/// Request payload for the synthesis API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisRequest {
    /// The natural language query.
    pub query: String,
    /// The database schema to synthesize against.
    pub schema: SchemaInfo,
}

/// Orchestrates the full synthesis pipeline.
pub struct Synthesizer<'a> {
    schema: &'a SchemaInfo,
}

impl<'a> Synthesizer<'a> {
    /// Create a new synthesizer for the given schema.
    #[must_use]
    pub fn new(schema: &'a SchemaInfo) -> Self {
        Self { schema }
    }

    /// Run the full synthesis pipeline on natural language input.
    ///
    /// # Pipeline
    ///
    /// 1. Parse intent from natural language
    /// 2. Generate `RelExpr` from intent
    /// 3. Validate against schema
    /// 4. Render to SQL
    ///
    /// # Errors
    ///
    /// Returns `SynthesisError` if any pipeline stage fails.
    pub fn synthesize(
        &self,
        input: &str,
    ) -> Result<SynthesisResult, SynthesisError> {
        let parser = IntentParser::new(self.schema);
        let intent = parser.parse(input)?;

        let generator = QueryGenerator::new(self.schema);
        let rel_expr = generator.generate(&intent)?;

        let validator = QueryValidator::new(self.schema);
        let validation = validator.validate(&rel_expr)?;

        let sql = SqlRenderer::render(&rel_expr);

        Ok(SynthesisResult {
            input: input.to_string(),
            intent,
            rel_expr,
            sql,
            warnings: validation.issues,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, ForeignKey, TableInfo};

    fn test_schema() -> SchemaInfo {
        let mut schema = SchemaInfo::new();
        schema.add_table(TableInfo::new(
            "users",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("name", "TEXT").not_null(),
                ColumnInfo::new("email", "TEXT"),
                ColumnInfo::new("age", "INTEGER"),
            ],
        ));
        let mut orders = TableInfo::new(
            "orders",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("user_id", "INTEGER").not_null(),
                ColumnInfo::new("amount", "REAL").not_null(),
                ColumnInfo::new("status", "TEXT"),
            ],
        );
        orders.add_foreign_key(ForeignKey {
            columns: vec!["user_id".into()],
            referenced_table: "users".into(),
            referenced_columns: vec!["id".into()],
        });
        schema.add_table(orders);
        schema
    }

    #[test]
    fn synthesize_simple_query() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth
            .synthesize("show all users")
            .expect("test");
        assert!(result.sql.contains("FROM users"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn synthesize_filter_query() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth
            .synthesize("find users where age greater than 25")
            .expect("test");
        assert!(result.sql.contains("WHERE"));
        assert!(result.sql.contains("age"));
    }

    #[test]
    fn synthesize_aggregate_query() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth
            .synthesize("count of users")
            .expect("test");
        assert!(result.sql.contains("COUNT"));
    }

    #[test]
    fn synthesize_limit_query() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth
            .synthesize("show top 5 users")
            .expect("test");
        assert!(result.sql.contains("LIMIT 5"));
    }

    #[test]
    fn synthesize_join_query() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth
            .synthesize("show users and their orders")
            .expect("test");
        assert!(result.sql.contains("JOIN"));
    }

    #[test]
    fn synthesize_order_query() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth
            .synthesize("show users sorted by name")
            .expect("test");
        assert!(result.sql.contains("ORDER BY"));
    }

    #[test]
    fn synthesize_unknown_tables_error() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth.synthesize("show all products");
        assert!(result.is_err());
    }

    #[test]
    fn synthesis_result_serializable() {
        let schema = test_schema();
        let synth = Synthesizer::new(&schema);
        let result = synth
            .synthesize("show all users")
            .expect("test");
        let json = serde_json::to_string(&result).expect("test");
        assert!(!json.is_empty());
    }

    #[test]
    fn synthesis_request_deserializable() {
        let json = r#"{
            "query": "show all users",
            "schema": {"tables": {}}
        }"#;
        let req: SynthesisRequest =
            serde_json::from_str(json).expect("test");
        assert_eq!(req.query, "show all users");
    }
}
