//! Validation of generated relational expressions against a schema.
//!
//! Checks that all table and column references in a [`RelExpr`] tree
//! actually exist in the schema.

use ra_core::RelExpr;

use crate::error::SynthesisError;
use crate::schema::SchemaInfo;

/// Validates [`RelExpr`] trees against a schema.
pub struct QueryValidator<'a> {
    schema: &'a SchemaInfo,
}

/// Result of validation: either success or a list of issues.
#[derive(Debug)]
pub struct ValidationResult {
    /// Validation issues found.
    pub issues: Vec<String>,
}

impl ValidationResult {
    /// Whether validation passed with no issues.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

impl<'a> QueryValidator<'a> {
    /// Create a new validator for the given schema.
    #[must_use]
    pub fn new(schema: &'a SchemaInfo) -> Self {
        Self { schema }
    }

    /// Validate a relational expression tree.
    ///
    /// # Errors
    ///
    /// Returns `SynthesisError::ValidationFailed` if the expression
    /// references tables or columns not in the schema.
    pub fn validate(
        &self,
        expr: &RelExpr,
    ) -> Result<ValidationResult, SynthesisError> {
        let mut issues = Vec::new();
        self.check_node(expr, &mut issues);
        Ok(ValidationResult { issues })
    }

    fn check_node(
        &self,
        expr: &RelExpr,
        issues: &mut Vec<String>,
    ) {
        if let RelExpr::Scan { table, .. } = expr {
            if self.schema.find_table(table).is_none() {
                issues.push(format!("unknown table: {table}"));
            }
        }

        let cols = expr.referenced_columns();
        for col in &cols {
            if let Some(table_name) = &col.table {
                if let Some(table) =
                    self.schema.find_table(table_name)
                {
                    if table.find_column(&col.column).is_none() {
                        issues.push(format!(
                            "unknown column `{}` in table `{}`",
                            col.column, table_name
                        ));
                    }
                }
            }
        }

        for child in expr.children() {
            self.check_node(child, issues);
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, TableInfo};
    use ra_core::{BinOp, ColumnRef, Const, Expr};

    fn test_schema() -> SchemaInfo {
        let mut schema = SchemaInfo::new();
        schema.add_table(TableInfo::new(
            "users",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("name", "TEXT"),
            ],
        ));
        schema
    }

    #[test]
    fn valid_scan() {
        let schema = test_schema();
        let validator = QueryValidator::new(&schema);
        let expr = RelExpr::scan("users");
        let result = validator.validate(&expr).expect("test");
        assert!(result.is_valid());
    }

    #[test]
    fn invalid_table() {
        let schema = test_schema();
        let validator = QueryValidator::new(&schema);
        let expr = RelExpr::scan("nonexistent");
        let result = validator.validate(&expr).expect("test");
        assert!(!result.is_valid());
        assert!(result.issues[0].contains("unknown table"));
    }

    #[test]
    fn invalid_column() {
        let schema = test_schema();
        let validator = QueryValidator::new(&schema);
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(
                "users", "missing",
            ))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });
        let result = validator.validate(&expr).expect("test");
        assert!(!result.is_valid());
        assert!(result.issues[0].contains("unknown column"));
    }

    #[test]
    fn valid_filter_with_known_column() {
        let schema = test_schema();
        let validator = QueryValidator::new(&schema);
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(
                "users", "name",
            ))),
            right: Box::new(Expr::Const(Const::String(
                "alice".into(),
            ))),
        });
        let result = validator.validate(&expr).expect("test");
        assert!(result.is_valid());
    }
}
