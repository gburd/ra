//! Feature extraction from relational expressions.
//!
//! Converts [`RelExpr`] query plan nodes into fixed-size numeric
//! feature vectors suitable for neural network input. The encoding
//! follows the MSCN (Multi-Set Convolutional Network) approach:
//! one-hot operator types, table presence, predicate column
//! encodings, and log-scaled statistics.

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, Expr, UnaryOp};
use ra_core::statistics::Statistics;

use serde::{Deserialize, Serialize};

/// Schema describing the feature vector layout.
///
/// The schema maps table names and column names to their positions
/// in the one-hot encoded sections of the feature vector. This
/// must be consistent between training and inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSchema {
    /// Map from table name to its one-hot index.
    pub table_indices: HashMap<String, usize>,
    /// Map from column name to its one-hot index.
    pub column_indices: HashMap<String, usize>,
    /// Total number of features in the output vector.
    pub total_features: usize,
}

/// Feature layout offsets within the vector.
const OP_TYPE_OFFSET: usize = 0;
const OP_TYPE_COUNT: usize = 10;
const JOIN_TYPE_OFFSET: usize = OP_TYPE_OFFSET + OP_TYPE_COUNT;
const JOIN_TYPE_COUNT: usize = 7;
const PRED_OP_OFFSET: usize = JOIN_TYPE_OFFSET + JOIN_TYPE_COUNT;
const PRED_OP_COUNT: usize = 15;
const STATS_OFFSET: usize = PRED_OP_OFFSET + PRED_OP_COUNT;
const STATS_COUNT: usize = 6;
const FIXED_FEATURES: usize = STATS_OFFSET + STATS_COUNT;

impl FeatureSchema {
    /// Build a schema from known table and column names.
    #[must_use]
    pub fn new(tables: &[&str], columns: &[&str]) -> Self {
        let mut table_indices = HashMap::new();
        for (i, table) in tables.iter().enumerate() {
            table_indices.insert((*table).to_string(), i);
        }

        let mut column_indices = HashMap::new();
        for (i, col) in columns.iter().enumerate() {
            column_indices.insert((*col).to_string(), i);
        }

        let total_features =
            FIXED_FEATURES + tables.len() + columns.len();

        Self {
            table_indices,
            column_indices,
            total_features,
        }
    }

    /// Extract features from a relational expression.
    ///
    /// `stats` maps table names to their statistics. Tables not
    /// present in the map get zero-valued statistic features.
    ///
    /// Returns a fixed-size vector of `self.total_features`
    /// elements.
    #[must_use]
    pub fn extract(
        &self,
        expr: &RelExpr,
        stats: &HashMap<String, Statistics>,
    ) -> Vec<f64> {
        let mut features = vec![0.0; self.total_features];
        self.encode_expr(expr, stats, &mut features);
        features
    }

    #[allow(clippy::cast_precision_loss)]
    fn encode_expr(
        &self,
        expr: &RelExpr,
        stats: &HashMap<String, Statistics>,
        features: &mut [f64],
    ) {
        match expr {
            RelExpr::Scan { table, .. } => {
                features[OP_TYPE_OFFSET] = 1.0;
                self.encode_table(table, stats, features);
            }
            RelExpr::Filter {
                predicate, input, ..
            } => {
                features[OP_TYPE_OFFSET + 1] = 1.0;
                self.encode_predicate(predicate, features);
                self.encode_expr(input, stats, features);
            }
            RelExpr::Project { input, columns, .. } => {
                features[OP_TYPE_OFFSET + 2] = 1.0;
                features[STATS_OFFSET + 5] =
                    log_scale(columns.len() as f64);
                self.encode_expr(input, stats, features);
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
                ..
            } => {
                features[OP_TYPE_OFFSET + 3] = 1.0;
                encode_join_type(*join_type, features);
                self.encode_predicate(condition, features);
                self.encode_expr(left, stats, features);
                self.encode_expr(right, stats, features);
            }
            RelExpr::Aggregate {
                group_by, input, ..
            } => {
                features[OP_TYPE_OFFSET + 4] = 1.0;
                features[STATS_OFFSET + 4] =
                    log_scale(group_by.len() as f64);
                self.encode_expr(input, stats, features);
            }
            RelExpr::Sort { input, keys, .. } => {
                features[OP_TYPE_OFFSET + 5] = 1.0;
                features[STATS_OFFSET + 5] =
                    log_scale(keys.len() as f64);
                self.encode_expr(input, stats, features);
            }
            RelExpr::Limit {
                count,
                offset,
                input,
                ..
            } => {
                features[OP_TYPE_OFFSET + 6] = 1.0;
                features[STATS_OFFSET + 2] =
                    log_scale(*count as f64);
                features[STATS_OFFSET + 3] =
                    log_scale(*offset as f64);
                self.encode_expr(input, stats, features);
            }
            RelExpr::Union { left, right, .. } => {
                features[OP_TYPE_OFFSET + 7] = 1.0;
                self.encode_expr(left, stats, features);
                self.encode_expr(right, stats, features);
            }
            RelExpr::Intersect { left, right, .. } => {
                features[OP_TYPE_OFFSET + 8] = 1.0;
                self.encode_expr(left, stats, features);
                self.encode_expr(right, stats, features);
            }
            RelExpr::Except { left, right, .. } => {
                features[OP_TYPE_OFFSET + 9] = 1.0;
                self.encode_expr(left, stats, features);
                self.encode_expr(right, stats, features);
            }
            RelExpr::CTE {
                definition, body, ..
            } => {
                features[OP_TYPE_OFFSET + 4] = 1.0;
                self.encode_expr(definition, stats, features);
                self.encode_expr(body, stats, features);
            }
            RelExpr::Window { input, .. }
            | RelExpr::Distinct { input, .. } => {
                features[OP_TYPE_OFFSET + 2] = 1.0;
                self.encode_expr(input, stats, features);
            }
            RelExpr::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => {
                features[OP_TYPE_OFFSET + 4] = 1.0;
                self.encode_expr(base_case, stats, features);
                self.encode_expr(recursive_case, stats, features);
                self.encode_expr(body, stats, features);
            }
            RelExpr::Values { .. } => {
                features[OP_TYPE_OFFSET] = 1.0;
            }
            RelExpr::Unnest { input, .. }
            | RelExpr::TableFunction { input, .. } => {
                features[OP_TYPE_OFFSET + 2] = 1.0;
                if let Some(inp) = input {
                    self.encode_expr(inp, stats, features);
                }
            }
            RelExpr::MultiUnnest { .. } => {
                features[OP_TYPE_OFFSET + 2] = 1.0;
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn encode_table(
        &self,
        table: &str,
        stats: &HashMap<String, Statistics>,
        features: &mut [f64],
    ) {
        if let Some(&idx) = self.table_indices.get(table) {
            let offset = FIXED_FEATURES + idx;
            if offset < features.len() {
                features[offset] = 1.0;
            }
        }

        if let Some(s) = stats.get(table) {
            features[STATS_OFFSET] = log_scale(s.row_count);
            features[STATS_OFFSET + 1] =
                log_scale(s.avg_row_size as f64);
        }
    }

    fn encode_predicate(
        &self,
        expr: &Expr,
        features: &mut [f64],
    ) {
        match expr {
            Expr::Column(col_ref) => {
                if let Some(&idx) =
                    self.column_indices.get(&col_ref.column)
                {
                    let offset = FIXED_FEATURES
                        + self.table_indices.len()
                        + idx;
                    if offset < features.len() {
                        features[offset] = 1.0;
                    }
                }
            }
            Expr::BinOp {
                op, left, right, ..
            } => {
                encode_binop(*op, features);
                self.encode_predicate(left, features);
                self.encode_predicate(right, features);
            }
            Expr::UnaryOp { op, operand, .. } => {
                encode_unaryop(*op, features);
                self.encode_predicate(operand, features);
            }
            Expr::Const(_)
            | Expr::Function { .. }
            | Expr::Case { .. }
            | Expr::Cast { .. }
            | Expr::Array(_)
            | Expr::ArrayIndex(_, _)
            | Expr::ArraySlice { .. } => {}
        }
    }
}

fn encode_join_type(jt: JoinType, features: &mut [f64]) {
    let idx = match jt {
        JoinType::Inner => 0,
        JoinType::LeftOuter => 1,
        JoinType::RightOuter => 2,
        JoinType::FullOuter => 3,
        JoinType::Cross => 4,
        JoinType::Semi => 5,
        JoinType::Anti => 6,
    };
    features[JOIN_TYPE_OFFSET + idx] = 1.0;
}

fn encode_binop(op: BinOp, features: &mut [f64]) {
    let idx = match op {
        BinOp::Eq => 0,
        BinOp::Ne => 1,
        BinOp::Lt => 2,
        BinOp::Le => 3,
        BinOp::Gt => 4,
        BinOp::Ge => 5,
        BinOp::And => 6,
        BinOp::Or => 7,
        BinOp::Add => 8,
        BinOp::Sub => 9,
        BinOp::Mul => 10,
        BinOp::Div => 11,
        BinOp::Mod => 12,
        BinOp::Concat => 13,
        BinOp::JsonAccess => 14,
    };
    features[PRED_OP_OFFSET + idx] = 1.0;
}

fn encode_unaryop(op: UnaryOp, features: &mut [f64]) {
    let idx = match op {
        UnaryOp::Not | UnaryOp::Neg => 12,
        UnaryOp::IsNull => 13,
        UnaryOp::IsNotNull => 14,
    };
    features[PRED_OP_OFFSET + idx] = 1.0;
}

/// Log-scale a positive value for stable neural network input.
/// Uses `log2(1 + x)` to handle zero gracefully.
#[must_use]
pub fn log_scale(x: f64) -> f64 {
    (1.0 + x.max(0.0)).log2()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{
        BinOp as ExprBinOp, ColumnRef, Const, Expr,
    };

    fn test_schema() -> FeatureSchema {
        FeatureSchema::new(
            &["users", "orders"],
            &["id", "name", "amount", "user_id"],
        )
    }

    fn test_stats() -> HashMap<String, Statistics> {
        let mut map = HashMap::new();
        map.insert("users".to_string(), Statistics::new(1000.0));
        map.insert("orders".to_string(), Statistics::new(5000.0));
        map
    }

    #[test]
    fn scan_features() {
        let schema = test_schema();
        let stats_map = test_stats();
        let expr = RelExpr::scan("users");
        let features = schema.extract(&expr, &stats_map);

        assert_eq!(features.len(), schema.total_features);
        assert!(
            (features[OP_TYPE_OFFSET] - 1.0).abs() < f64::EPSILON
        );
        let users_idx = FIXED_FEATURES;
        assert!(
            (features[users_idx] - 1.0).abs() < f64::EPSILON
        );
        assert!(features[STATS_OFFSET] > 0.0);
    }

    #[test]
    fn filter_features() {
        let schema = test_schema();
        let stats_map = test_stats();
        let expr = RelExpr::scan("orders").filter(Expr::BinOp {
            op: ExprBinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new(
                "amount",
            ))),
            right: Box::new(Expr::Const(Const::Int(100))),
        });
        let features = schema.extract(&expr, &stats_map);

        assert!(
            (features[OP_TYPE_OFFSET + 1] - 1.0).abs()
                < f64::EPSILON
        );
        assert!(
            (features[PRED_OP_OFFSET + 4] - 1.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn join_features() {
        let schema = test_schema();
        let stats_map = test_stats();
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: ExprBinOp::Eq,
                left: Box::new(Expr::Column(
                    ColumnRef::qualified("users", "id"),
                )),
                right: Box::new(Expr::Column(
                    ColumnRef::qualified("orders", "user_id"),
                )),
            },
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let features = schema.extract(&expr, &stats_map);

        assert!(
            (features[OP_TYPE_OFFSET + 3] - 1.0).abs()
                < f64::EPSILON
        );
        assert!(
            (features[JOIN_TYPE_OFFSET] - 1.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn feature_schema_dimensions() {
        let schema = test_schema();
        assert_eq!(schema.total_features, FIXED_FEATURES + 2 + 4);
    }

    #[test]
    fn log_scale_zero() {
        let val = log_scale(0.0);
        assert!((val - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn log_scale_positive() {
        let val = log_scale(1.0);
        assert!((val - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn log_scale_negative_clamped() {
        let val = log_scale(-5.0);
        assert!((val - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_table_ignored() {
        let schema = test_schema();
        let expr = RelExpr::scan("unknown_table");
        let empty_stats = HashMap::new();
        let features = schema.extract(&expr, &empty_stats);
        assert!(
            (features[STATS_OFFSET] - 0.0).abs() < f64::EPSILON
        );
    }

    #[test]
    fn schema_serialization() {
        let schema = test_schema();
        let json = serde_json::to_string(&schema)
            .expect("serialize schema");
        let restored: FeatureSchema = serde_json::from_str(&json)
            .expect("deserialize schema");
        assert_eq!(
            restored.total_features,
            schema.total_features
        );
        assert_eq!(
            restored.table_indices.len(),
            schema.table_indices.len()
        );
    }
}
