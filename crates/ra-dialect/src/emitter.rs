//! SQL emitter: converts `RelExpr` trees to dialect-specific SQL strings.
//!
//! The emitter walks a [`RelExpr`] tree and produces a SQL string
//! appropriate for the target [`Dialect`]. It handles dialect-specific
//! differences in syntax, function names, identifier quoting, boolean
//! literals, LIMIT/OFFSET, and string concatenation.

use std::fmt::Write;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey, WindowExpr, WindowFrame, WindowFrameBound, WindowFrameMode,
    WindowFunction,
};
use ra_core::expr::{BinOp, Const, Expr, SubQueryType, UnaryOp};

use crate::dialect::Dialect;
use crate::error::{TranslationError, TranslationWarning, WarningSeverity};
use crate::functions::build_function_map;

/// Result of emitting SQL from a `RelExpr`.
#[derive(Debug)]
pub struct EmitResult {
    /// The emitted SQL string.
    pub sql: String,
    /// Warnings generated during emission.
    pub warnings: Vec<TranslationWarning>,
}

/// Emit dialect-specific SQL from a `RelExpr`.
///
/// # Errors
///
/// Returns `TranslationError` if the expression contains constructs
/// that cannot be represented in the target dialect.
pub fn emit_sql(expr: &RelExpr, target: Dialect) -> Result<EmitResult, TranslationError> {
    let mut ctx = EmitContext::new(target);
    let sql = ctx.emit_rel_expr(expr)?;
    Ok(EmitResult {
        sql,
        warnings: ctx.warnings,
    })
}

struct EmitContext {
    target: Dialect,
    warnings: Vec<TranslationWarning>,
    function_map: std::collections::HashMap<String, crate::functions::FunctionMapping>,
}

impl EmitContext {
    fn new(target: Dialect) -> Self {
        Self {
            target,
            warnings: Vec::new(),
            function_map: build_function_map(target),
        }
    }

    fn emit_rel_expr(&mut self, expr: &RelExpr) -> Result<String, TranslationError> {
        match expr {
            RelExpr::Scan { table, alias } => Ok(self.emit_scan(table, alias.as_ref())),
            RelExpr::Filter { predicate, input } => self.emit_filter(predicate, input),
            RelExpr::Project { columns, input } => self.emit_project(columns, input),
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => self.emit_join(*join_type, condition, left, right),
            RelExpr::Aggregate {
                group_by,
                aggregates,
                input,
            } => self.emit_aggregate(group_by, aggregates, input),
            RelExpr::Sort { keys, input } => self.emit_sort(keys, input),
            RelExpr::Limit {
                count,
                offset,
                input,
            } => self.emit_limit(*count, *offset, input),
            RelExpr::Union { all, left, right } => self.emit_set_op("UNION", *all, left, right),
            RelExpr::Intersect { all, left, right } => {
                self.emit_set_op("INTERSECT", *all, left, right)
            }
            RelExpr::Except { all, left, right } => {
                let op = if self.target == Dialect::Oracle {
                    "MINUS"
                } else {
                    "EXCEPT"
                };
                self.emit_set_op(op, *all, left, right)
            }
            RelExpr::CTE {
                name,
                definition,
                body,
            } => self.emit_cte(name, definition, body),
            RelExpr::RecursiveCTE {
                name,
                base_case,
                recursive_case,
                body,
                ..
            } => self.emit_recursive_cte(name, base_case, recursive_case, body),
            RelExpr::Window { functions, input } => self.emit_window(functions, input),
            RelExpr::Distinct { input } => self.emit_distinct(input),
            RelExpr::Values { rows } => self.emit_values(rows),
            other => Err(TranslationError::UnsupportedStatement(format!("{other:?}"))),
        }
    }

    fn emit_scan(&self, table: &str, alias: Option<&String>) -> String {
        let quoted = self.quote_ident(table);
        let mut sql = format!("SELECT * FROM {quoted}");
        if let Some(a) = alias {
            let _ = write!(sql, " AS {}", self.quote_ident(a));
        }
        sql
    }

    fn emit_filter(
        &mut self,
        predicate: &Expr,
        input: &RelExpr,
    ) -> Result<String, TranslationError> {
        let inner = self.emit_as_source(input)?;
        let pred = self.emit_expr(predicate)?;
        Ok(format!("SELECT * FROM ({inner}) AS _t WHERE {pred}"))
    }

    fn emit_project(
        &mut self,
        columns: &[ProjectionColumn],
        input: &RelExpr,
    ) -> Result<String, TranslationError> {
        let col_list = self.emit_projection_list(columns)?;
        let source = self.emit_as_source(input)?;
        Ok(format!("SELECT {col_list} FROM ({source}) AS _t"))
    }

    fn emit_join(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<String, TranslationError> {
        let left_sql = self.emit_as_source(left)?;
        let right_sql = self.emit_as_source(right)?;
        let cond = self.emit_expr(condition)?;
        let join_kw = match join_type {
            JoinType::Inner | JoinType::Semi => "JOIN",
            JoinType::LeftOuter | JoinType::Anti => "LEFT JOIN",
            JoinType::RightOuter => "RIGHT JOIN",
            JoinType::FullOuter => {
                if !self.target.supports_full_outer_join() {
                    return Err(TranslationError::UnsupportedFeature {
                        dialect: self.target,
                        feature: "FULL OUTER JOIN".to_string(),
                    });
                }
                "FULL OUTER JOIN"
            }
            JoinType::Cross => {
                return Ok(format!(
                    "SELECT * FROM ({left_sql}) AS _l \
                     CROSS JOIN ({right_sql}) AS _r"
                ));
            }
        };
        Ok(format!(
            "SELECT * FROM ({left_sql}) AS _l \
             {join_kw} ({right_sql}) AS _r ON {cond}"
        ))
    }

    fn emit_aggregate(
        &mut self,
        group_by: &[Expr],
        aggregates: &[AggregateExpr],
        input: &RelExpr,
    ) -> Result<String, TranslationError> {
        let source = self.emit_as_source(input)?;

        let mut select_items = Vec::new();
        for g in group_by {
            select_items.push(self.emit_expr(g)?);
        }
        for agg in aggregates {
            select_items.push(self.emit_aggregate_expr(agg)?);
        }

        let select_list = select_items.join(", ");
        let mut sql = format!("SELECT {select_list} FROM ({source}) AS _t");

        if !group_by.is_empty() {
            let group_list: Result<Vec<String>, _> =
                group_by.iter().map(|g| self.emit_expr(g)).collect();
            let group_list = group_list?.join(", ");
            let _ = write!(sql, " GROUP BY {group_list}");
        }

        Ok(sql)
    }

    fn emit_sort(&mut self, keys: &[SortKey], input: &RelExpr) -> Result<String, TranslationError> {
        let inner = self.emit_as_source(input)?;
        let order_list = self.emit_sort_keys(keys)?;
        Ok(format!(
            "SELECT * FROM ({inner}) AS _t ORDER BY {order_list}"
        ))
    }

    fn emit_limit(
        &mut self,
        count: u64,
        offset: u64,
        input: &RelExpr,
    ) -> Result<String, TranslationError> {
        let inner = self.emit_as_source(input)?;
        let mut sql = format!("SELECT * FROM ({inner}) AS _t");

        if self.target.supports_limit() {
            if offset > 0 {
                let _ = write!(sql, " LIMIT {count} OFFSET {offset}");
            } else {
                let _ = write!(sql, " LIMIT {count}");
            }
        } else if self.target.supports_fetch() {
            if offset > 0 {
                let _ = write!(sql, " OFFSET {offset} ROWS");
            }
            let _ = write!(sql, " FETCH FIRST {count} ROWS ONLY");
            self.warnings.push(TranslationWarning {
                severity: WarningSeverity::Info,
                message: format!("LIMIT translated to FETCH for {}", self.target),
                hint: Some("FETCH requires ORDER BY in MSSQL".into()),
            });
        } else {
            let _ = write!(sql, " LIMIT {count}");
        }

        Ok(sql)
    }

    fn emit_set_op(
        &mut self,
        op: &str,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<String, TranslationError> {
        let left_sql = self.emit_rel_expr(left)?;
        let right_sql = self.emit_rel_expr(right)?;
        let quantifier = if all { " ALL" } else { "" };
        Ok(format!("{left_sql} {op}{quantifier} {right_sql}"))
    }

    fn emit_cte(
        &mut self,
        name: &str,
        definition: &RelExpr,
        body: &RelExpr,
    ) -> Result<String, TranslationError> {
        let def_sql = self.emit_rel_expr(definition)?;
        let body_sql = self.emit_rel_expr(body)?;
        let qname = self.quote_ident(name);
        Ok(format!("WITH {qname} AS ({def_sql}) {body_sql}"))
    }

    fn emit_recursive_cte(
        &mut self,
        name: &str,
        base_case: &RelExpr,
        recursive_case: &RelExpr,
        body: &RelExpr,
    ) -> Result<String, TranslationError> {
        let base_sql = self.emit_rel_expr(base_case)?;
        let rec_sql = self.emit_rel_expr(recursive_case)?;
        let body_sql = self.emit_rel_expr(body)?;
        let qname = self.quote_ident(name);
        Ok(format!(
            "WITH RECURSIVE {qname} AS \
             ({base_sql} UNION ALL {rec_sql}) {body_sql}"
        ))
    }

    fn emit_window(
        &mut self,
        functions: &[WindowExpr],
        input: &RelExpr,
    ) -> Result<String, TranslationError> {
        let source = self.emit_as_source(input)?;
        let mut items = vec!["*".to_string()];
        for wf in functions {
            items.push(self.emit_window_expr(wf)?);
        }
        let select_list = items.join(", ");
        Ok(format!("SELECT {select_list} FROM ({source}) AS _t"))
    }

    fn emit_distinct(&mut self, input: &RelExpr) -> Result<String, TranslationError> {
        let inner = self.emit_as_source(input)?;
        Ok(format!("SELECT DISTINCT * FROM ({inner}) AS _t"))
    }

    fn emit_values(&mut self, rows: &[Vec<Expr>]) -> Result<String, TranslationError> {
        let mut row_strs = Vec::new();
        for row in rows {
            let vals: Result<Vec<String>, _> = row.iter().map(|e| self.emit_expr(e)).collect();
            let vals = vals?.join(", ");
            row_strs.push(format!("({vals})"));
        }
        Ok(format!("VALUES {}", row_strs.join(", ")))
    }

    // ---- Expression emission ----

    fn emit_expr(&mut self, expr: &Expr) -> Result<String, TranslationError> {
        match expr {
            Expr::Column(col_ref) => Ok(self.emit_column_ref(col_ref)),
            Expr::Const(c) => Ok(self.emit_const(c)),
            Expr::BinOp { op, left, right } => self.emit_binop(*op, left, right),
            Expr::UnaryOp { op, operand } => self.emit_unary(*op, operand),
            Expr::Function { name, args } => self.emit_function(name, args),
            Expr::Case {
                operand,
                when_clauses,
                else_result,
            } => self.emit_case(operand.as_deref(), when_clauses, else_result.as_deref()),
            Expr::Cast { expr, target_type } => self.emit_cast(expr, target_type),
            Expr::Array(elems) => {
                let items: Result<Vec<String>, _> =
                    elems.iter().map(|e| self.emit_expr(e)).collect();
                Ok(format!("ARRAY[{}]", items?.join(", ")))
            }
            Expr::SubQuery {
                subquery_type,
                query,
                test_expr,
            } => self.emit_subquery(subquery_type, query, test_expr.as_deref()),
            other => Err(TranslationError::UnsupportedStatement(format!(
                "unsupported expression: {other:?}"
            ))),
        }
    }

    fn emit_column_ref(&self, col_ref: &ra_core::expr::ColumnRef) -> String {
        if let Some(table) = &col_ref.table {
            format!(
                "{}.{}",
                self.quote_ident(table),
                self.quote_ident(&col_ref.column)
            )
        } else if col_ref.column == "*" {
            "*".to_string()
        } else {
            self.quote_ident(&col_ref.column)
        }
    }

    fn emit_const(&mut self, c: &Const) -> String {
        match c {
            Const::Null => "NULL".to_string(),
            Const::Bool(b) => {
                if self.target.supports_boolean_literals() {
                    if *b {
                        "TRUE".to_string()
                    } else {
                        "FALSE".to_string()
                    }
                } else {
                    let val = i32::from(*b);
                    self.warnings.push(TranslationWarning {
                        severity: WarningSeverity::Info,
                        message: format!(
                            "Boolean literal translated to \
                             {val} for {}",
                            self.target
                        ),
                        hint: None,
                    });
                    val.to_string()
                }
            }
            Const::Int(i) => i.to_string(),
            Const::Float(f) => f.to_string(),
            Const::String(s) => format!("'{}'", s.replace('\'', "''")),
        }
    }

    fn emit_binop(
        &mut self,
        op: BinOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<String, TranslationError> {
        if op == BinOp::Concat {
            return self.emit_concat(left, right);
        }
        let l = self.emit_expr(left)?;
        let r = self.emit_expr(right)?;
        let op_str = op.to_string();
        Ok(format!("({l} {op_str} {r})"))
    }

    fn emit_concat(&mut self, left: &Expr, right: &Expr) -> Result<String, TranslationError> {
        let l = self.emit_expr(left)?;
        let r = self.emit_expr(right)?;

        match self.target {
            Dialect::MsSql => {
                self.warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: "|| translated to + for MSSQL \
                         string concatenation"
                        .into(),
                    hint: None,
                });
                Ok(format!("({l} + {r})"))
            }
            Dialect::MySql => {
                self.warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: "|| translated to CONCAT() for MySQL".into(),
                    hint: None,
                });
                Ok(format!("CONCAT({l}, {r})"))
            }
            _ => Ok(format!("({l} || {r})")),
        }
    }

    fn emit_unary(&mut self, op: UnaryOp, operand: &Expr) -> Result<String, TranslationError> {
        let inner = self.emit_expr(operand)?;
        match op {
            UnaryOp::Not => Ok(format!("NOT ({inner})")),
            UnaryOp::IsNull => Ok(format!("({inner}) IS NULL")),
            UnaryOp::IsNotNull => Ok(format!("({inner}) IS NOT NULL")),
            UnaryOp::Neg => Ok(format!("-({inner})")),
        }
    }

    fn emit_function(&mut self, name: &str, args: &[Expr]) -> Result<String, TranslationError> {
        let upper_name = name.to_uppercase();

        // Handle parser-produced special function names
        if let Some(result) = self.emit_special_function(&upper_name, args)? {
            return Ok(result);
        }

        let target_name = self.function_map.get(&upper_name).map_or_else(
            || upper_name.clone(),
            |mapping| {
                if mapping.target_name != upper_name {
                    self.warnings.push(TranslationWarning {
                        severity: WarningSeverity::Info,
                        message: format!(
                            "Function {upper_name} \
                                     translated to {}",
                            mapping.target_name
                        ),
                        hint: None,
                    });
                }
                mapping.target_name.clone()
            },
        );

        let arg_strs: Result<Vec<String>, _> = args.iter().map(|a| self.emit_expr(a)).collect();
        let arg_list = arg_strs?.join(", ");

        Ok(format!("{target_name}({arg_list})"))
    }

    /// Handle special function names produced by `ra-parser`
    /// for SQL constructs that map to operators or keywords
    /// rather than real functions.
    fn emit_special_function(
        &mut self,
        name: &str,
        args: &[Expr],
    ) -> Result<Option<String>, TranslationError> {
        match name {
            // ra-parser converts || to OP_STRINGCONCAT
            "OP_STRINGCONCAT" if args.len() == 2 => {
                let result = self.emit_concat(&args[0], &args[1])?;
                Ok(Some(result))
            }
            // ra-parser converts Modulo to OP_MODULO
            "OP_MODULO" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} % {r})")))
            }
            // ILIKE operator
            "ILIKE" if args.len() == 2 => {
                let result = self.emit_ilike(&args[0], &args[1])?;
                Ok(Some(result))
            }
            // LIKE operator
            "LIKE" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} LIKE {r})")))
            }
            // IN (value list)
            "IN_LIST" if args.len() >= 2 => {
                let target = self.emit_expr(&args[0])?;
                let vals: Result<Vec<String>, _> =
                    args[1..].iter().map(|a| self.emit_expr(a)).collect();
                let list = vals?.join(", ");
                Ok(Some(format!("{target} IN ({list})")))
            }
            "NOT_IN_LIST" if args.len() >= 2 => {
                let target = self.emit_expr(&args[0])?;
                let vals: Result<Vec<String>, _> =
                    args[1..].iter().map(|a| self.emit_expr(a)).collect();
                let list = vals?.join(", ");
                Ok(Some(format!("{target} NOT IN ({list})")))
            }
            // IN (subquery) -- parser wraps as Function
            "IN_SUBQUERY" if args.len() == 1 => {
                let target = self.emit_expr(&args[0])?;
                Ok(Some(format!("{target} IN (SELECT ...)")))
            }
            // EXISTS (subquery) -- parser wraps as Function
            "EXISTS" if args.is_empty() => Ok(Some("EXISTS (SELECT ...)".to_string())),
            // BETWEEN
            "BETWEEN" if args.len() == 3 => {
                let e = self.emit_expr(&args[0])?;
                let lo = self.emit_expr(&args[1])?;
                let hi = self.emit_expr(&args[2])?;
                Ok(Some(format!("{e} BETWEEN {lo} AND {hi}")))
            }
            _ => Ok(None),
        }
    }

    fn emit_ilike(&mut self, expr: &Expr, pattern: &Expr) -> Result<String, TranslationError> {
        let e = self.emit_expr(expr)?;
        let p = self.emit_expr(pattern)?;

        if self.target.supports_ilike() {
            return Ok(format!("({e} ILIKE {p})"));
        }

        self.warnings.push(TranslationWarning {
            severity: WarningSeverity::Info,
            message: format!("ILIKE translated to LOWER() + LIKE for {}", self.target),
            hint: Some(
                "LOWER() may not handle all Unicode case \
                 folding correctly"
                    .into(),
            ),
        });
        Ok(format!("(LOWER({e}) LIKE LOWER({p}))"))
    }

    fn emit_case(
        &mut self,
        operand: Option<&Expr>,
        when_clauses: &[(Expr, Expr)],
        else_result: Option<&Expr>,
    ) -> Result<String, TranslationError> {
        let mut sql = String::from("CASE");

        if let Some(op) = operand {
            let op_str = self.emit_expr(op)?;
            let _ = write!(sql, " {op_str}");
        }

        for (cond, result) in when_clauses {
            let c = self.emit_expr(cond)?;
            let r = self.emit_expr(result)?;
            let _ = write!(sql, " WHEN {c} THEN {r}");
        }

        if let Some(el) = else_result {
            let e = self.emit_expr(el)?;
            let _ = write!(sql, " ELSE {e}");
        }

        sql.push_str(" END");
        Ok(sql)
    }

    fn emit_cast(&mut self, expr: &Expr, target_type: &str) -> Result<String, TranslationError> {
        let inner = self.emit_expr(expr)?;
        if self.target.supports_double_colon_cast() {
            Ok(format!("({inner})::{target_type}"))
        } else {
            Ok(format!("CAST({inner} AS {target_type})"))
        }
    }

    fn emit_subquery(
        &mut self,
        subquery_type: &SubQueryType,
        query: &ra_core::algebra::RelExpr,
        test_expr: Option<&Expr>,
    ) -> Result<String, TranslationError> {
        let inner = self.emit_rel_expr(query)?;
        match subquery_type {
            SubQueryType::Scalar => Ok(format!("({inner})")),
            SubQueryType::Exists => Ok(format!("EXISTS ({inner})")),
            SubQueryType::In => {
                if let Some(te) = test_expr {
                    let e = self.emit_expr(te)?;
                    Ok(format!("{e} IN ({inner})"))
                } else {
                    Ok(format!("IN ({inner})"))
                }
            }
            SubQueryType::Any => {
                if let Some(te) = test_expr {
                    let e = self.emit_expr(te)?;
                    Ok(format!("{e} = ANY ({inner})"))
                } else {
                    Ok(format!("ANY ({inner})"))
                }
            }
            SubQueryType::All => {
                if let Some(te) = test_expr {
                    let e = self.emit_expr(te)?;
                    Ok(format!("{e} = ALL ({inner})"))
                } else {
                    Ok(format!("ALL ({inner})"))
                }
            }
        }
    }

    // ---- Helper: aggregate expression ----

    fn emit_aggregate_expr(&mut self, agg: &AggregateExpr) -> Result<String, TranslationError> {
        let func_name = match agg.function {
            AggregateFunction::Count => "COUNT",
            AggregateFunction::Sum => "SUM",
            AggregateFunction::Avg => "AVG",
            AggregateFunction::Min => "MIN",
            AggregateFunction::Max => "MAX",
            AggregateFunction::StdDev => "STDDEV",
            AggregateFunction::Variance => "VARIANCE",
            AggregateFunction::StringAgg => "STRING_AGG",
            AggregateFunction::ArrayAgg => "ARRAY_AGG",
        };

        let distinct = if agg.distinct { "DISTINCT " } else { "" };

        let arg_str = if let Some(arg) = &agg.arg {
            self.emit_expr(arg)?
        } else {
            "*".to_string()
        };

        let mut sql = format!("{func_name}({distinct}{arg_str})");

        if let Some(alias) = &agg.alias {
            let _ = write!(sql, " AS {}", self.quote_ident(alias));
        }

        Ok(sql)
    }

    // ---- Helper: window expression ----

    fn emit_window_expr(&mut self, wf: &WindowExpr) -> Result<String, TranslationError> {
        let func_name = match wf.function {
            WindowFunction::Avg => "AVG",
            WindowFunction::Sum => "SUM",
            WindowFunction::Count => "COUNT",
            WindowFunction::Min => "MIN",
            WindowFunction::Max => "MAX",
            WindowFunction::RowNumber => "ROW_NUMBER",
            WindowFunction::Rank => "RANK",
            WindowFunction::DenseRank => "DENSE_RANK",
            WindowFunction::PercentRank => "PERCENT_RANK",
            WindowFunction::Ntile => "NTILE",
            WindowFunction::Lag => "LAG",
            WindowFunction::Lead => "LEAD",
            WindowFunction::FirstValue => "FIRST_VALUE",
            WindowFunction::LastValue => "LAST_VALUE",
            WindowFunction::NthValue => "NTH_VALUE",
        };

        let arg_str = if let Some(arg) = &wf.arg {
            self.emit_expr(arg)?
        } else {
            "*".to_string()
        };

        let mut over_parts = Vec::new();

        if !wf.partition_by.is_empty() {
            let parts: Result<Vec<String>, _> =
                wf.partition_by.iter().map(|e| self.emit_expr(e)).collect();
            over_parts.push(format!("PARTITION BY {}", parts?.join(", ")));
        }

        if !wf.order_by.is_empty() {
            let order = self.emit_sort_keys(&wf.order_by)?;
            over_parts.push(format!("ORDER BY {order}"));
        }

        if let Some(frame) = &wf.frame {
            over_parts.push(Self::emit_window_frame(frame));
        }

        let over_clause = over_parts.join(" ");
        let mut sql = format!("{func_name}({arg_str}) OVER ({over_clause})");

        if let Some(alias) = &wf.alias {
            let _ = write!(sql, " AS {}", self.quote_ident(alias));
        }

        Ok(sql)
    }

    fn emit_window_frame(frame: &WindowFrame) -> String {
        let mode = match frame.mode {
            WindowFrameMode::Rows => "ROWS",
            WindowFrameMode::Range => "RANGE",
            WindowFrameMode::Groups => "GROUPS",
        };
        let start = emit_frame_bound(&frame.start);
        let end = emit_frame_bound(&frame.end);
        format!("{mode} BETWEEN {start} AND {end}")
    }

    // ---- Helper: sort keys ----

    fn emit_sort_keys(&mut self, keys: &[SortKey]) -> Result<String, TranslationError> {
        let parts: Result<Vec<String>, _> = keys
            .iter()
            .map(|k| {
                let expr = self.emit_expr(&k.expr)?;
                let dir = match k.direction {
                    SortDirection::Desc => " DESC",
                    SortDirection::Asc => " ASC",
                };
                let nulls = if self.target.supports_nulls_first_last() {
                    match k.nulls {
                        NullOrdering::First => " NULLS FIRST",
                        NullOrdering::Last => " NULLS LAST",
                    }
                } else {
                    ""
                };
                Ok(format!("{expr}{dir}{nulls}"))
            })
            .collect();
        Ok(parts?.join(", "))
    }

    // ---- Helper: projection list ----

    fn emit_projection_list(
        &mut self,
        columns: &[ProjectionColumn],
    ) -> Result<String, TranslationError> {
        let items: Result<Vec<String>, _> = columns
            .iter()
            .map(|pc| {
                let expr = self.emit_expr(&pc.expr)?;
                if let Some(alias) = &pc.alias {
                    Ok(format!("{expr} AS {}", self.quote_ident(alias)))
                } else {
                    Ok(expr)
                }
            })
            .collect();
        Ok(items?.join(", "))
    }

    // ---- Helper: emit inner query as source ----

    fn emit_as_source(&mut self, expr: &RelExpr) -> Result<String, TranslationError> {
        self.emit_rel_expr(expr)
    }

    // ---- Helper: identifier quoting ----

    fn quote_ident(&self, ident: &str) -> String {
        if ident == "*" {
            return "*".to_string();
        }
        let q = self.target.quote_style();
        if q == '`' {
            format!("`{}`", ident.replace('`', "``"))
        } else {
            format!("\"{ident}\"",)
        }
    }
}

fn emit_frame_bound(bound: &WindowFrameBound) -> String {
    match bound {
        WindowFrameBound::UnboundedPreceding => "UNBOUNDED PRECEDING".to_string(),
        WindowFrameBound::Preceding(n) => {
            format!("{n} PRECEDING")
        }
        WindowFrameBound::CurrentRow => "CURRENT ROW".to_string(),
        WindowFrameBound::Following(n) => {
            format!("{n} FOLLOWING")
        }
        WindowFrameBound::UnboundedFollowing => "UNBOUNDED FOLLOWING".to_string(),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::algebra::{NullOrdering, ProjectionColumn, SortDirection, SortKey};
    use ra_core::expr::{ColumnRef, Const};

    fn simple_scan() -> RelExpr {
        RelExpr::Scan {
            table: "users".to_string(),
            alias: None,
        }
    }

    #[test]
    fn emit_simple_scan() {
        let result = emit_sql(&simple_scan(), Dialect::PostgreSql).expect("should emit");
        assert!(result.sql.contains("SELECT"));
        assert!(result.sql.contains("users"));
    }

    #[test]
    fn emit_filter_with_boolean_sqlite() {
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("active"))),
                right: Box::new(Expr::Const(Const::Bool(true))),
            },
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::Sqlite).expect("should emit");
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_limit_to_mssql() {
        let expr = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::MsSql).expect("should emit");
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_concat_mysql() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::BinOp {
                    op: BinOp::Concat,
                    left: Box::new(Expr::Column(ColumnRef::new("first"))),
                    right: Box::new(Expr::Column(ColumnRef::new("last"))),
                },
                alias: None,
            }],
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::MySql).expect("should emit");
        assert!(
            result.sql.contains("CONCAT"),
            "Expected CONCAT in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_concat_mssql() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::BinOp {
                    op: BinOp::Concat,
                    left: Box::new(Expr::Column(ColumnRef::new("first"))),
                    right: Box::new(Expr::Column(ColumnRef::new("last"))),
                },
                alias: None,
            }],
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::MsSql).expect("should emit");
        assert!(result.sql.contains('+'), "Expected + in: {}", result.sql);
    }

    #[test]
    fn emit_union() {
        let expr = RelExpr::Union {
            all: false,
            left: Box::new(simple_scan()),
            right: Box::new(RelExpr::Scan {
                table: "admins".to_string(),
                alias: None,
            }),
        };
        let result = emit_sql(&expr, Dialect::PostgreSql).expect("should emit");
        assert!(
            result.sql.contains("UNION"),
            "Expected UNION in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_sort() {
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("name")),
                direction: SortDirection::Desc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::PostgreSql).expect("should emit");
        assert!(
            result.sql.contains("ORDER BY"),
            "Expected ORDER BY in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("DESC"),
            "Expected DESC in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_distinct() {
        let expr = RelExpr::Distinct {
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::MySql).expect("should emit");
        assert!(
            result.sql.contains("DISTINCT"),
            "Expected DISTINCT in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_cast_postgres_double_colon() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Cast {
                    expr: Box::new(Expr::Column(ColumnRef::new("age"))),
                    target_type: "int".to_string(),
                },
                alias: None,
            }],
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::PostgreSql).expect("should emit");
        assert!(result.sql.contains("::"), "Expected :: in: {}", result.sql);
    }

    #[test]
    fn emit_cast_mysql_uses_cast() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Cast {
                    expr: Box::new(Expr::Column(ColumnRef::new("age"))),
                    target_type: "int".to_string(),
                },
                alias: None,
            }],
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::MySql).expect("should emit");
        assert!(
            result.sql.contains("CAST"),
            "Expected CAST in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_function_length_mssql() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Function {
                    name: "LENGTH".to_string(),
                    args: vec![Expr::Column(ColumnRef::new("name"))],
                },
                alias: None,
            }],
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::MsSql).expect("should emit");
        assert!(
            result.sql.contains("LEN"),
            "Expected LEN in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_cte() {
        let expr = RelExpr::CTE {
            name: "active".to_string(),
            definition: Box::new(RelExpr::Filter {
                predicate: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::new("active"))),
                    right: Box::new(Expr::Const(Const::Bool(true))),
                },
                input: Box::new(simple_scan()),
            }),
            body: Box::new(RelExpr::Scan {
                table: "active".to_string(),
                alias: None,
            }),
        };
        let result = emit_sql(&expr, Dialect::PostgreSql).expect("should emit");
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH in: {}",
            result.sql
        );
    }

    #[test]
    fn emit_except_oracle_uses_minus() {
        let expr = RelExpr::Except {
            all: false,
            left: Box::new(simple_scan()),
            right: Box::new(RelExpr::Scan {
                table: "banned".to_string(),
                alias: None,
            }),
        };
        let result = emit_sql(&expr, Dialect::Oracle).expect("should emit");
        assert!(
            result.sql.contains("MINUS"),
            "Expected MINUS in: {}",
            result.sql
        );
    }
}
