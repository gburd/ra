//! E-graph analysis for tracking properties of equivalence classes.
//!
//! The [`RelAnalysis`] tracks metadata about each e-class, such as
//! which tables are referenced, estimated cardinality, and whether
//! the expression is a relational or scalar node. This information
//! drives cost estimation and condition checking in rewrite rules.

use std::collections::{HashMap, HashSet};

use egg::{Analysis, DidMerge, EGraph, Id};
use ra_core::facts::TableInfo;

use crate::egraph::RelLang;

/// Metadata tracked per e-class during equality saturation.
#[derive(Debug, Clone, Default)]
pub struct RelData {
    /// Table names referenced by this e-class.
    pub tables: HashSet<String>,
    /// Whether this e-class contains a relational operator.
    pub is_relational: bool,
    /// Estimated row count (if computable).
    pub estimated_rows: Option<f64>,
}

/// E-graph analysis that tracks relational metadata.
#[derive(Debug, Clone, Default)]
pub struct RelAnalysis {
    /// Table schema information, including storage format.
    /// Used by storage-specific optimization rules to check preconditions.
    pub table_info: HashMap<String, TableInfo>,
}

impl Analysis<RelLang> for RelAnalysis {
    type Data = RelData;

    #[expect(clippy::match_same_arms, reason = "legacy allow")]
    fn make(egraph: &mut EGraph<RelLang, Self>, enode: &RelLang, _id: Id) -> Self::Data {
        let mut data = RelData::default();

        match enode {
            RelLang::Scan([table_id])
            | RelLang::ScanAlias([table_id, _])
            | RelLang::IndexScan([table_id, _])
            | RelLang::IndexOnlyScan([table_id, _, _, _]) => {
                data.is_relational = true;
                if let Some(sym) = get_symbol(egraph, *table_id) {
                    data.tables.insert(sym);
                }
            }
            RelLang::Filter([_, input_id])
            | RelLang::Project([_, input_id])
            | RelLang::Sort([_, input_id]) => {
                data.is_relational = true;
                merge_child_tables(&mut data.tables, egraph, *input_id);
            }
            RelLang::Aggregate([_, _, input_id]) | RelLang::Limit([_, _, input_id]) => {
                data.is_relational = true;
                merge_child_tables(&mut data.tables, egraph, *input_id);
            }
            RelLang::MetadataLookup([table_id, _]) | RelLang::MvScan([table_id, _, _, _]) => {
                data.is_relational = true;
                if let Some(sym) = get_symbol(egraph, *table_id) {
                    data.tables.insert(sym);
                }
            }
            RelLang::Join([_, _, left_id, right_id])
            | RelLang::HashJoinOp([_, _, left_id, right_id])
            | RelLang::MergeJoinOp([_, _, left_id, right_id])
            | RelLang::NestLoopOp([_, _, left_id, right_id])
            | RelLang::IndexNestLoopOp([_, _, left_id, right_id])
            | RelLang::Union([_, left_id, right_id])
            | RelLang::Intersect([_, left_id, right_id])
            | RelLang::Except([_, left_id, right_id]) => {
                data.is_relational = true;
                merge_child_tables(&mut data.tables, egraph, *left_id);
                merge_child_tables(&mut data.tables, egraph, *right_id);
            }
            _ => {}
        }

        data
    }

    fn merge(&mut self, to: &mut Self::Data, from: Self::Data) -> DidMerge {
        let before_len = to.tables.len();
        to.tables.extend(from.tables);
        to.is_relational |= from.is_relational;

        if from.estimated_rows.is_some() && to.estimated_rows.is_none() {
            to.estimated_rows = from.estimated_rows;
        }

        DidMerge(
            to.tables.len() != before_len,
            false, // from is consumed, nothing to report
        )
    }
}

fn get_symbol(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Option<String> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Symbol(s) = node {
            return Some(s.to_string());
        }
    }
    None
}

fn merge_child_tables(
    tables: &mut HashSet<String>,
    egraph: &EGraph<RelLang, RelAnalysis>,
    child_id: Id,
) {
    let canonical = egraph.find(child_id);
    tables.extend(egraph[canonical].data.tables.iter().cloned());
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::to_rec_expr;
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn analysis_tracks_tables_for_scan() {
        let expr = RelExpr::scan("users");
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let runner: Runner<RelLang, RelAnalysis> = Runner::default().with_expr(&rec).run(&[]);
        let root = runner.roots[0];
        let data = &runner.egraph[root].data;
        assert!(data.is_relational);
        assert!(data.tables.contains("users"));
    }

    #[test]
    fn analysis_tracks_tables_through_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            },
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let runner: Runner<RelLang, RelAnalysis> = Runner::default().with_expr(&rec).run(&[]);
        let root = runner.roots[0];
        let data = &runner.egraph[root].data;
        assert!(data.is_relational);
        assert!(data.tables.contains("orders"));
        assert!(data.tables.contains("customers"));
    }

    #[test]
    fn analysis_tracks_tables_through_filter() {
        let expr = RelExpr::scan("products").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("price"))),
            right: Box::new(Expr::Const(Const::Int(100))),
        });
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let runner: Runner<RelLang, RelAnalysis> = Runner::default().with_expr(&rec).run(&[]);
        let root = runner.roots[0];
        let data = &runner.egraph[root].data;
        assert!(data.is_relational);
        assert!(data.tables.contains("products"));
    }
}
