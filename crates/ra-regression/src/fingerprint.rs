//! Plan fingerprinting for structural comparison.

use datafusion::logical_expr::LogicalPlan;
use sha2::{Digest, Sha256};
use std::fmt;

/// A hash of a logical plan's structure, ignoring constants.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlanFingerprint {
    hash: String,
}

impl PlanFingerprint {
    /// Create a fingerprint from a logical plan.
    pub fn from_plan(plan: &LogicalPlan) -> Self {
        let canonical = Self::canonicalize_plan(plan);
        let hash = Self::hash_string(&canonical);
        Self { hash }
    }

    /// Get the hash string representation.
    pub fn as_str(&self) -> &str {
        &self.hash
    }

    /// Canonicalize a plan to ignore constants and focus on structure.
    fn canonicalize_plan(plan: &LogicalPlan) -> String {
        // Build a canonical representation that ignores constants
        let mut result = String::new();
        Self::visit_plan(&mut result, plan, 0);
        result
    }

    /// Recursively visit plan nodes to build canonical representation.
    fn visit_plan(output: &mut String, plan: &LogicalPlan, depth: usize) {
        let indent = "  ".repeat(depth);

        // Add node type
        let node_type = match plan {
            LogicalPlan::Projection(_) => "Projection",
            LogicalPlan::Filter(_) => "Filter",
            LogicalPlan::Aggregate(_) => "Aggregate",
            LogicalPlan::Sort(_) => "Sort",
            LogicalPlan::Join(_) => "Join",
            LogicalPlan::Repartition(_) => "Repartition",
            LogicalPlan::Union(_) => "Union",
            LogicalPlan::TableScan(_) => "TableScan",
            LogicalPlan::EmptyRelation(_) => "EmptyRelation",
            LogicalPlan::Limit(_) => "Limit",
            LogicalPlan::Subquery(_) => "Subquery",
            LogicalPlan::SubqueryAlias(_) => "SubqueryAlias",
            LogicalPlan::Values(_) => "Values",
            LogicalPlan::Explain(_) => "Explain",
            LogicalPlan::Analyze(_) => "Analyze",
            LogicalPlan::Extension(_) => "Extension",
            LogicalPlan::Prepare(_) => "Prepare",
            LogicalPlan::Dml(_) => "Dml",
            LogicalPlan::Ddl(_) => "Ddl",
            LogicalPlan::Copy(_) => "Copy",
            LogicalPlan::DescribeTable(_) => "DescribeTable",
            LogicalPlan::Unnest(_) => "Unnest",
            LogicalPlan::RecursiveQuery(_) => "RecursiveQuery",
            _ => "Unknown",
        };

        output.push_str(&format!("{}{}\n", indent, node_type));

        // Add key attributes (but not constants)
        match plan {
            LogicalPlan::Join(join) => {
                output.push_str(&format!("{}  join_type: {:?}\n", indent, join.join_type));
            }
            LogicalPlan::Aggregate(agg) => {
                output.push_str(&format!("{}  group_exprs: {}\n", indent, agg.group_expr.len()));
                output.push_str(&format!("{}  agg_exprs: {}\n", indent, agg.aggr_expr.len()));
            }
            LogicalPlan::TableScan(scan) => {
                output.push_str(&format!("{}  table: {}\n", indent, scan.table_name));
                if let Some(projection) = &scan.projection {
                    output.push_str(&format!("{}  projection_count: {}\n", indent, projection.len()));
                }
            }
            _ => {}
        }

        // Visit children
        for child in plan.inputs() {
            Self::visit_plan(output, child, depth + 1);
        }
    }

    /// Hash a string using SHA256.
    fn hash_string(s: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(s.as_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    }
}

impl fmt::Display for PlanFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.hash[..16]) // Show first 16 chars for brevity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::datatypes::Schema;
    use datafusion::logical_expr::{EmptyRelation, LogicalPlan};
    use std::sync::Arc;

    #[test]
    fn test_fingerprint_generation() {
        // Test that fingerprints can be generated for different plan types
        let schema = Arc::new(Schema::empty());

        let plan1 = LogicalPlan::EmptyRelation(EmptyRelation {
            produce_one_row: false,
            schema: schema.clone(),
        });

        let plan2 = LogicalPlan::EmptyRelation(EmptyRelation {
            produce_one_row: true,
            schema,
        });

        let fp1 = PlanFingerprint::from_plan(&plan1);
        let fp2 = PlanFingerprint::from_plan(&plan2);

        // Fingerprints should be non-empty
        assert!(!fp1.as_str().is_empty());
        assert!(!fp2.as_str().is_empty());

        // Display should show first 16 chars
        let display1 = format!("{}", fp1);
        assert!(display1.len() <= 16);
    }

    #[test]
    fn test_fingerprint_equality() {
        let schema = Arc::new(Schema::empty());

        // Same plans should produce same fingerprints
        let plan1 = LogicalPlan::EmptyRelation(EmptyRelation {
            produce_one_row: false,
            schema: schema.clone(),
        });

        let plan2 = LogicalPlan::EmptyRelation(EmptyRelation {
            produce_one_row: false,
            schema,
        });

        let fp1 = PlanFingerprint::from_plan(&plan1);
        let fp2 = PlanFingerprint::from_plan(&plan2);

        assert_eq!(fp1, fp2);
    }
}