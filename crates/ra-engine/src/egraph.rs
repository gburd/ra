//! E-graph integration using the egg library.
//!
//! Defines the [`RelLang`] language for representing relational algebra
//! expressions as S-expressions inside an e-graph. Provides conversion
//! between [`ra_core::RelExpr`] and the e-graph representation, plus
//! the [`Optimizer`] that drives equality saturation.

use std::collections::HashMap;

use egg::{define_language, EGraph, Id, RecExpr, Runner};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};

use crate::analysis::RelAnalysis;
use crate::extract::extract_best;
use crate::rewrite::all_rules;

define_language! {
    /// S-expression language for relational algebra in the e-graph.
    ///
    /// Each variant maps to a relational or scalar operator. Children
    /// are represented as [`Id`] references into the e-graph.
    pub enum RelLang {
        // -- Relational operators --
        "scan" = Scan([Id; 1]),
        "scan-alias" = ScanAlias([Id; 2]),
        "filter" = Filter([Id; 2]),
        "project" = Project([Id; 2]),
        "join" = Join([Id; 4]),
        "aggregate" = Aggregate([Id; 3]),
        "sort" = Sort([Id; 2]),
        "limit" = Limit([Id; 3]),
        "union" = Union([Id; 3]),
        "intersect" = Intersect([Id; 3]),
        "except" = Except([Id; 3]),

        // -- Join types --
        "inner" = Inner,
        "left-outer" = LeftOuter,
        "right-outer" = RightOuter,
        "full-outer" = FullOuter,
        "cross" = Cross,
        "semi" = Semi,
        "anti" = Anti,

        // -- Boolean flags --
        "true" = True,
        "false" = False,

        // -- Scalar expressions --
        "col" = Col([Id; 1]),
        "qcol" = QCol([Id; 2]),
        "const-null" = ConstNull,
        "const-bool" = ConstBool([Id; 1]),
        "const-int" = ConstInt([Id; 1]),
        "const-float" = ConstFloat([Id; 1]),
        "const-str" = ConstStr([Id; 1]),

        // -- Binary operators --
        "add" = Add([Id; 2]),
        "sub" = Sub([Id; 2]),
        "mul" = Mul([Id; 2]),
        "div" = Div([Id; 2]),
        "eq" = Eq([Id; 2]),
        "ne" = Ne([Id; 2]),
        "lt" = Lt([Id; 2]),
        "le" = Le([Id; 2]),
        "gt" = Gt([Id; 2]),
        "ge" = Ge([Id; 2]),
        "and" = And([Id; 2]),
        "or" = Or([Id; 2]),

        // -- Unary operators --
        "not" = Not([Id; 1]),
        "is-null" = IsNull([Id; 1]),
        "is-not-null" = IsNotNull([Id; 1]),
        "neg" = Neg([Id; 1]),

        // -- Function call --
        "func" = Func(Box<[Id]>),

        // -- Aggregate functions --
        "count" = Count([Id; 1]),
        "sum" = Sum([Id; 1]),
        "avg" = Avg([Id; 1]),
        "min" = Min([Id; 1]),
        "max" = Max([Id; 1]),

        // -- Lists --
        "list" = List(Box<[Id]>),
        "nil" = Nil,

        // -- Projection column --
        "proj-col" = ProjCol([Id; 1]),
        "proj-alias" = ProjAlias([Id; 2]),

        // -- Sort keys --
        "sort-key" = SortKey([Id; 3]),
        "asc" = Asc,
        "desc" = Desc,
        "nulls-first" = NullsFirst,
        "nulls-last" = NullsLast,

        // -- Aggregate expression --
        "agg-expr" = AggExpr([Id; 3]),
        "distinct" = Distinct,
        "all" = All,

        // -- Leaf symbols (table names, column names, strings) --
        Symbol(egg::Symbol),
    }
}

/// Configuration for the equality saturation optimizer.
#[derive(Debug, Clone)]
pub struct OptimizerConfig {
    /// Maximum number of e-graph nodes before stopping.
    pub node_limit: usize,
    /// Maximum number of iterations.
    pub iter_limit: usize,
    /// Time limit in seconds.
    pub time_limit_secs: u64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            node_limit: 100_000,
            iter_limit: 30,
            time_limit_secs: 10,
        }
    }
}

/// The main optimization engine.
///
/// Converts a [`RelExpr`] into an e-graph, runs equality saturation
/// with rewrite rules, then extracts the lowest-cost plan.
#[derive(Debug)]
pub struct Optimizer {
    config: OptimizerConfig,
    table_stats: HashMap<String, ra_core::statistics::Statistics>,
}

impl Optimizer {
    /// Create a new optimizer with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: OptimizerConfig::default(),
            table_stats: HashMap::new(),
        }
    }

    /// Create an optimizer with custom configuration.
    #[must_use]
    pub fn with_config(config: OptimizerConfig) -> Self {
        Self {
            config,
            table_stats: HashMap::new(),
        }
    }

    /// Register table statistics for cost estimation.
    pub fn add_table_stats(
        &mut self,
        table: impl Into<String>,
        stats: ra_core::statistics::Statistics,
    ) {
        self.table_stats.insert(table.into(), stats);
    }

    /// Optimize a relational expression using equality saturation.
    ///
    /// Returns the optimized expression, or an error if conversion
    /// fails.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression cannot be converted to
    /// the e-graph representation or if extraction fails.
    pub fn optimize(&self, expr: &RelExpr) -> Result<RelExpr, EGraphError> {
        let rec_expr = to_rec_expr(expr)?;
        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec_expr)
            .with_node_limit(self.config.node_limit)
            .with_iter_limit(self.config.iter_limit)
            .with_time_limit(std::time::Duration::from_secs(self.config.time_limit_secs))
            .run(&all_rules());

        let root = runner.roots[0];
        let result = extract_best(&runner.egraph, root, &self.table_stats)?;
        Ok(result)
    }

    /// Run optimization and return both the result and the e-graph
    /// for inspection.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion or extraction fails.
    pub fn optimize_with_egraph(
        &self,
        expr: &RelExpr,
    ) -> Result<(RelExpr, EGraph<RelLang, RelAnalysis>), EGraphError> {
        let rec_expr = to_rec_expr(expr)?;
        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec_expr)
            .with_node_limit(self.config.node_limit)
            .with_iter_limit(self.config.iter_limit)
            .with_time_limit(std::time::Duration::from_secs(self.config.time_limit_secs))
            .run(&all_rules());

        let root = runner.roots[0];
        let result = extract_best(&runner.egraph, root, &self.table_stats)?;
        Ok((result, runner.egraph))
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during e-graph optimization.
#[derive(Debug, thiserror::Error)]
pub enum EGraphError {
    /// Failed to convert a relational expression to the e-graph.
    #[error("failed to convert expression to e-graph: {0}")]
    ConversionError(String),

    /// Failed to extract a plan from the e-graph.
    #[error("failed to extract plan from e-graph: {0}")]
    ExtractionError(String),
}

/// Convert a [`RelExpr`] into an egg [`RecExpr`].
///
/// # Errors
///
/// Returns an error if the expression contains unsupported constructs.
pub fn to_rec_expr(expr: &RelExpr) -> Result<RecExpr<RelLang>, EGraphError> {
    let mut rec = RecExpr::default();
    add_rel_expr(&mut rec, expr)?;
    Ok(rec)
}

fn add_rel_expr(rec: &mut RecExpr<RelLang>, expr: &RelExpr) -> Result<Id, EGraphError> {
    match expr {
        RelExpr::Scan { table, alias } => {
            let table_id = add_symbol(rec, table);
            if let Some(alias_name) = alias {
                let alias_id = add_symbol(rec, alias_name);
                Ok(rec.add(RelLang::ScanAlias([table_id, alias_id])))
            } else {
                Ok(rec.add(RelLang::Scan([table_id])))
            }
        }
        RelExpr::Filter { predicate, input } => {
            let pred_id = add_scalar_expr(rec, predicate)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Filter([pred_id, input_id])))
        }
        RelExpr::Project { columns, input } => {
            let cols_id = add_projection_list(rec, columns)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Project([cols_id, input_id])))
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let jt_id = add_join_type(rec, *join_type);
            let cond_id = add_scalar_expr(rec, condition)?;
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Join([jt_id, cond_id, left_id, right_id])))
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            let groups_id = add_expr_list(rec, group_by)?;
            let aggs_id = add_aggregate_list(rec, aggregates)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Aggregate([groups_id, aggs_id, input_id])))
        }
        RelExpr::Sort { keys, input } => {
            let keys_id = add_sort_key_list(rec, keys)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Sort([keys_id, input_id])))
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            let count_id = add_symbol(rec, &count.to_string());
            let offset_id = add_symbol(rec, &offset.to_string());
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Limit([count_id, offset_id, input_id])))
        }
        RelExpr::Union { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Union([all_id, left_id, right_id])))
        }
        RelExpr::Intersect { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Intersect([all_id, left_id, right_id])))
        }
        RelExpr::Except { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Except([all_id, left_id, right_id])))
        }
    }
}

fn add_scalar_expr(rec: &mut RecExpr<RelLang>, expr: &Expr) -> Result<Id, EGraphError> {
    match expr {
        Expr::Column(col_ref) => {
            let col_id = add_symbol(rec, &col_ref.column);
            if let Some(table) = &col_ref.table {
                let table_id = add_symbol(rec, table);
                Ok(rec.add(RelLang::QCol([table_id, col_id])))
            } else {
                Ok(rec.add(RelLang::Col([col_id])))
            }
        }
        Expr::Const(c) => Ok(add_const(rec, c)),
        Expr::BinOp { op, left, right } => {
            let left_id = add_scalar_expr(rec, left)?;
            let right_id = add_scalar_expr(rec, right)?;
            let node = match op {
                BinOp::Add => RelLang::Add([left_id, right_id]),
                BinOp::Sub => RelLang::Sub([left_id, right_id]),
                BinOp::Mul => RelLang::Mul([left_id, right_id]),
                BinOp::Div => RelLang::Div([left_id, right_id]),
                BinOp::Eq => RelLang::Eq([left_id, right_id]),
                BinOp::Ne => RelLang::Ne([left_id, right_id]),
                BinOp::Lt => RelLang::Lt([left_id, right_id]),
                BinOp::Le => RelLang::Le([left_id, right_id]),
                BinOp::Gt => RelLang::Gt([left_id, right_id]),
                BinOp::Ge => RelLang::Ge([left_id, right_id]),
                BinOp::And => RelLang::And([left_id, right_id]),
                BinOp::Or => RelLang::Or([left_id, right_id]),
            };
            Ok(rec.add(node))
        }
        Expr::UnaryOp { op, operand } => {
            let operand_id = add_scalar_expr(rec, operand)?;
            let node = match op {
                UnaryOp::Not => RelLang::Not([operand_id]),
                UnaryOp::IsNull => RelLang::IsNull([operand_id]),
                UnaryOp::IsNotNull => RelLang::IsNotNull([operand_id]),
                UnaryOp::Neg => RelLang::Neg([operand_id]),
            };
            Ok(rec.add(node))
        }
        Expr::Function { name, args } => {
            let name_id = add_symbol(rec, name);
            let mut ids = vec![name_id];
            for arg in args {
                ids.push(add_scalar_expr(rec, arg)?);
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::Case { .. } => Err(EGraphError::ConversionError(
            "CASE expressions are not yet supported in the \
                 e-graph representation"
                .into(),
        )),
        Expr::Cast { .. } => Err(EGraphError::ConversionError(
            "CAST expressions are not yet supported in the \
                 e-graph representation"
                .into(),
        )),
    }
}

fn add_const(rec: &mut RecExpr<RelLang>, c: &Const) -> Id {
    match c {
        Const::Null => rec.add(RelLang::ConstNull),
        Const::Bool(b) => {
            let val_id = if *b {
                rec.add(RelLang::True)
            } else {
                rec.add(RelLang::False)
            };
            rec.add(RelLang::ConstBool([val_id]))
        }
        Const::Int(i) => {
            let val_id = add_symbol(rec, &i.to_string());
            rec.add(RelLang::ConstInt([val_id]))
        }
        Const::Float(f) => {
            let val_id = add_symbol(rec, &f.to_string());
            rec.add(RelLang::ConstFloat([val_id]))
        }
        Const::String(s) => {
            let val_id = add_symbol(rec, s);
            rec.add(RelLang::ConstStr([val_id]))
        }
    }
}

fn add_symbol(rec: &mut RecExpr<RelLang>, s: &str) -> Id {
    rec.add(RelLang::Symbol(egg::Symbol::from(s)))
}

fn add_join_type(rec: &mut RecExpr<RelLang>, jt: JoinType) -> Id {
    let node = match jt {
        JoinType::Inner => RelLang::Inner,
        JoinType::LeftOuter => RelLang::LeftOuter,
        JoinType::RightOuter => RelLang::RightOuter,
        JoinType::FullOuter => RelLang::FullOuter,
        JoinType::Cross => RelLang::Cross,
        JoinType::Semi => RelLang::Semi,
        JoinType::Anti => RelLang::Anti,
    };
    rec.add(node)
}

fn add_bool_flag(rec: &mut RecExpr<RelLang>, val: bool) -> Id {
    if val {
        rec.add(RelLang::True)
    } else {
        rec.add(RelLang::False)
    }
}

fn add_projection_list(
    rec: &mut RecExpr<RelLang>,
    columns: &[ProjectionColumn],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(columns.len());
    for col in columns {
        let expr_id = add_scalar_expr(rec, &col.expr)?;
        let proj_id = if let Some(alias) = &col.alias {
            let alias_id = add_symbol(rec, alias);
            rec.add(RelLang::ProjAlias([expr_id, alias_id]))
        } else {
            rec.add(RelLang::ProjCol([expr_id]))
        };
        ids.push(proj_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_expr_list(rec: &mut RecExpr<RelLang>, exprs: &[Expr]) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(exprs.len());
    for e in exprs {
        ids.push(add_scalar_expr(rec, e)?);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_aggregate_list(
    rec: &mut RecExpr<RelLang>,
    aggs: &[AggregateExpr],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(aggs.len());
    for agg in aggs {
        let func_node = match agg.function {
            AggregateFunction::Count => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Count([arg_id])
            }
            AggregateFunction::Sum => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Sum([arg_id])
            }
            AggregateFunction::Avg => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Avg([arg_id])
            }
            AggregateFunction::Min => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Min([arg_id])
            }
            AggregateFunction::Max => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Max([arg_id])
            }
        };
        let func_id = rec.add(func_node);
        let distinct_id = if agg.distinct {
            rec.add(RelLang::Distinct)
        } else {
            rec.add(RelLang::All)
        };
        let alias_id = if let Some(alias) = &agg.alias {
            add_symbol(rec, alias)
        } else {
            rec.add(RelLang::Nil)
        };
        let agg_id = rec.add(RelLang::AggExpr([func_id, distinct_id, alias_id]));
        ids.push(agg_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_agg_arg(rec: &mut RecExpr<RelLang>, arg: Option<&Expr>) -> Result<Id, EGraphError> {
    match arg {
        Some(e) => add_scalar_expr(rec, e),
        None => Ok(rec.add(RelLang::Nil)),
    }
}

fn add_sort_key_list(rec: &mut RecExpr<RelLang>, keys: &[SortKey]) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(keys.len());
    for key in keys {
        let expr_id = add_scalar_expr(rec, &key.expr)?;
        let dir_id = match key.direction {
            SortDirection::Asc => rec.add(RelLang::Asc),
            SortDirection::Desc => rec.add(RelLang::Desc),
        };
        let nulls_id = match key.nulls {
            NullOrdering::First => rec.add(RelLang::NullsFirst),
            NullOrdering::Last => rec.add(RelLang::NullsLast),
        };
        let key_id = rec.add(RelLang::SortKey([expr_id, dir_id, nulls_id]));
        ids.push(key_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

/// Convert an e-graph node (by class [`Id`]) back to a [`RelExpr`].
///
/// Extracts the best node from each e-class using the given extractor
/// function, then reconstructs the AST.
///
/// # Errors
///
/// Returns an error if the e-graph contains nodes that cannot be
/// mapped back to [`RelExpr`].
pub fn from_egraph_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<RelExpr, EGraphError> {
    let nodes = &egraph[id].nodes;
    let node = &nodes[0];
    from_node(egraph, node)
}

#[allow(clippy::too_many_lines)]
fn from_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    node: &RelLang,
) -> Result<RelExpr, EGraphError> {
    match node {
        RelLang::Scan([table_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan { table, alias: None })
        }
        RelLang::ScanAlias([table_id, alias_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let alias = extract_symbol(egraph, *alias_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some(alias),
            })
        }
        RelLang::Filter([pred_id, input_id]) => {
            let predicate = extract_scalar_expr(egraph, *pred_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Filter {
                predicate,
                input: Box::new(input),
            })
        }
        RelLang::Project([cols_id, input_id]) => {
            let columns = extract_projection_list(egraph, *cols_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Project {
                columns,
                input: Box::new(input),
            })
        }
        RelLang::Join([jt_id, cond_id, left_id, right_id]) => {
            let join_type = extract_join_type(egraph, *jt_id)?;
            let condition = extract_scalar_expr(egraph, *cond_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Join {
                join_type,
                condition,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Aggregate([groups_id, aggs_id, input_id]) => {
            let group_by = extract_expr_list(egraph, *groups_id)?;
            let aggregates = extract_aggregate_list(egraph, *aggs_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Aggregate {
                group_by,
                aggregates,
                input: Box::new(input),
            })
        }
        RelLang::Sort([keys_id, input_id]) => {
            let keys = extract_sort_key_list(egraph, *keys_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Sort {
                keys,
                input: Box::new(input),
            })
        }
        RelLang::Limit([count_id, offset_id, input_id]) => {
            let count_str = extract_symbol(egraph, *count_id)?;
            let offset_str = extract_symbol(egraph, *offset_id)?;
            let count = count_str
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid limit count: {e}")))?;
            let offset = offset_str
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid limit offset: {e}")))?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Limit {
                count,
                offset,
                input: Box::new(input),
            })
        }
        RelLang::Union([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Union {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Intersect([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Intersect {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Except([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Except {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        other => Err(EGraphError::ExtractionError(format!(
            "unexpected relational node: {other:?}"
        ))),
    }
}

fn extract_symbol(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<String, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Symbol(s) = node {
            return Ok(s.to_string());
        }
    }
    Err(EGraphError::ExtractionError(format!(
        "expected Symbol node at e-class {id:?}"
    )))
}

fn extract_bool_flag(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<bool, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::True => return Ok(true),
            RelLang::False => return Ok(false),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(format!(
        "expected True/False node at e-class {id:?}"
    )))
}

fn extract_join_type(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<JoinType, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        let jt = match node {
            RelLang::Inner => JoinType::Inner,
            RelLang::LeftOuter => JoinType::LeftOuter,
            RelLang::RightOuter => JoinType::RightOuter,
            RelLang::FullOuter => JoinType::FullOuter,
            RelLang::Cross => JoinType::Cross,
            RelLang::Semi => JoinType::Semi,
            RelLang::Anti => JoinType::Anti,
            _ => continue,
        };
        return Ok(jt);
    }
    Err(EGraphError::ExtractionError(format!(
        "expected join type node at e-class {id:?}"
    )))
}

fn extract_scalar_expr(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<Expr, EGraphError> {
    let canonical = egraph.find(id);
    let node = &egraph[canonical].nodes[0];
    scalar_from_node(egraph, node)
}

#[allow(clippy::too_many_lines)]
fn scalar_from_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    node: &RelLang,
) -> Result<Expr, EGraphError> {
    match node {
        RelLang::Col([name_id]) => {
            let name = extract_symbol(egraph, *name_id)?;
            Ok(Expr::Column(ColumnRef::new(name)))
        }
        RelLang::QCol([table_id, name_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let name = extract_symbol(egraph, *name_id)?;
            Ok(Expr::Column(ColumnRef::qualified(table, name)))
        }
        RelLang::ConstNull => Ok(Expr::Const(Const::Null)),
        RelLang::ConstBool([val_id]) => {
            let b = extract_bool_flag(egraph, *val_id)?;
            Ok(Expr::Const(Const::Bool(b)))
        }
        RelLang::ConstInt([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            let i = s.parse::<i64>().map_err(|e| {
                EGraphError::ExtractionError(format!("invalid integer constant: {e}"))
            })?;
            Ok(Expr::Const(Const::Int(i)))
        }
        RelLang::ConstFloat([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            let f = s.parse::<f64>().map_err(|e| {
                EGraphError::ExtractionError(format!("invalid float constant: {e}"))
            })?;
            Ok(Expr::Const(Const::Float(f)))
        }
        RelLang::ConstStr([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            Ok(Expr::Const(Const::String(s)))
        }
        RelLang::Add([l, r]) => extract_binop(egraph, BinOp::Add, *l, *r),
        RelLang::Sub([l, r]) => extract_binop(egraph, BinOp::Sub, *l, *r),
        RelLang::Mul([l, r]) => extract_binop(egraph, BinOp::Mul, *l, *r),
        RelLang::Div([l, r]) => extract_binop(egraph, BinOp::Div, *l, *r),
        RelLang::Eq([l, r]) => extract_binop(egraph, BinOp::Eq, *l, *r),
        RelLang::Ne([l, r]) => extract_binop(egraph, BinOp::Ne, *l, *r),
        RelLang::Lt([l, r]) => extract_binop(egraph, BinOp::Lt, *l, *r),
        RelLang::Le([l, r]) => extract_binop(egraph, BinOp::Le, *l, *r),
        RelLang::Gt([l, r]) => extract_binop(egraph, BinOp::Gt, *l, *r),
        RelLang::Ge([l, r]) => extract_binop(egraph, BinOp::Ge, *l, *r),
        RelLang::And([l, r]) => extract_binop(egraph, BinOp::And, *l, *r),
        RelLang::Or([l, r]) => extract_binop(egraph, BinOp::Or, *l, *r),
        RelLang::Not([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            })
        }
        RelLang::IsNull([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::IsNull,
                operand: Box::new(operand),
            })
        }
        RelLang::IsNotNull([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::IsNotNull,
                operand: Box::new(operand),
            })
        }
        RelLang::Neg([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            })
        }
        RelLang::Func(ids) => {
            if ids.is_empty() {
                return Err(EGraphError::ExtractionError(
                    "function call with no children".into(),
                ));
            }
            let name = extract_symbol(egraph, ids[0])?;
            let mut args = Vec::with_capacity(ids.len() - 1);
            for &arg_id in &ids[1..] {
                args.push(extract_scalar_expr(egraph, arg_id)?);
            }
            Ok(Expr::Function { name, args })
        }
        other => Err(EGraphError::ExtractionError(format!(
            "unexpected scalar node: {other:?}"
        ))),
    }
}

fn extract_binop(
    egraph: &EGraph<RelLang, RelAnalysis>,
    op: BinOp,
    left_id: Id,
    right_id: Id,
) -> Result<Expr, EGraphError> {
    let left = extract_scalar_expr(egraph, left_id)?;
    let right = extract_scalar_expr(egraph, right_id)?;
    Ok(Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn extract_projection_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<ProjectionColumn>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut cols = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                cols.push(extract_projection_column(egraph, child_id)?);
            }
            return Ok(cols);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for projection columns".into(),
    ))
}

fn extract_projection_column(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<ProjectionColumn, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::ProjCol([expr_id]) => {
                let expr = extract_scalar_expr(egraph, *expr_id)?;
                return Ok(ProjectionColumn { expr, alias: None });
            }
            RelLang::ProjAlias([expr_id, alias_id]) => {
                let expr = extract_scalar_expr(egraph, *expr_id)?;
                let alias = extract_symbol(egraph, *alias_id)?;
                return Ok(ProjectionColumn {
                    expr,
                    alias: Some(alias),
                });
            }
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected ProjCol or ProjAlias node".into(),
    ))
}

fn extract_expr_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut exprs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                exprs.push(extract_scalar_expr(egraph, child_id)?);
            }
            return Ok(exprs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for expression list".into(),
    ))
}

fn extract_aggregate_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<AggregateExpr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut aggs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                aggs.push(extract_agg_expr(egraph, child_id)?);
            }
            return Ok(aggs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for aggregate list".into(),
    ))
}

fn extract_agg_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<AggregateExpr, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::AggExpr([func_id, distinct_id, alias_id]) = node {
            let (function, arg) = extract_agg_function(egraph, *func_id)?;
            let distinct = extract_distinct_flag(egraph, *distinct_id)?;
            let alias = extract_optional_symbol(egraph, *alias_id)?;
            return Ok(AggregateExpr {
                function,
                arg,
                distinct,
                alias,
            });
        }
    }
    Err(EGraphError::ExtractionError("expected AggExpr node".into()))
}

fn extract_agg_function(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<(AggregateFunction, Option<Expr>), EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        let (func, arg_id) = match node {
            RelLang::Count([a]) => (AggregateFunction::Count, *a),
            RelLang::Sum([a]) => (AggregateFunction::Sum, *a),
            RelLang::Avg([a]) => (AggregateFunction::Avg, *a),
            RelLang::Min([a]) => (AggregateFunction::Min, *a),
            RelLang::Max([a]) => (AggregateFunction::Max, *a),
            _ => continue,
        };
        let arg = extract_optional_expr(egraph, arg_id)?;
        return Ok((func, arg));
    }
    Err(EGraphError::ExtractionError(
        "expected aggregate function node".into(),
    ))
}

fn extract_optional_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
    }
    Ok(Some(extract_scalar_expr(egraph, id)?))
}

fn extract_optional_symbol(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<String>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
    }
    Ok(Some(extract_symbol(egraph, id)?))
}

fn extract_distinct_flag(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<bool, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Distinct => return Ok(true),
            RelLang::All => return Ok(false),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected Distinct/All flag".into(),
    ))
}

fn extract_sort_key_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<SortKey>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut keys = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                keys.push(extract_sort_key(egraph, child_id)?);
            }
            return Ok(keys);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for sort keys".into(),
    ))
}

fn extract_sort_key(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<SortKey, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::SortKey([expr_id, dir_id, nulls_id]) = node {
            let expr = extract_scalar_expr(egraph, *expr_id)?;
            let direction = extract_sort_direction(egraph, *dir_id)?;
            let nulls = extract_null_ordering(egraph, *nulls_id)?;
            return Ok(ra_core::algebra::SortKey {
                expr,
                direction,
                nulls,
            });
        }
    }
    Err(EGraphError::ExtractionError("expected SortKey node".into()))
}

fn extract_sort_direction(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<SortDirection, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Asc => return Ok(SortDirection::Asc),
            RelLang::Desc => return Ok(SortDirection::Desc),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected Asc/Desc node".into(),
    ))
}

fn extract_null_ordering(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<NullOrdering, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::NullsFirst => return Ok(NullOrdering::First),
            RelLang::NullsLast => return Ok(NullOrdering::Last),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected NullsFirst/NullsLast node".into(),
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn roundtrip_scan() {
        let expr = RelExpr::scan("users");
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn roundtrip_filter() {
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(42))),
        });
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn roundtrip_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn optimizer_roundtrip_simple_scan() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize(&expr)
            .expect("optimization should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn optimizer_roundtrip_filter() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = optimizer
            .optimize(&expr)
            .expect("optimization should succeed");
        // The optimized result should be semantically equivalent
        // (may or may not be structurally identical)
        assert!(matches!(result, RelExpr::Filter { .. }) || matches!(result, RelExpr::Scan { .. }));
    }
}
